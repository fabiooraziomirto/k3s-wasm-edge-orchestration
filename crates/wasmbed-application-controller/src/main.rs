// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use kube::{
    api::{Api, ListParams, Patch, PatchParams, PostParams},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        watcher,
    },
    ResourceExt,
};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use futures_util::StreamExt;
use std::time::Duration;
use tracing::{error, info, warn};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControllerError {
    #[error("Kubernetes API error: {0}")]
    KubeError(#[from] kube::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("Application error: {0}")]
    ApplicationError(String),
    #[error("Anyhow error: {0}")]
    AnyhowError(#[from] anyhow::Error),
}

/// Application Controller
#[derive(Clone)]
pub struct ApplicationController {
    client: Client,
    applications: Api<wasmbed_k8s_resource::Application>,
    devices: Api<wasmbed_k8s_resource::Device>,
    gateways: Api<wasmbed_k8s_resource::Gateway>,
    deployments: Api<Deployment>,
    gateway_endpoint: String,
    // In-memory phase cache: overrides stale k8s status (kube-rs GET does not always
    // reflect a just-written status subresource, causing spurious Creating loops).
    app_phases: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, wasmbed_k8s_resource::ApplicationPhase>>>,
}

impl ApplicationController {
    pub fn new(client: Client) -> Self {
        // Get gateway endpoint from environment variable or use default
        let gateway_endpoint = std::env::var("GATEWAY_ENDPOINT")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "http://localhost:8080".to_string());
        
        Self {
            applications: Api::<wasmbed_k8s_resource::Application>::namespaced(client.clone(), "wasmbed"),
            devices: Api::<wasmbed_k8s_resource::Device>::namespaced(client.clone(), "wasmbed"),
            gateways: Api::<wasmbed_k8s_resource::Gateway>::namespaced(client.clone(), "wasmbed"),
            deployments: Api::<Deployment>::namespaced(client.clone(), "wasmbed"),
            client,
            gateway_endpoint,
            app_phases: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let applications_api = Api::<wasmbed_k8s_resource::Application>::namespaced(self.client.clone(), "wasmbed");
        let controller = self.clone();
        
        Controller::new(applications_api, watcher::Config::default())
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

    pub async fn reconcile(&self, application: wasmbed_k8s_resource::Application) -> Result<Action, ControllerError> {
        let name = application.name_any();
        info!("Reconciling application: {}", name);

        // Always fetch fresh application from Kubernetes to get latest status
        let application = match self.applications.get(&name).await {
            Ok(app) => app,
            Err(e) => {
                error!("Failed to get application {}: {}", name, e);
                return Err(ControllerError::KubeError(e));
            }
        };

        // Get current phase from status
        let k8s_phase = application
            .status()
            .map(|s| s.phase)
            .unwrap_or(wasmbed_k8s_resource::ApplicationPhase::Creating);

        // Prefer in-memory phase when it shows progress beyond Creating.
        // kube-rs api.get() returns the spec resource; the status subresource
        // may lag or not deserialize correctly, causing spurious Creating reads.
        let phase = {
            let phases = self.app_phases.read().await;
            match phases.get(&name).copied() {
                Some(p) if p != wasmbed_k8s_resource::ApplicationPhase::Creating => p,
                _ => k8s_phase,
            }
        };
        
        info!("Application {} - Current phase: {:?}", name, phase);
        
        // Route to correct handler based on phase
        match phase {
            wasmbed_k8s_resource::ApplicationPhase::Creating => {
                self.handle_creating(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::Deploying => {
                self.handle_deploying(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::Running => {
                self.handle_running(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::PartiallyRunning => {
                self.handle_partially_running(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::Failed => {
                self.handle_failed(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::Stopping => {
                self.handle_stopping(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::Stopped => {
                self.handle_stopped(&application).await?;
            }
            wasmbed_k8s_resource::ApplicationPhase::Deleting => {
                self.handle_deleting(&application).await?;
            }
        }
        
        Ok(Action::requeue(Duration::from_secs(30)))
    }

    async fn get_application_status_with_fallback(&self, name: &str, application: &wasmbed_k8s_resource::Application) -> Result<wasmbed_k8s_resource::ApplicationStatus, ControllerError> {
        // CRITICAL FIX: Api::get() doesn't deserialize the status field correctly when using status subresource.
        // We use a simple workaround: if we just set the status to "Deploying" in handle_creating,
        // we track this in memory and immediately return "Deploying" on the next reconciliation.
        // This avoids the deserialization issue entirely.
        
        // First, try to get status from the deserialized object
        if let Some(status) = application.status() {
            if status.phase != wasmbed_k8s_resource::ApplicationPhase::Creating {
                // If we have a status and it's not Creating, trust it
                info!("Application {} - Using status from deserialized object, phase: {:?}", name, status.phase);
                return Ok(status.clone());
            }
        }
        
        if let Some(status) = application.status() {
            if status.phase != wasmbed_k8s_resource::ApplicationPhase::Creating {
                // If we have a status and it's not Creating, trust it
                info!("Application {} - Using status() method, phase: {:?}", name, status.phase);
                return Ok(status.clone());
            }
        }
        
        // If status is Creating or None, check if we recently moved it to Deploying
        // by checking the resource version or by re-fetching with a fresh API call
        // For now, we'll use a simpler approach: if status is Creating, check if
        // we can get a fresh status by calling Api::get() again, but this time
        // we'll parse the raw response if needed.
        
        // Actually, the simplest fix: if we're in Creating phase but we know we just
        // set it to Deploying, we can check the actual Kubernetes state by making
        // a direct API call using reqwest to the Kubernetes API server.
        // But this requires knowing the API server URL and having proper auth.
        
        // SIMPLEST FIX: Just always assume that if status is Creating, we should
        // check by re-fetching the application. But we already do that in reconcile().
        // The real issue is that even after re-fetching, the status is still Creating.
        
        // Re-fetch the application to get fresh status
        match self.applications.get(name).await {
            Ok(fresh_app) => {
                // Try status() method first
                if let Some(status) = fresh_app.status() {
                    info!("Application {} - Got status from fresh fetch using status(), phase: {:?}", name, status.phase);
                    return Ok(status.clone());
                }
                // Status is only available via status() method
                // If status() returns None, create default status
            },
            Err(e) => {
                warn!("Application {} - Failed to re-fetch: {}", name, e);
            }
        }
        
        // Fallback: return Creating if we can't determine the status
        warn!("Application {} - Could not determine status, defaulting to Creating", name);
        Ok(wasmbed_k8s_resource::ApplicationStatus {
            phase: wasmbed_k8s_resource::ApplicationPhase::Creating,
            device_statuses: None,
            statistics: None,
            last_updated: Some(chrono::Utc::now().to_rfc3339()),
            error: None,
        })
    }

    async fn handle_creating(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        let name = application.name_any();
        info!("Handling creating application: {}", name);
        
        // If the in-memory map says this app is already past Creating, skip.
        {
            let phases = self.app_phases.read().await;
            if let Some(&p) = phases.get(&name) {
                if p != wasmbed_k8s_resource::ApplicationPhase::Creating {
                    info!("Application {} already in phase {:?} (memory), skipping handle_creating", name, p);
                    return Ok(());
                }
            }
        }
        
        // Find target devices
        let target_devices = self.find_target_devices(&application.spec.target_devices).await?;
        
        if target_devices.is_empty() {
            warn!("No target devices found for application: {}", name);
            return Ok(());
        }

        // Eagerly set in-memory phase to Deploying BEFORE the status patch
        // so that any immediate reconcile triggered by the patch sees Deploying.
        {
            let mut phases = self.app_phases.write().await;
            phases.insert(name.clone(), wasmbed_k8s_resource::ApplicationPhase::Deploying);
        }
        
        // Update status to deploying
        let status = wasmbed_k8s_resource::ApplicationStatus {
            phase: wasmbed_k8s_resource::ApplicationPhase::Deploying,
            device_statuses: None,
            statistics: Some(wasmbed_k8s_resource::ApplicationStatistics {
                total_devices: target_devices.len() as u32,
                deployed_devices: 0,
                running_devices: 0,
                failed_devices: 0,
                stopped_devices: 0,
            }),
            last_updated: Some(chrono::Utc::now().to_rfc3339()),
            error: None,
        };

        self.update_application_status(application, status).await?;
        
        info!("Application {} moved to deploying phase", name);
        Ok(())
    }

    async fn handle_deploying(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        info!("Handling deploying application: {}", application.name_any());
        
        // Find target devices
        let target_devices = self.find_target_devices(&application.spec.target_devices).await?;
        
        if target_devices.is_empty() {
            warn!("No target devices found for application: {}", application.name_any());
            return Ok(());
        }

        // Deploy to each target device via gateway
        let mut device_statuses = std::collections::BTreeMap::new();
        let mut running_count = 0;
        let mut failed_count = 0;
        
        for device in &target_devices {
            let device_id = device.name_any();
            
            // Check if device is connected (status should be Connected)
            let device_phase = device.status.as_ref()
                .map(|s| &s.phase)
                .unwrap_or(&wasmbed_k8s_resource::DevicePhase::Pending);
            if device_phase != &wasmbed_k8s_resource::DevicePhase::Connected {
                warn!("Device {} is not connected (phase: {:?}), skipping deployment", device_id, device_phase);
                let device_status = wasmbed_k8s_resource::DeviceApplicationStatus {
                    status: wasmbed_k8s_resource::DeviceApplicationPhase::Failed,
                    last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                    metrics: None,
                    error: Some("Device not connected".to_string()),
                    restart_count: 0,
                };
                device_statuses.insert(device_id, device_status);
                failed_count += 1;
                continue;
            }

            // Get the gateway endpoint from the device's gateway reference
            // If endpoint is a Renode endpoint (127.0.0.1:port), construct gateway service DNS endpoint
            let gateway_endpoint = {
                if let Some(gateway_ref) = device.status.as_ref().and_then(|s| s.gateway.as_ref()) {
                    let endpoint = gateway_ref.endpoint.trim();
                    let gateway_name = &gateway_ref.name;
                    
                    // Check if endpoint is a Renode endpoint (127.0.0.1:port) or localhost
                    if endpoint.is_empty() {
                        // Empty endpoint in status can happen on transient state updates.
                        // Fall back to the gateway service DNS endpoint.
                        format!("{}-service.wasmbed.svc.cluster.local:8080", gateway_name)
                    } else if endpoint.starts_with("127.0.0.1:") || endpoint.starts_with("localhost:") {
                        // Endpoint is a Renode endpoint, construct gateway service DNS endpoint
                        // Format: {gateway-name}-service.wasmbed.svc.cluster.local:8080
                        format!("{}-service.wasmbed.svc.cluster.local:8080", gateway_name)
                    } else if endpoint.contains("svc.cluster.local") {
                        // Valid Kubernetes service DNS endpoint, use as-is
                        endpoint.to_string()
                    } else {
                        // Use endpoint as-is
                        endpoint.to_string()
                    }
                } else {
                    // No gateway reference, use default
                    self.gateway_endpoint.clone()
                }
            };
            
            // Deploy application to device via gateway
            match self.deploy_application_to_device_with_gateway(&device_id, application, &gateway_endpoint).await {
                Ok(_) => {
                    info!("Successfully deployed application {} to device {}", application.name_any(), device_id);
                    let device_status = wasmbed_k8s_resource::DeviceApplicationStatus {
                        status: wasmbed_k8s_resource::DeviceApplicationPhase::Running,
                        last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                        metrics: None,
                        error: None,
                        restart_count: 0,
                    };
                    device_statuses.insert(device_id, device_status);
                    running_count += 1;
                }
                Err(e) => {
                    error!("Failed to deploy application {} to device {}: {}", application.name_any(), device_id, e);
                    let device_status = wasmbed_k8s_resource::DeviceApplicationStatus {
                        status: wasmbed_k8s_resource::DeviceApplicationPhase::Failed,
                        last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                        metrics: None,
                        error: Some(e.to_string()),
                        restart_count: 0,
                    };
                    device_statuses.insert(device_id, device_status);
                    failed_count += 1;
                }
            }
        }

        // Update application status based on deployment results
        let phase = if running_count == target_devices.len() {
            wasmbed_k8s_resource::ApplicationPhase::Running
        } else if running_count > 0 {
            wasmbed_k8s_resource::ApplicationPhase::PartiallyRunning
        } else if failed_count > 0 {
            wasmbed_k8s_resource::ApplicationPhase::Failed
        } else {
            wasmbed_k8s_resource::ApplicationPhase::Deploying
        };

        let status = wasmbed_k8s_resource::ApplicationStatus {
            phase,
            device_statuses: Some(device_statuses),
            statistics: Some(wasmbed_k8s_resource::ApplicationStatistics {
                total_devices: target_devices.len() as u32,
                deployed_devices: running_count as u32,
                running_devices: running_count as u32,
                failed_devices: failed_count as u32,
                stopped_devices: 0,
            }),
            last_updated: Some(chrono::Utc::now().to_rfc3339()),
            error: if failed_count > 0 {
                Some(format!("Failed to deploy to {} device(s)", failed_count))
            } else {
                None
            },
        };

        // update_application_status updates app_phases for all terminal phases.
        self.update_application_status(application, status).await?;
        info!("Application {} deployment completed: {} running, {} failed", 
              application.name_any(), running_count, failed_count);
        Ok(())
    }

    async fn handle_running(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        info!("Application {} is running (stable)", application.name_any());

        let current_phase = application
            .status()
            .map(|status| status.phase)
            .unwrap_or(wasmbed_k8s_resource::ApplicationPhase::Creating);

        if current_phase != wasmbed_k8s_resource::ApplicationPhase::Running {
            info!(
                "Application {} status is stale ({:?}), reconciling it back to Running",
                application.name_any(),
                current_phase
            );

            let status = wasmbed_k8s_resource::ApplicationStatus {
                phase: wasmbed_k8s_resource::ApplicationPhase::Running,
                device_statuses: application.status().and_then(|status| status.device_statuses.clone()),
                statistics: application.status().and_then(|status| status.statistics.clone()),
                last_updated: Some(chrono::Utc::now().to_rfc3339()),
                error: None,
            };

            self.update_application_status(application, status).await?;
            return Ok(());
        }

        // Refresh in-memory phase without writing to k8s.
        // Writing to k8s on every reconcile would trigger a watch event that
        // immediately re-queues the reconcile, creating a tight loop.
        // The 30-second requeue is sufficient for steady-state monitoring.
        {
            let mut phases = self.app_phases.write().await;
            phases.insert(application.name_any(), wasmbed_k8s_resource::ApplicationPhase::Running);
        }
        Ok(())
    }

    async fn handle_partially_running(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        info!("Application {} is partially running", application.name_any());
        Ok(())
    }

    async fn handle_failed(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        warn!("Application {} is in failed state, checking if devices are now connected for retry", application.name_any());
        
        // Find target devices
        let target_devices = self.find_target_devices(&application.spec.target_devices).await?;
        
        if target_devices.is_empty() {
            warn!("No target devices found for application: {}", application.name_any());
            return Ok(());
        }
        
        // Check if any devices are now connected
        let mut has_connected_devices = false;
        for device in &target_devices {
            let device_phase = device.status.as_ref()
                .map(|s| &s.phase)
                .unwrap_or(&wasmbed_k8s_resource::DevicePhase::Pending);
            if device_phase == &wasmbed_k8s_resource::DevicePhase::Connected {
                has_connected_devices = true;
                break;
            }
        }
        
        // If devices are now connected, retry deployment by moving back to Deploying phase
        if has_connected_devices {
            info!("Application {} has connected devices, retrying deployment", application.name_any());
            let status = wasmbed_k8s_resource::ApplicationStatus {
                phase: wasmbed_k8s_resource::ApplicationPhase::Deploying,
                device_statuses: None,
                statistics: Some(wasmbed_k8s_resource::ApplicationStatistics {
                    total_devices: target_devices.len() as u32,
                    deployed_devices: 0,
                    running_devices: 0,
                    failed_devices: 0,
                    stopped_devices: 0,
                }),
                last_updated: Some(chrono::Utc::now().to_rfc3339()),
                error: None,
            };
            self.update_application_status(application, status).await?;
        } else {
            // No connected devices yet, keep in failed state but log for debugging
            let device_phases: Vec<String> = target_devices.iter()
                .map(|d| {
                    d.status.as_ref()
                        .map(|s| format!("{:?}", s.phase))
                        .unwrap_or_else(|| "Pending".to_string())
                })
                .collect();
            info!("Application {} still in failed state - device phases: {:?}", 
                  application.name_any(), device_phases);
        }
        
        Ok(())
    }

    async fn handle_stopping(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        info!("Application {} is stopping", application.name_any());

        {
            let mut phases = self.app_phases.write().await;
            phases.insert(application.name_any(), wasmbed_k8s_resource::ApplicationPhase::Stopping);
        }
        
        // Update status to stopped
        let status = wasmbed_k8s_resource::ApplicationStatus {
            phase: wasmbed_k8s_resource::ApplicationPhase::Stopped,
            device_statuses: None,
            statistics: None,
            last_updated: Some(chrono::Utc::now().to_rfc3339()),
            error: None,
        };
        
        self.update_application_status(application, status).await?;
        Ok(())
    }

    async fn handle_stopped(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        info!("Application {} is stopped", application.name_any());
        {
            let mut phases = self.app_phases.write().await;
            phases.insert(application.name_any(), wasmbed_k8s_resource::ApplicationPhase::Stopped);
        }
        Ok(())
    }

    async fn handle_deleting(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        info!("Application {} is being deleted", application.name_any());
        {
            let mut phases = self.app_phases.write().await;
            phases.remove(&application.name_any());
        }
        Ok(())
    }

    async fn find_target_devices(&self, target_devices: &wasmbed_k8s_resource::TargetDevices) -> Result<Vec<wasmbed_k8s_resource::Device>, ControllerError> {
        let all_devices = self.devices.list(&ListParams::default()).await?;
        let mut matching_devices = vec![];

        // If all_devices is true, return all devices
        if target_devices.all_devices.unwrap_or(false) {
            return Ok(all_devices.items);
        }

        // If device_names is specified, match by name
        if let Some(device_names) = &target_devices.device_names {
            for device in all_devices.items {
                if device_names.contains(&device.name_any()) {
                    matching_devices.push(device);
                }
            }
        }

        Ok(matching_devices)
    }

    async fn deploy_application_to_device(&self, device_id: &str, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        // Use default gateway endpoint
        self.deploy_application_to_device_with_gateway(device_id, application, &self.gateway_endpoint).await
    }
    
    async fn deploy_application_to_device_with_gateway(&self, device_id: &str, application: &wasmbed_k8s_resource::Application, gateway_endpoint: &str) -> Result<(), ControllerError> {
        info!("Deploying application {} to device {} via gateway {}", application.name_any(), device_id, gateway_endpoint);
        
        // wasm_bytes is already base64 encoded in the spec
        let wasm_bytes_base64 = &application.spec.wasm_bytes;
        
        // Create deployment request
        let deployment_request = serde_json::json!({
            "app_id": application.name_any(),
            "name": application.spec.name,
            "wasm_bytes": wasm_bytes_base64
        });
        
        // Normalize endpoint to avoid malformed URLs like "http://http//...".
        let endpoint_no_scheme = gateway_endpoint
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let endpoint_with_http_port = if endpoint_no_scheme.trim().is_empty() {
            "gateway-1-service.wasmbed.svc.cluster.local:8080".to_string()
        } else if endpoint_no_scheme.ends_with(":8443") {
            endpoint_no_scheme.replacen(":8443", ":8080", 1)
        } else if endpoint_no_scheme.contains(':') {
            endpoint_no_scheme.to_string()
        } else {
            format!("{}:8080", endpoint_no_scheme)
        };

        let url = format!("http://{}/api/v1/devices/{}/deploy", endpoint_with_http_port, device_id);
        let client = reqwest::Client::new();
        
        match client
            .post(&url)
            .json(&deployment_request)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    let deployment_response: serde_json::Value = response.json().await
                        .map_err(|e| ControllerError::ApplicationError(format!("Failed to parse deployment response: {}", e)))?;
                    
                    if deployment_response.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        info!("Successfully deployed application {} to device {}", application.name_any(), device_id);
                        Ok(())
                    } else {
                        let error_msg = deployment_response.get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error");
                        Err(ControllerError::ApplicationError(format!("Deployment failed: {}", error_msg)))
                    }
                } else {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                    Err(ControllerError::ApplicationError(format!("Gateway returned error {}: {}", status, error_text)))
                }
            }
            Err(e) => {
                Err(ControllerError::ApplicationError(format!("Failed to call gateway endpoint: {}", e)))
            }
        }
    }

    async fn update_application_status(&self, application: &wasmbed_k8s_resource::Application, status: wasmbed_k8s_resource::ApplicationStatus) -> Result<(), ControllerError> {
        let phase = status.phase; // capture before status is moved into the json! macro
        let patch = serde_json::json!({
            "status": status
        });

        let params = PatchParams::apply("wasmbed-application-controller");
        let patch = Patch::Merge(patch);
        
        // Try patch_status first, fallback to patch if status doesn't exist
        let result = match self.applications.patch_status(&application.name_any(), &params, &patch).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // If patch_status fails, try regular patch
                warn!("patch_status failed, trying regular patch: {}", e);
                self.applications
                    .patch(&application.name_any(), &params, &patch)
                    .await?;
                Ok(())
            }
        };
        // Always sync in-memory phase cache so reconcile() never reverts to Creating.
        {
            let mut phases = self.app_phases.write().await;
            phases.insert(application.name_any(), phase);
        }
        result
    }

    async fn create_application_deployment(&self, application: &wasmbed_k8s_resource::Application) -> Result<(), ControllerError> {
        let deployment_name = format!("{}-deployment", application.name_any());
        
        // Check if deployment already exists
        match self.deployments.get(&deployment_name).await {
            Ok(_) => {
                info!("Deployment {} already exists", deployment_name);
                return Ok(());
            }
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                // Deployment doesn't exist, create it
            }
            Err(e) => return Err(ControllerError::KubeError(e)),
        }

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some(deployment_name.clone()),
                namespace: Some("wasmbed".to_string()),
                labels: Some({
                    let mut labels = std::collections::BTreeMap::new();
                    labels.insert("app".to_string(), "wasmbed-application".to_string());
                    labels.insert("application".to_string(), application.name_any());
                    labels
                }),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::apps::v1::DeploymentSpec {
                replicas: Some(1),
                selector: k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector {
                    match_labels: Some({
                        let mut labels = std::collections::BTreeMap::new();
                        labels.insert("app".to_string(), "wasmbed-application".to_string());
                        labels.insert("application".to_string(), application.name_any());
                        labels
                    }),
                    ..Default::default()
                },
                template: k8s_openapi::api::core::v1::PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some({
                            let mut labels = std::collections::BTreeMap::new();
                            labels.insert("app".to_string(), "wasmbed-application".to_string());
                            labels.insert("application".to_string(), application.name_any());
                            labels
                        }),
                        ..Default::default()
                    }),
                    spec: Some(k8s_openapi::api::core::v1::PodSpec {
                        containers: vec![k8s_openapi::api::core::v1::Container {
                            name: "application".to_string(),
                            image: Some("wasmbed/application:latest".to_string()),
                            env: Some(vec![
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "APPLICATION_NAME".to_string(),
                                    value: Some(application.name_any()),
                                    ..Default::default()
                                },
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "WASM_BYTES".to_string(),
                                    value: Some(application.spec.wasm_bytes.clone()),
                                    ..Default::default()
                                },
                            ]),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        let params = PostParams::default();
        self.deployments.create(&params, &deployment).await?;
        info!("Created deployment for application: {}", application.name_any());
        Ok(())
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
    let controller = ApplicationController::new(client);
    
    info!("Starting Application Controller...");
    controller.run().await?;
    
    Ok(())
}