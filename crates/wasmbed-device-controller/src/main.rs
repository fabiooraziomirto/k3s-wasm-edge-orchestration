// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        watcher,
    },
    ResourceExt,
};
use futures_util::StreamExt;
use std::time::Duration;
use tracing::{error, info, warn};
use wasmbed_k8s_resource::{Device, DeviceStatus, DevicePhase, Gateway, GatewayPhase};
use wasmbed_types::GatewayReference;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControllerError {
    #[error("Kubernetes API error: {0}")]
    KubeError(#[from] kube::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("Device error: {0}")]
    DeviceError(String),
}

/// Device Controller
#[derive(Clone)]
pub struct DeviceController {
    client: Client,
    devices: Api<Device>,
}

impl DeviceController {
    pub fn new(client: Client) -> Self {
        Self {
            devices: Api::<Device>::namespaced(client.clone(), "wasmbed"),
            client,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let devices_api = Api::<Device>::namespaced(self.client.clone(), "wasmbed");
        let controller = self.clone();
        
        Controller::new(devices_api, watcher::Config::default())
            .shutdown_on_signal()
            .run(
                move |obj, _ctx| {
                    let controller = controller.clone();
                    async move {
                        controller.reconcile((*obj).clone()).await
                    }
                },
                move |_obj, _err, _ctx| {
                    Action::requeue(Duration::from_secs(30))
                },
                std::sync::Arc::new(()),
            )
            .for_each(|res| async move {
                match res {
                    Ok(o) => info!("reconciled {:?}", o),
                    Err(e) => error!("reconcile failed: {}", e),
                }
            })
            .await;

        Ok(())
    }
}

impl DeviceController {
    pub async fn reconcile(&self, device: Device) -> Result<Action, ControllerError> {
        let name = device.name_any();
        info!("Reconciling device: {}", name);

        match device.status.as_ref().map(|s| &s.phase) {
            Some(DevicePhase::Pending) => {
                self.handle_pending(&device).await?;
            }
            Some(DevicePhase::Enrolling) => {
                self.handle_enrollment(&device).await?;
            }
            Some(DevicePhase::Enrolled) => {
                self.handle_enrolled(&device).await?;
            }
            Some(DevicePhase::Connected) => {
                self.handle_connected(&device).await?;
            }
            Some(DevicePhase::Disconnected) => {
                self.handle_disconnected(&device).await?;
            }
            Some(DevicePhase::Unreachable) => {
                self.handle_unreachable(&device).await?;
            }
            None => {
                // Initialize device status
                self.initialize_device(&device).await?;
            }
        }

        Ok(Action::requeue(Duration::from_secs(30)))
    }
}

impl DeviceController {
    async fn handle_pending(&self, device: &Device) -> Result<(), ControllerError> {
        info!("Handling pending device: {}", device.name_any());
        
        // Move to enrolling phase
        let mut status = device.status.clone().unwrap_or_else(|| DeviceStatus {
            phase: DevicePhase::Pending,
            gateway: None,
            connected_since: None,
            last_heartbeat: None,
            pairing_mode: false,
        });
        
        status.phase = DevicePhase::Enrolling;
        self.update_device_status(device, status).await?;
        
        Ok(())
    }

    async fn initialize_device(&self, device: &Device) -> Result<(), ControllerError> {
        let status = DeviceStatus {
            phase: DevicePhase::Pending,
            gateway: None,
            connected_since: None,
            last_heartbeat: None,
            pairing_mode: false,
        };

        self.update_device_status(device, status).await?;
        info!("Initialized device: {}", device.name_any());
        Ok(())
    }

    async fn handle_enrollment(&self, device: &Device) -> Result<(), ControllerError> {
        info!("Handling enrollment for device: {}", device.name_any());
        
        info!("Device {} - Preferred gateway in spec: {:?}", 
            device.name_any(), device.spec.preferred_gateway);
        
        // Check if there are any active gateways
        let gateways_api = Api::<Gateway>::namespaced(self.client.clone(), "wasmbed");
        let gateways = gateways_api.list(&ListParams::default()).await?;
        
        let active_gateways: Vec<_> = gateways
            .items
            .iter()
            .filter(|g| g.status.as_ref()
                .map(|s| matches!(s.phase, GatewayPhase::Running))
                .unwrap_or(false))
            .collect();
        
        if active_gateways.is_empty() {
            let total_gateways = gateways.items.len();
            let pending_gateways: Vec<_> = gateways
                .items
                .iter()
                .filter(|g| g.status.as_ref()
                    .map(|s| matches!(s.phase, GatewayPhase::Pending))
                    .unwrap_or(true))
                .collect();
            
            if total_gateways > 0 {
                info!("No active gateways found for device {} ({} total, {} pending). Waiting for gateway to become active...", 
                    device.name_any(), total_gateways, pending_gateways.len());
            } else {
                info!("No gateways found for device {}, keeping in enrolling state", device.name_any());
            }
            // Keep device in enrolling state until a gateway becomes available
            return Ok(());
        }
        
        // Check if device has a preferred gateway specified in spec
        let gateway = if let Some(preferred_gateway_name) = device.spec.preferred_gateway.as_ref() {
            info!("Device {} has preferred gateway: {}", device.name_any(), preferred_gateway_name);
            // Try to find the preferred gateway
            if let Some(preferred) = active_gateways.iter().find(|g| g.name_any() == preferred_gateway_name.as_str()) {
                info!("Using preferred gateway {} for device {}", preferred_gateway_name, device.name_any());
                preferred
            } else {
                warn!("Preferred gateway {} not found or not active for device {}, falling back to round-robin", 
                    preferred_gateway_name, device.name_any());
                // Fall back to round-robin
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                device.name_any().hash(&mut hasher);
                let device_hash = hasher.finish() as usize;
                let gateway_index = device_hash % active_gateways.len();
                active_gateways[gateway_index]
            }
        } else {
            info!("Device {} has no preferred gateway, using round-robin", device.name_any());
            // No preferred gateway - use round-robin
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            device.name_any().hash(&mut hasher);
            let device_hash = hasher.finish() as usize;
            let gateway_index = device_hash % active_gateways.len();
            active_gateways[gateway_index]
        };
        
        let gateway_name = gateway.name_any();
        
        info!("Selected gateway {} for device {} (preferred: {})", 
            gateway_name, 
            device.name_any(),
            device.spec.preferred_gateway.as_ref().map(|s| s.as_str()).unwrap_or("none"));
        
        // Simulate enrollment process with gateway
        tokio::time::sleep(Duration::from_secs(2)).await;

        let mut status = device.status.clone().unwrap_or_else(|| DeviceStatus {
            phase: DevicePhase::Enrolling,
            gateway: None,
            connected_since: None,
            last_heartbeat: None,
            pairing_mode: false,
        });

        status.phase = DevicePhase::Enrolled;
        // Get gateway endpoint from gateway spec
        let gateway_endpoint = gateway.spec.endpoint.clone();
        status.gateway = Some(GatewayReference {
            name: gateway_name.clone(),
            endpoint: gateway_endpoint,
            connected_at: None,
        });
        status.pairing_mode = true;

        self.update_device_status(device, status).await?;
        info!("Device {} enrolled successfully with gateway {}", device.name_any(), gateway_name);
        Ok(())
    }

    async fn handle_enrolled(&self, device: &Device) -> Result<(), ControllerError> {
        info!("Device {} is enrolled", device.name_any());
        
        // If device doesn't have a gateway assigned, assign one now
        let mut status = device.status.clone().unwrap_or_else(|| DeviceStatus {
            phase: DevicePhase::Enrolled,
            gateway: None,
            connected_since: None,
            last_heartbeat: None,
            pairing_mode: false,
        });
        
        // Check if gateway is missing, incomplete, or doesn't match preferred gateway
        let current_gateway_name = status.gateway.as_ref().map(|g| g.name.as_str());
        let preferred_gateway_name = device.spec.preferred_gateway.as_ref().map(|s| s.as_str());
        
        info!("Device {} - Current gateway: {:?}, Preferred gateway: {:?}", 
            device.name_any(), current_gateway_name, preferred_gateway_name);
        
        // Check if we need to assign/change gateway:
        // 1. No gateway assigned
        // 2. Gateway endpoint is empty
        // 3. Preferred gateway is specified and different from current gateway
        let endpoint_empty = status.gateway.as_ref()
            .map(|g| g.endpoint.is_empty())
            .unwrap_or(true);
        let preferred_mismatch = preferred_gateway_name.is_some() && 
            current_gateway_name != preferred_gateway_name;
        
        let needs_gateway = status.gateway.is_none() || 
            endpoint_empty ||
            preferred_mismatch;
        
        info!("Device {} - Needs gateway: {} (gateway.is_none: {}, endpoint_empty: {}, preferred_mismatch: {})", 
            device.name_any(), 
            needs_gateway,
            status.gateway.is_none(),
            endpoint_empty,
            preferred_mismatch);
        
        if needs_gateway {
            // Check if there are any active gateways
            let gateways_api = Api::<Gateway>::namespaced(self.client.clone(), "wasmbed");
            let gateways = gateways_api.list(&ListParams::default()).await?;
            
            let active_gateways: Vec<_> = gateways
                .items
                .iter()
                .filter(|g| g.status.as_ref()
                    .map(|s| matches!(s.phase, GatewayPhase::Running))
                    .unwrap_or(false))
                .collect();
            
            if !active_gateways.is_empty() {
                // Check if device has a preferred gateway specified in spec
                let gateway = if let Some(preferred_gateway_name) = device.spec.preferred_gateway.as_ref() {
                    // Try to find the preferred gateway
                    if let Some(preferred) = active_gateways.iter().find(|g| g.name_any() == preferred_gateway_name.as_str()) {
                        info!("Using preferred gateway {} for enrolled device {}", preferred_gateway_name, device.name_any());
                        preferred
                    } else {
                        warn!("Preferred gateway {} not found or not active for device {}, falling back to round-robin", 
                            preferred_gateway_name, device.name_any());
                        // Fall back to round-robin
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        let mut hasher = DefaultHasher::new();
                        device.name_any().hash(&mut hasher);
                        let device_hash = hasher.finish() as usize;
                        let gateway_index = device_hash % active_gateways.len();
                        active_gateways[gateway_index]
                    }
                } else {
                    // No preferred gateway - use round-robin
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    device.name_any().hash(&mut hasher);
                    let device_hash = hasher.finish() as usize;
                    let gateway_index = device_hash % active_gateways.len();
                    active_gateways[gateway_index]
                };
                
                let gateway_name = gateway.name_any();
                let gateway_endpoint = gateway.spec.endpoint.clone();
                
                info!("Assigning gateway {} to enrolled device {} (preferred: {})", 
                    gateway_name, 
                    device.name_any(),
                    device.spec.preferred_gateway.as_ref().map(|s| s.as_str()).unwrap_or("none"));
                status.gateway = Some(GatewayReference {
                    name: gateway_name,
                    endpoint: gateway_endpoint,
                    connected_at: None,
                });
                status.pairing_mode = true;
                self.update_device_status(device, status).await?;
            } else {
                warn!("Device {} is enrolled but no active gateways available", device.name_any());
            }
        }
        
        // Start Renode automatically for the device
        // Renode is managed as a standalone process by RenodeManager, not as a Docker container
        // The RenodeManager will spawn Renode as a process (not a pod)
        self.start_renode_device(device).await?;
        
        Ok(())
    }
    
    async fn start_renode_device(&self, device: &Device) -> Result<(), ControllerError> {
        let device_id = device.name_any();
        info!("Starting Renode for device: {}", device_id);
        
        // Call the API server's endpoint to start Renode
        // The API server has access to RenodeManager
        // Default to port 3001 (API server) not 3000 (dashboard frontend)
        // In Kubernetes/Kind, use 172.18.0.1 (Docker gateway IP) to reach host services
        let api_server_url = std::env::var("WASMBED_API_SERVER_URL")
            .unwrap_or_else(|_| "http://172.18.0.1:3001".to_string());
        
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ControllerError::DeviceError(format!("Failed to create HTTP client: {}", e)))?;
        
        let url = format!("{}/api/v1/devices/{}/renode/start", api_server_url, device_id);
        info!("Calling API server to start Renode: {}", url);
        
        let response = client
            .post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| ControllerError::DeviceError(format!("Failed to call API server: {} (URL: {})", e, url)))?;
        
        let status = response.status();
        if status.is_success() {
            let response_text = response.text().await.unwrap_or_else(|_| "".to_string());
            info!("Renode started successfully for device {}: {}", device_id, response_text);
        } else {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            warn!("Failed to start Renode for device {}: HTTP {} - {}", device_id, status, error_text);
            return Err(ControllerError::DeviceError(format!("API server returned error: HTTP {} - {}", status, error_text)));
        }
        
        Ok(())
    }

    async fn handle_connected(&self, device: &Device) -> Result<(), ControllerError> {
        info!("Device {} is connected", device.name_any());
        
        // Check if the gateway referenced by this device still exists
        if let Some(gateway_ref) = &device.status.as_ref().and_then(|s| s.gateway.as_ref()) {
            info!("Device {} has gateway reference: name={}, endpoint={}", 
                device.name_any(), gateway_ref.name, gateway_ref.endpoint);
            // GatewayReference now has fields: name, endpoint, connected_at
            let gateways_api = Api::<Gateway>::namespaced(self.client.clone(), "wasmbed");
            match gateways_api.get(&gateway_ref.name).await {
                Ok(gateway) => {
                    // Gateway exists, check if it's running
                    if let Some(gateway_status) = &gateway.status {
                        if !matches!(gateway_status.phase, GatewayPhase::Running) {
                            warn!("Device {} is connected to gateway {} which is not running (phase: {:?}). Disconnecting device.", 
                                device.name_any(), &gateway_ref.name, gateway_status.phase);
                            let mut status = device.status.clone().unwrap();
                            status.phase = DevicePhase::Disconnected;
                            status.gateway = None;
                            self.update_device_status(device, status).await?;
                            return Ok(());
                        }
                    }
                    
                    // Check if TLS connection is actually active by querying gateway API
                    let gateway_endpoint = if gateway_ref.endpoint.is_empty() {
                        let endpoint = "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string();
                        // Persist a usable endpoint to avoid later reconnect attempts with empty URL.
                        if let Some(current) = &device.status {
                            let mut updated = current.clone();
                            if let Some(gw) = &mut updated.gateway {
                                gw.endpoint = endpoint.clone();
                                if let Err(e) = self.update_device_status(device, updated).await {
                                    warn!("Failed to persist default gateway endpoint for device {}: {}", device.name_any(), e);
                                }
                            }
                        }
                        endpoint
                    } else {
                        let ep = gateway_ref.endpoint.clone();
                        if ep.starts_with("http://") || ep.starts_with("https://") {
                            ep
                        } else {
                            format!("http://{}", ep)
                        }
                    };
                    
                    // Verify TLS connection is active
                    let client = reqwest::Client::new();
                    let device_id = device.name_any();
                    let gateway_url = format!("{}/api/v1/devices", gateway_endpoint);
                    match client
                        .get(&gateway_url)
                        .timeout(std::time::Duration::from_secs(5))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(devices_response) = resp.json::<serde_json::Value>().await {
                                if let Some(devices) = devices_response["devices"].as_array() {
                                    let device_found = devices.iter().find(|d| {
                                        d["device_id"].as_str() == Some(&device_id)
                                    });
                                    
                                    if let Some(device_info) = device_found {
                                        let tls_connected = device_info["tls_connected"]
                                            .as_bool()
                                            .unwrap_or(false);
                                        
                                        if !tls_connected {
                                            // Do not force-disconnect here: gateway-side status/heartbeat is authoritative,
                                            // and transient races can briefly report false negatives.
                                            warn!("Device {} is marked as Connected but gateway currently reports tls_connected=false. Keeping state and waiting for next reconcile.", device_id);
                                        }
                                    } else {
                                        // Keep state on temporary lookup misses; the gateway will update status if truly disconnected.
                                        warn!("Device {} is marked as Connected but not found in gateway list. Keeping state and retrying on next reconcile.", device_id);
                                    }
                                }
                            }
                        }
                        Ok(_) => {
                            warn!("Failed to check TLS connection for device {}: gateway returned non-success status", device_id);
                        }
                        Err(e) => {
                            warn!("Failed to check TLS connection for device {}: {}. Continuing...", device_id, e);
                        }
                    }
                }
                Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, .. })) => {
                    // Gateway doesn't exist anymore, disconnect the device
                    warn!("Device {} is connected to gateway {} which no longer exists. Disconnecting device.", 
                        device.name_any(), &gateway_ref.name);
                    let mut status = device.status.clone().unwrap();
                    status.phase = DevicePhase::Disconnected;
                    status.gateway = None;
                    self.update_device_status(device, status).await?;
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to check gateway {} for device {}: {}. Continuing...", 
                        &gateway_ref.name, device.name_any(), e);
                }
            }
        }
        
        // Try to start Renode if not already running
        // This handles devices that were connected before being enrolled
        // or devices that were enrolled but Renode failed to start
        if let Err(e) = self.start_renode_device(device).await {
            warn!("Failed to start Renode for connected device {}: {}", device.name_any(), e);
            // Don't fail the reconciliation if Renode fails to start
            // The device can still be connected without Renode
        }
        
        // Update heartbeat only if device has active TLS connection
        // (if we got here, TLS connection is active)
        let mut status = device.status.clone().unwrap();
        status.last_heartbeat = Some(chrono::Utc::now());
        
        self.update_device_status(device, status).await?;
        Ok(())
    }

    async fn handle_disconnected(&self, device: &Device) -> Result<(), ControllerError> {
        let device_id = device.name_any();
        info!("Device {} disconnected, attempting automatic reconnection", device_id);
        
        // Get gateway endpoint from device status or use default
        let gateway_endpoint = if let Some(gateway_ref) = device.status.as_ref().and_then(|s| s.gateway.as_ref()) {
            if gateway_ref.endpoint.is_empty() {
                "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string()
            } else {
                gateway_ref.endpoint.clone()
            }
        } else {
            // Try to find a running gateway
            let gateways_api = Api::<Gateway>::namespaced(self.client.clone(), "wasmbed");
            match gateways_api.list(&kube::api::ListParams::default()).await {
                Ok(gateways) => {
                    // Find first running gateway
                    gateways.items.iter()
                        .find_map(|g| {
                            if let Some(status) = &g.status {
                                if matches!(status.phase, GatewayPhase::Running) {
                                    let endpoint = if g.spec.endpoint.is_empty() {
                                        "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string()
                                    } else {
                                        g.spec.endpoint.clone()
                                    };
                                    return Some(endpoint);
                                }
                            }
                            None
                        })
                        .unwrap_or_else(|| "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string())
                }
                Err(_) => "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string(),
            }
        };
        
        // Register device with gateway to enable reconnection
        info!("Registering device {} with gateway at {} for reconnection", device_id, gateway_endpoint);
        let gateway_url = format!("{}/api/v1/devices/{}/connect", gateway_endpoint, device_id);
        
        let client = reqwest::Client::new();
        match client
            .post(&gateway_url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                info!("Device {} successfully re-registered with gateway", device_id);
                // The device will transition to Connected when TLS connection is established
            }
            Ok(resp) => {
                warn!("Gateway reconnection returned status {} for device {}", resp.status(), device_id);
            }
            Err(e) => {
                warn!("Failed to reconnect device {} to gateway: {}. Will retry on next reconciliation.", device_id, e);
            }
        }
        
        Ok(())
    }

    async fn handle_unreachable(&self, device: &Device) -> Result<(), ControllerError> {
        let device_id = device.name_any();
        info!("Device {} is unreachable (heartbeat timeout), attempting automatic recovery", device_id);

        // Same reconnection as handle_disconnected: re-register with Gateway so when device reconnects TLS and sends heartbeat, Gateway can mark Connected
        let gateway_endpoint = if let Some(gateway_ref) = device.status.as_ref().and_then(|s| s.gateway.as_ref()) {
            if gateway_ref.endpoint.is_empty() {
                "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string()
            } else {
                gateway_ref.endpoint.clone()
            }
        } else {
            let gateways_api = Api::<Gateway>::namespaced(self.client.clone(), "wasmbed");
            match gateways_api.list(&kube::api::ListParams::default()).await {
                Ok(gateways) => gateways.items.iter()
                    .find_map(|g| {
                        if let Some(status) = &g.status {
                            if matches!(status.phase, GatewayPhase::Running) {
                                return Some(if g.spec.endpoint.is_empty() {
                                    "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string()
                                } else {
                                    g.spec.endpoint.clone()
                                });
                            }
                        }
                        None
                    })
                    .unwrap_or_else(|| "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string()),
                Err(_) => "http://gateway-1-service.wasmbed.svc.cluster.local:8080".to_string(),
            }
        };

        let gateway_url = format!("{}/api/v1/devices/{}/connect", gateway_endpoint, device_id);
        let client = reqwest::Client::new();
        match client.post(&gateway_url).timeout(std::time::Duration::from_secs(10)).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("Device {} re-registered with gateway for recovery; will become Connected when TLS heartbeat is received", device_id);
            }
            Ok(resp) => warn!("Gateway recovery returned status {} for device {}", resp.status(), device_id),
            Err(e) => warn!("Failed to re-register device {} with gateway: {}. Will retry on next reconciliation.", device_id, e),
        }
        Ok(())
    }

    async fn update_device_status(&self, device: &Device, status: DeviceStatus) -> Result<(), ControllerError> {
        let patch = serde_json::json!({
            "status": status
        });

        let params = PatchParams::apply("wasmbed-device-controller");
        let patch = Patch::Merge(patch);
        
        // Try patch_status first, fallback to patch if status doesn't exist
        match self.devices.patch_status(&device.name_any(), &params, &patch).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // If patch_status fails, try regular patch
                warn!("patch_status failed, trying regular patch: {}", e);
                self.devices
                    .patch(&device.name_any(), &params, &patch)
                    .await?;
                Ok(())
            }
        }
    }

}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize rustls crypto provider
    use rustls::crypto::aws_lc_rs::default_provider;
    rustls::crypto::CryptoProvider::install_default(default_provider())
        .expect("Failed to install default crypto provider");
    
    tracing_subscriber::fmt::init();
    
    let client = Client::try_default().await?;
    let controller = DeviceController::new(client);
    
    info!("Starting Device Controller...");
    controller.run().await?;
    
    Ok(())
}
