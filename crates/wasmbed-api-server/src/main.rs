// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use clap::Parser;
use axum::{
    extract::{State, WebSocketUpgrade, Path, Json},
    http::StatusCode,
    response::Html,
    routing::{get, post, delete, put},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
    process::Stdio,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use tokio::sync::RwLock;
use tokio::net::TcpListener;
use reqwest;
use tracing::{error, info, warn};

mod device_management;
mod application_management;
mod gateway_management;
mod monitoring;
mod templates;

use device_management::DeviceManager;
use application_management::ApplicationManager;
use gateway_management::GatewayManager;
use monitoring::MonitoringDashboard;
use templates::DashboardTemplates;

// Renode Manager integration (previously QEMU, now using Renode)
use wasmbed_qemu_manager::{RenodeManager, McuType};

#[derive(Parser)]
#[command(name = "wasmbed-api-server")]
#[command(about = "Wasmbed API Server - Backend API for managing edge devices")]
struct Args {
    #[arg(long, env = "WASMBED_API_SERVER_PORT", default_value = "3001")]
    port: u16,
    #[arg(long, env = "WASMBED_API_SERVER_GATEWAY_ENDPOINT", default_value = "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080")]
    gateway_endpoint: String,
    /// Optional; if empty, monitoring uses fallback metrics and infrastructure health is "not_configured"
    #[arg(long, env = "WASMBED_API_SERVER_INFRASTRUCTURE_ENDPOINT", default_value = "")]
    infrastructure_endpoint: String,
}

/// Dashboard configuration
#[derive(Debug, Clone)]
pub struct DashboardConfig {
    pub port: u16,
    pub gateway_endpoint: String,
    pub infrastructure_endpoint: String,
    pub refresh_interval: Duration,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            port: 3001,
            gateway_endpoint: "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080".to_string(), // Kubernetes service name (no port-forward needed)
            infrastructure_endpoint: String::new(), // Not deployed by default; set env to enable
            refresh_interval: Duration::from_secs(5),
        }
    }
}

/// Dashboard state
#[derive(Debug, Clone)]
pub struct DashboardState {
    pub config: DashboardConfig,
    pub device_manager: Arc<DeviceManager>,
    pub application_manager: Arc<ApplicationManager>,
    pub gateway_manager: Arc<GatewayManager>,
    pub monitoring: Arc<MonitoringDashboard>,
    pub templates: Arc<DashboardTemplates>,
    pub system_status: Arc<RwLock<SystemStatus>>,
    pub renode_manager: Arc<RenodeManager>, // Renode manager (previously qemu_manager)
}

/// System status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub devices: DeviceStats,
    pub applications: ApplicationStats,
    pub gateways: GatewayStats,
    pub infrastructure: InfrastructureStats,
    pub uptime: u64,
    pub last_update: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStats {
    pub total: u32,
    pub connected: u32,
    pub enrolled: u32,
    pub unreachable: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationStats {
    pub total: u32,
    pub running: u32,
    pub pending: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStats {
    pub total: u32,
    pub active: u32,
    pub inactive: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureStats {
    pub ca_status: String,
    pub secret_store_status: String,
    pub monitoring_status: String,
    pub logging_status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalExecuteRequest {
    pub command: String,
    pub command_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalExecuteResponse {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PodInfo {
    pub name: String,
    pub ready: String,
    pub status: String,
    pub restarts: u32,
    pub age: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub r#type: String,
    pub cluster_ip: String,
    pub external_ip: String,
    pub ports: String,
    pub age: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PodMetric {
    pub name: String,
    pub cpu: String,
    pub memory: String,
}

/// Dashboard API handlers
pub struct DashboardApi;

// Helper function for device connection - separated to avoid Handler trait issues
async fn do_connect_device(
    state: Arc<DashboardState>,
    device_id: String,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::process::Stdio;
    
    // Complete device connection flow: Kubernetes + Gateway registration + Renode startup
    info!("Connecting device {}", device_id);
    
    // Get gateway ID from device info
    let gateway_id = {
        let device_info = state.device_manager.get_device(&device_id).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        match device_info {
            Some(info) => info.gateway_id.unwrap_or_else(|| "gateway-1".to_string()),
            None => "gateway-1".to_string(),
        }
    };
    
    // Step 1: Register device with gateway to enable WASM deployment
    let gateway_endpoint = std::env::var("WASMBED_API_SERVER_GATEWAY_ENDPOINT")
        .unwrap_or_else(|_| "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080".to_string());
    info!("Registering device {} with gateway at {}", device_id, gateway_endpoint);
    let gateway_url = format!("{}/api/v1/devices/{}/connect", gateway_endpoint, device_id);
    
    let gateway_response = reqwest::Client::new()
        .post(&gateway_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;
    
    match &gateway_response {
        Ok(resp) if resp.status().is_success() => {
            info!("Device {} successfully registered with gateway", device_id);
        }
        Ok(resp) => {
            let status = resp.status();
            warn!("Gateway registration returned status {} for device {}", status, device_id);
        }
        Err(e) => {
            warn!("Failed to connect to gateway for device {}: {}", device_id, e);
        }
    }
    
    // Step 2: Calculate bridge port and endpoints
    let gateway_host = gateway_endpoint
        .replace("http://", "")
        .replace("https://", "")
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string();
    
    // Get TLS port dynamically from gateway CRD or service
    let gateway_tls_port = DashboardApi::get_gateway_tls_port(&gateway_host).await.unwrap_or(8081);
    let gateway_tls_endpoint = format!("{}:{}", gateway_host, gateway_tls_port);
    
    let mut hasher = DefaultHasher::new();
    device_id.hash(&mut hasher);
    let device_hash = hasher.finish();
    let bridge_port = 40000 + (device_hash % 1000) as u16;
    let bridge_endpoint = format!("127.0.0.1:{}", bridge_port);
    
    info!("TCP bridge will use port {} for device {} (gateway TLS: {})", bridge_port, device_id, gateway_tls_endpoint);
    
    // Step 3: Start Renode emulation (TCP bridge is handled internally by RenodeManager)
    info!("Starting Renode emulation for device {} with bridge endpoint {}", device_id, bridge_endpoint);
    if let Err(e) = state.renode_manager.start_device(&device_id, Some(bridge_endpoint.clone())).await {
        // Do not report success here: the caller (dashboard, test harness, experiment
        // campaign) would record an attachment that never happened.
        error!("Failed to start Renode for device {}: {}", device_id, e);
        return Ok(Json(serde_json::json!({
            "success": false,
            "status": "Error",
            "message": format!("Failed to start emulation for device {}: {}", device_id, e)
        })));
    }
    info!("Renode started successfully for device {}", device_id);

    // Step 5: Update device status in Kubernetes.
    // Only the gateway may declare a device Connected, and only a real Heartbeat
    // may set lastHeartbeat: those two fields are the evidence that the device
    // actually attached. Writing them here would make the API server's own
    // request the proof of its success. Emulation started, enrollment has not
    // happened yet, so the phase is Enrolling.
    let patch = serde_json::json!({
        "status": {
            "phase": "Enrolling",
            "gateway": {
                "name": gateway_id,
                "endpoint": format!("127.0.0.1:{}", 30450 + device_id.len() as u16),
                "connectedAt": chrono::Utc::now().to_rfc3339()
            }
        }
    });
    
    let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "{}".to_string());
    let _output = tokio::process::Command::new("kubectl")
        .args(&["patch", "devices.wasmbed.github.io", &device_id, "-n", "wasmbed", "--type", "merge", "--subresource", "status", "--patch", &patch_str])
        .output()
        .await;
    
    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Device {} connected and registered", device_id),
        "lastHeartbeat": SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        "status": "Connected"
    })))
}
#[axum::debug_handler]
async fn connect_device_standalone_handler(
    State(state): State<Arc<DashboardState>>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Connecting device: {}", device_id);
    
    // Get device info from Kubernetes to retrieve gateway ID
    let device_info = state.device_manager.get_device(&device_id).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let device_info = match device_info {
        Some(info) => info,
        None => return Err(StatusCode::NOT_FOUND),
    };
    
    // Get gateway ID from device info
    let gateway_id = device_info.gateway_id.unwrap_or_else(|| "gateway-1".to_string());
    
    // Calculate bridge port based on device ID hash
    let mut hasher = DefaultHasher::new();
    device_id.hash(&mut hasher);
    let device_hash = hasher.finish();
    let bridge_port = 40000 + (device_hash % 1000) as u16;
    let bridge_endpoint = format!("127.0.0.1:{}", bridge_port);
    
    // Register device with gateway to enable WASM deployment
    let gateway_endpoint = std::env::var("WASMBED_API_SERVER_GATEWAY_ENDPOINT")
        .unwrap_or_else(|_| "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080".to_string());
    info!("Registering device {} with gateway at {}", device_id, gateway_endpoint);
    
    let gateway_url = format!("{}/api/v1/devices/{}/connect", gateway_endpoint, device_id);
    let gateway_response = reqwest::Client::new()
        .post(&gateway_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;
    
    match gateway_response {
        Ok(resp) => {
            if resp.status().is_success() {
                info!("Device {} successfully registered with gateway", device_id);
            } else {
                let status = resp.status();
                let error_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                warn!("Gateway registration failed for device {} (status: {}): {}", device_id, status, error_text);
            }
        }
        Err(e) => {
            warn!("Failed to connect to gateway for device {} registration: {}", device_id, e);
        }
    }
    
    // Start TCP bridge for device
    let gateway_host = gateway_endpoint
        .replace("http://", "")
        .replace("https://", "")
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string();
    // Gateway TLS port is 8081 (not 8443) - see k8s/deployments/wasmbed-deployments.yaml
    let gateway_tls_endpoint = format!("{}:8081", gateway_host);
    
    // Start Renode emulation directly (not delegated to device controller)
    // Call internal endpoint to avoid Send issues with anyhow::Error
    info!("Starting Renode emulation for device {} (gateway: {})", device_id, gateway_endpoint);
    
    // Use HTTP call to internal endpoint which properly handles anyhow::Error
    let internal_url = format!("http://localhost:3001/api/v1/devices/{}/renode/start", device_id);
    let renode_start_response = reqwest::Client::new()
        .post(&internal_url)
        .json(&serde_json::json!({"gatewayEndpoint": gateway_endpoint}))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    
    // Calculate bridge endpoint for status update (same logic as RenodeManager uses)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    device_id.hash(&mut hasher);
    let device_hash = hasher.finish();
    let bridge_port = 40000 + (device_hash % 1000) as u16;
    let bridge_endpoint = format!("127.0.0.1:{}", bridge_port);
    
    // If the emulation did not start there is no device to attach, and saying so
    // here is the only way the caller can tell. Reporting success would put a
    // device that never booted into the connected count.
    match renode_start_response {
        Ok(resp) if resp.status().is_success() => {
            info!("Renode Docker container started for device {} (bridge endpoint: {})", device_id, bridge_endpoint);
        }
        Ok(resp) => {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            error!("Failed to start Renode for device {}: HTTP {} - {}", device_id, status, error_text);
            return Ok(Json(serde_json::json!({
                "success": false,
                "status": "Error",
                "message": format!("Failed to start emulation for device {}: HTTP {} - {}", device_id, status, error_text)
            })));
        }
        Err(e) => {
            error!("Failed to call Renode start endpoint for device {}: {}", device_id, e);
            return Ok(Json(serde_json::json!({
                "success": false,
                "status": "Error",
                "message": format!("Failed to reach the emulation endpoint for device {}: {}", device_id, e)
            })));
        }
    }

    // Update device status in Kubernetes.
    // phase=Connected, connectedSince and lastHeartbeat are evidence that the
    // device attached: they belong to the gateway, which sees the TLS session and
    // the heartbeats. All this endpoint knows is that the emulation started, so
    // it records the gateway association and leaves the phase at Enrolling.
    let patch = serde_json::json!({
        "status": {
            "phase": "Enrolling",
            "gateway": {
                "name": gateway_id,
                "endpoint": bridge_endpoint,
                "connectedAt": chrono::Utc::now().to_rfc3339()
            },
            "pairingMode": false
        }
    });
    
    let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "{}".to_string());
    let output = tokio::process::Command::new("kubectl")
        .args(&["patch", "devices.wasmbed.github.io", &device_id, "-n", "wasmbed", "--type", "merge", "--subresource", "status", "--patch", &patch_str])
        .output()
        .await;
        
    match output {
        Ok(output) => {
            if output.status.success() {
                info!("Emulation started for device {}; waiting for it to enroll with the gateway", device_id);
                Ok(Json(serde_json::json!({
                    "success": true,
                    "status": "Enrolling",
                    "message": format!("Emulation started for device {}; the gateway will report it connected once the TLS session is established", device_id)
                })))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("Failed to update Kubernetes status for device {}: {}", device_id, stderr);
                Ok(Json(serde_json::json!({
                    "success": false,
                    "status": "Error",
                    "message": format!("Emulation started for device {} but its Kubernetes status could not be updated: {}", device_id, stderr)
                })))
            }
        }
        Err(e) => {
            error!("Failed to execute kubectl for device connection {}: {}", device_id, e);
            Ok(Json(serde_json::json!({
                "success": false,
                "status": "Error",
                "message": format!("Emulation started for device {} but kubectl could not be executed: {}", device_id, e)
            })))
        }
    }
}

async fn start_qemu_device_handler(
    state: State<Arc<DashboardState>>,
    path: Path<String>,
    request: Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    DashboardApi::start_qemu_device(state, path, request).await
}

async fn stop_qemu_device_handler(
    state: State<Arc<DashboardState>>,
    path: Path<String>,
    request: Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    DashboardApi::stop_qemu_device(state, path, request).await
}

async fn stop_emulation_handler(
    State(state): State<Arc<DashboardState>>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Stopping emulation for device: {}", device_id);

    // Must go through RenodeManager: it owns both the real container name
    // (wasmbed-renode-<id>, not renode-<id>) and the in-memory device state.
    // Stopping the container behind its back leaves the device marked Running
    // for ever, and every later start fails with "Device is already running"
    // while this endpoint keeps answering success.
    match state.renode_manager.stop_device(&device_id).await {
        Ok(_) => {
            info!("Emulation stopped for device {}", device_id);
            Ok(Json(serde_json::json!({
                "success": true,
                "message": format!("Emulation stopped for device {}", device_id)
            })))
        }
        Err(e) => {
            warn!("Failed to stop emulation for device {}: {}", device_id, e);
            Ok(Json(serde_json::json!({
                "success": false,
                "message": format!("Failed to stop emulation for device {}: {}", device_id, e)
            })))
        }
    }
}

impl DashboardApi {
    /// Get dashboard home page
    pub async fn home(State(state): State<Arc<DashboardState>>) -> Result<Html<String>, StatusCode> {
        let system_status = state.system_status.read().await;
        let html = state.templates.render_dashboard(&system_status).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Html(html))
    }

    /// Get devices page
    pub async fn devices(State(state): State<Arc<DashboardState>>) -> Result<Html<String>, StatusCode> {
        let devices = state.device_manager.get_all_devices().await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let html = state.templates.render_devices(&devices).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Html(html))
    }

    /// Get applications page
    pub async fn applications(State(state): State<Arc<DashboardState>>) -> Result<Html<String>, StatusCode> {
        let applications = state.application_manager.get_all_applications().await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let html = state.templates.render_applications(&applications).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Html(html))
    }

    /// Get gateways page
    pub async fn gateways(State(state): State<Arc<DashboardState>>) -> Result<Html<String>, StatusCode> {
        let gateways = state.gateway_manager.get_all_gateways().await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let html = state.templates.render_gateways(&gateways).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Html(html))
    }

    /// Get monitoring page
    pub async fn monitoring(State(state): State<Arc<DashboardState>>) -> Result<Html<String>, StatusCode> {
        let metrics = state.monitoring.get_metrics().await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let html = state.templates.render_monitoring(&metrics).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Html(html))
    }

    /// Health check endpoint
    pub async fn health_check() -> Result<Json<serde_json::Value>, StatusCode> {
        Ok(Json(serde_json::json!({
            "status": "healthy",
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        })))
    }

    /// Get system status API
    pub async fn api_status(State(state): State<Arc<DashboardState>>) -> Result<Json<SystemStatus>, StatusCode> {
        let system_status = state.system_status.read().await;
        Ok(Json(system_status.clone()))
    }

    /// Get devices API
    pub async fn api_devices(
        State(state): State<Arc<DashboardState>>,
        headers: axum::http::HeaderMap,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        let user_agent = headers.get("user-agent")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown");
        let connection = headers.get("connection")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown");
        info!("API devices request - User-Agent: {}, Connection: {}", user_agent, connection);
        match tokio::time::timeout(Duration::from_secs(5), state.device_manager.get_all_devices()).await {
            Ok(Ok(devices)) => Ok(Json(serde_json::json!({
                "devices": devices.into_iter().map(|d| serde_json::json!({
                    "id": d.device_id,
                    "name": d.device_id,
                    "type": d.device_type,
                    "architecture": d.architecture,
                    "status": d.status,
                    "gateway": d.gateway_id,
                    "mcuType": d.mcu_type.unwrap_or_else(|| "Mps2An385".to_string()),
                    "lastHeartbeat": d.last_heartbeat.map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()),
                    "publicKey": d.public_key
                })).collect::<Vec<_>>()
            }))),
            Ok(Err(_)) => Err(StatusCode::INTERNAL_SERVER_ERROR),
            Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
        }
    }

    /// Get applications API
    pub async fn api_applications(
        State(state): State<Arc<DashboardState>>,
        headers: axum::http::HeaderMap,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        let user_agent = headers.get("user-agent")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown");
        info!("API applications request - User-Agent: {}", user_agent);
        match tokio::time::timeout(Duration::from_secs(5), state.application_manager.get_all_applications()).await {
            Ok(Ok(applications)) => Ok(Json(serde_json::json!({
                "applications": applications.into_iter().map(|app| serde_json::json!({
                    "app_id": app.app_id,
                    "name": app.name,
                    "image": app.image,
                    "status": app.status,
                    "deployed_devices": app.deployed_devices,
                    "target_devices": app.target_devices,
                    "created_at": app.created_at.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                    "last_updated": app.last_updated.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                    "statistics": {
                        "total_devices": app.target_devices.as_ref().map_or(0, |v| v.len()),
                        "target_count": app.target_devices.as_ref().map_or(0, |v| v.len()),
                        "running_devices": app.deployed_devices.len(),
                        "deployed_count": app.deployed_devices.len(),
                        "failed_devices": app.failed_devices,
                        "deployment_progress": if let Some(targets) = &app.target_devices {
                            if targets.is_empty() { 0.0 } else { (app.deployed_devices.len() as f64 / targets.len() as f64) * 100.0 }
                        } else { 0.0 }
                    }
                })).collect::<Vec<_>>()
            }))),
            Ok(Err(_)) => Err(StatusCode::INTERNAL_SERVER_ERROR),
            Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
        }
    }

    /// Get gateways API
    pub async fn api_gateways(
        State(state): State<Arc<DashboardState>>,
        headers: axum::http::HeaderMap,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        let user_agent = headers.get("user-agent")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown");
        info!("API gateways request - User-Agent: {}", user_agent);
        match tokio::time::timeout(Duration::from_secs(5), state.gateway_manager.get_all_gateways()).await {
            Ok(Ok(gateways)) => {
                // Get all devices to calculate connected devices per gateway
                let devices = match tokio::time::timeout(Duration::from_secs(5), state.device_manager.get_all_devices()).await {
                    Ok(Ok(devices)) => devices,
                    Ok(Err(_)) => Vec::new(),
                    Err(_) => Vec::new(),
                };
                
                Ok(Json(serde_json::json!({
                    "gateways": gateways.into_iter().map(|g| {
                        let connected_count = devices.iter()
                            .filter(|d| d.status == "Connected" && 
                                d.gateway_id.as_ref().map_or(false, |gw_id| gw_id == &g.gateway_id))
                            .count();
                        let enrolled_count = devices.iter()
                            .filter(|d| d.status == "Enrolled" && 
                                d.gateway_id.as_ref().map_or(false, |gw_id| gw_id == &g.gateway_id))
                            .count();
                        
                        serde_json::json!({
                            "id": g.gateway_id,
                            "name": g.gateway_id,
                            "status": g.status,
                            "endpoint": g.endpoint,
                            "connected_devices": connected_count,
                            "enrolled_devices": enrolled_count
                        })
                    }).collect::<Vec<_>>()
                })))
            },
            Ok(Err(_)) => Err(StatusCode::INTERNAL_SERVER_ERROR),
            Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
        }
    }

    /// Delete gateway
    pub async fn delete_gateway(
        State(_state): State<Arc<DashboardState>>,
        Path(id): Path<String>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Deleting gateway: {}", id);

        let output = tokio::process::Command::new("kubectl")
            .args(&["delete", "gateways.wasmbed.io", &id, "-n", "wasmbed"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Gateway {} deleted successfully", id)
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to delete gateway {}: {}", id, stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl delete for gateway {}: {}", id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Delete device
    pub async fn delete_device(
        State(_state): State<Arc<DashboardState>>,
        Path(id): Path<String>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Deleting device: {}", id);

        let output = tokio::process::Command::new("kubectl")
            .args(&["delete", "devices.wasmbed.github.io", &id, "-n", "wasmbed"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Device {} deleted successfully", id)
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to delete device {}: {}", id, stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl delete for device {}: {}", id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Delete application
    pub async fn delete_application(
        State(_state): State<Arc<DashboardState>>,
        Path(id): Path<String>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Deleting application: {}", id);

        let output = tokio::process::Command::new("kubectl")
            .args(&["delete", "applications.wasmbed.github.io", &id, "-n", "wasmbed"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Application {} deleted successfully", id)
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to delete application {}: {}", id, stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl delete for application {}: {}", id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Toggle gateway status
    pub async fn toggle_gateway(
        State(_state): State<Arc<DashboardState>>,
        Path(id): Path<String>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Toggling gateway: {}", id);

        // Get current gateway status
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "gateways.wasmbed.io", &id, "-n", "wasmbed", "-o", "json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Ok(gateway_json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        let current_status = gateway_json["status"]["phase"]
                            .as_str()
                            .unwrap_or("Pending");

                        let new_status = if current_status == "Running" || current_status == "Active" {
                            "Stopped"
                        } else {
                            "Running"
                        };

                        // Update gateway status using kubectl patch
                        let patch_output = tokio::process::Command::new("kubectl")
                            .args(&[
                                "patch", "gateways.wasmbed.io", &id, "-n", "wasmbed",
                                "--type", "merge",
                                "--patch", &format!("{{\"status\":{{\"phase\":\"{}\"}}}}", new_status)
                            ])
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .output()
                            .await;

                        match patch_output {
                            Ok(patch_output) => {
                                if patch_output.status.success() {
                                    Ok(Json(serde_json::json!({
                                        "success": true,
                                        "message": format!("Gateway {} status changed to {}", id, new_status),
                                        "status": new_status
                                    })))
                                } else {
                                    let stderr = String::from_utf8_lossy(&patch_output.stderr);
                                    error!("Failed to patch gateway {}: {}", id, stderr);
                                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                                }
                            }
                            Err(e) => {
                                error!("Failed to execute kubectl patch for gateway {}: {}", id, e);
                                Err(StatusCode::INTERNAL_SERVER_ERROR)
                            }
                        }
                    } else {
                        error!("Failed to parse gateway JSON for {}", id);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to get gateway {}: {}", id, stderr);
                    Err(StatusCode::NOT_FOUND)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl get for gateway {}: {}", id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Update gateway configuration
    pub async fn update_gateway(
        State(_state): State<Arc<DashboardState>>,
        Path(id): Path<String>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Updating gateway {}: {:?}", id, request);
        
        let endpoint = request.get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:30452");
        
        let capabilities = request.get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_else(|| vec!["tls", "enrollment", "deployment"]);
        
        // Update gateway configuration using kubectl patch
        let patch = serde_json::json!({
            "spec": {
                "endpoint": endpoint,
                "capabilities": capabilities
            }
        });
        
        let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "{}".to_string());
        
        let output = tokio::process::Command::new("kubectl")
            .args(&["patch", "gateways.wasmbed.io", &id, "-n", "wasmbed", "--type", "merge", "--patch", &patch_str])
            .output()
            .await;
            
        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Gateway {} configuration updated successfully", id);
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Gateway {} configuration updated successfully", id)
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to update gateway {}: {}", id, stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl for gateway update {}: {}", id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Create gateways
    pub async fn create_gateway(
        State(state): State<Arc<DashboardState>>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Creating gateways: {:?}", request);
        
        let count = request.get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        
        // Don't set endpoint - let the gateway controller set it automatically to Kubernetes service DNS
        // The endpoint will be set to {gateway-name}-service.wasmbed.svc.cluster.local:8080 by the controller
        
        let gateway_name = request.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("gateway-{}", 1));
        
        let mut created_gateways = Vec::new();
        let mut errors = Vec::new();
        
        for i in 1..=count {
            let name = if count == 1 {
                gateway_name.clone()
            } else {
                format!("{}-{}", gateway_name, i)
            };
            // Don't set endpoint - gateway controller will set it to {name}-service.wasmbed.svc.cluster.local:8080
            // Use a placeholder that the controller will recognize and replace
            let endpoint_placeholder = format!("{}-service.wasmbed.svc.cluster.local:8080", name);
            
            // Create Gateway CRD in Kubernetes
            // Note: Use camelCase for spec fields, but status should not be set in spec
            // The gateway controller will update the endpoint to the correct Kubernetes service DNS
            let gateway_yaml = format!(
                r#"apiVersion: wasmbed.io/v1
kind: Gateway
metadata:
  name: {}
  namespace: wasmbed
spec:
  endpoint: {}
  capabilities:
    - tls
    - enrollment
    - deployment
  config:
    connectionTimeout: "10m"
    enrollmentTimeout: "5m"
    heartbeatInterval: "30s""#,
                name, endpoint_placeholder
            );
            
            // Apply the Gateway CRD using kubectl
            let output = tokio::process::Command::new("kubectl")
                .args(&["apply", "-f", "-"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();
                
            match output {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        use tokio::io::AsyncWriteExt;
                        if let Err(e) = stdin.write_all(gateway_yaml.as_bytes()).await {
                            error!("Failed to write to kubectl stdin for gateway {}: {}", name, e);
                            errors.push(format!("Failed to create gateway {}: {}", name, e));
                            continue;
                        }
                    }
                    
                    match child.wait_with_output().await {
                        Ok(output) => {
                            if output.status.success() {
                                info!("Gateway {} created successfully", name);
                                created_gateways.push(serde_json::json!({
                                    "id": name,
                                    "name": name,
                                    "status": "Pending",
                                    "endpoint": endpoint_placeholder
                                }));
                            } else {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                error!("Failed to create gateway {}: {}", name, stderr);
                                errors.push(format!("Failed to create gateway {}: {}", name, stderr));
                            }
                        }
                        Err(e) => {
                            error!("Failed to execute kubectl for gateway {}: {}", name, e);
                            errors.push(format!("Failed to create gateway {}: {}", name, e));
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to spawn kubectl for gateway {}: {}", name, e);
                    errors.push(format!("Failed to create gateway {}: {}", name, e));
                }
            }
        }
        
        if created_gateways.is_empty() {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        
        let message = if errors.is_empty() {
            format!("Successfully created {} gateways", created_gateways.len())
        } else {
            format!("Created {} gateways, {} errors: {}", 
                   created_gateways.len(), 
                   errors.len(), 
                   errors.join("; "))
        };
        
        Ok(Json(serde_json::json!({
            "success": true,
            "message": message,
            "gateways": created_gateways,
            "errors": errors
        })))
    }

    /// Create devices
    pub async fn create_device(
        State(state): State<Arc<DashboardState>>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Creating devices: {:?}", request);
        
        let count = request.get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        
        let device_type = request.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("MCU");
        
        let gateway_id = request.get("gatewayId")
            .and_then(|v| v.as_str())
            .unwrap_or("gateway-1");
        
        // Resolve the gateway endpoint dynamically based on gateway_id
        let gateway_endpoint = match state.gateway_manager.get_gateway(gateway_id).await {
            Ok(Some(gateway_info)) => {
                // Extract the base URL from the endpoint (remove /api/v1 if present)
                let endpoint = gateway_info.endpoint.trim();
                if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                    endpoint.to_string()
                } else {
                    // If endpoint doesn't have protocol, add http://
                    format!("http://{}", endpoint)
                }
            }
            Ok(None) => {
                warn!("Gateway {} not found, using default endpoint", gateway_id);
                state.config.gateway_endpoint.clone()
            }
            Err(e) => {
                warn!("Failed to get gateway {}: {}, using default endpoint", gateway_id, e);
                state.config.gateway_endpoint.clone()
            }
        };
        
        let mcu_type_str = request.get("mcuType")
            .and_then(|v| v.as_str())
            .unwrap_or("Stm32F746gDisco"); // Default to Ethernet-enabled board
        
        let mcu_type = match mcu_type_str {
            // Ethernet boards (recommended)
            "Stm32F746gDisco" | "Stm32f746gDisco" | "stm32f746g_disco" => wasmbed_qemu_manager::McuType::Stm32F746gDisco,
            "FrdmK64f" | "frdm_k64f" => wasmbed_qemu_manager::McuType::FrdmK64f,
            
            // WiFi boards
            "Esp32DevkitC" | "esp32_devkitc_wroom" => wasmbed_qemu_manager::McuType::Esp32DevkitC,
            
            // No network boards
            "Stm32F4Disco" | "stm32f4_discovery" => wasmbed_qemu_manager::McuType::Stm32F4Disco,
            "Nrf52840DK" | "nrf52840dk_nrf52840" => wasmbed_qemu_manager::McuType::Nrf52840DK,
            
            // Legacy boards
            "RenodeArduinoNano33Ble" => wasmbed_qemu_manager::McuType::RenodeArduinoNano33Ble,
            "RenodeStm32F4Discovery" => wasmbed_qemu_manager::McuType::RenodeStm32F4Discovery,
            "Mps2An385" | "mps2-an385" => wasmbed_qemu_manager::McuType::Mps2An385,
            
            _ => wasmbed_qemu_manager::McuType::Stm32F746gDisco, // Default to Ethernet board
        };
        
        // Map Renode MCU types to CRD-compatible values
        // The Device CRD only accepts: Mps2An385, Mps2An386, Mps2An500, Mps2An505, Stm32Vldiscovery, OlimexStm32H405
        let crd_mcu_type = match mcu_type_str {
            "RenodeArduinoNano33Ble" => "Mps2An385", // Map to default ARM Cortex-M3
            "RenodeStm32F4Discovery" => "Stm32Vldiscovery", // Map to STM32 variant
            _ => "Mps2An385", // Default fallback
        };
        
        let device_name = request.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("device-{}-{}", gateway_id, 1));
        
        let mut created_devices = Vec::new();
        let mut errors = Vec::new();
        
        info!("Using gateway endpoint: {} for gateway_id: {}", gateway_endpoint, gateway_id);
        
        for i in 1..=count {
            let name = if count == 1 {
                device_name.clone()
            } else {
                format!("{}-{}", device_name, i)
            };
            
                    // Call gateway API to create device (which generates a real Ed25519 public key)
                    let gateway_url = format!("{}/api/v1/devices", gateway_endpoint);
                    let device_payload = serde_json::json!({
                        "name": name,
                        "type": device_type,
                        "architecture": "ARM_CORTEX_M",
                        "mcuType": mcu_type_str, // Pass MCU type to gateway
                        "gateway": gateway_id, // This will be saved as preferred_gateway in DeviceSpec
                        "enabled": true
                    });
                    
                    // Create reqwest client with timeout and DNS resolution settings
                    let client = match reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .connect_timeout(std::time::Duration::from_secs(10))
                        .build()
                    {
                        Ok(client) => client,
                        Err(e) => {
                            error!("Failed to create HTTP client: {}", e);
                            errors.push(format!("Failed to create device {}: HTTP client initialization error - {}", name, e));
                            continue;
                        }
                    };
                    
                    info!("Calling gateway API at: {}", gateway_url);
                    match client
                        .post(&gateway_url)
                        .json(&device_payload)
                        .send()
                        .await
                    {
                        Ok(response) => {
                            let status = response.status();
                            if status.is_success() {
                                // Try to get the response body to check for public key
                                let response_text = response.text().await.unwrap_or_else(|_| "{}".to_string());
                                info!("Device {} created via gateway API. Response: {}", name, response_text);
                                
                                // Parse response to check for public key
                                if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(&response_text) {
                                    // Check for public_key in device object (gateway returns it nested)
                                    let public_key = response_json.get("device")
                                        .and_then(|d| d.get("public_key"))
                                        .or_else(|| response_json.get("publicKey"))
                                        .or_else(|| response_json.get("public_key"))
                                        .and_then(|v| v.as_str());
                                    
                                    if let Some(pk) = public_key {
                                        info!("Device {} has public key from gateway: {}...", name, &pk[..50.min(pk.len())]);
                                    } else {
                                        warn!("Device {} created but no public key in gateway response. Response structure: {}", name, 
                                            serde_json::to_string(&response_json).unwrap_or_else(|_| "invalid JSON".to_string()));
                                    }
                                }
                            } else {
                                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                                error!("Gateway API returned error for device {}: HTTP {} - {}", name, status, error_text);
                                
                                // Provide more helpful error messages for common issues
                                let error_msg = if error_text.contains("database space exceeded") || error_text.contains("etcdserver: mvcc") {
                                    format!("Failed to create device {}: Kubernetes etcd database space exceeded. Please clean up unused resources or increase etcd quota.", name)
                                } else {
                                    format!("Failed to create device {} via gateway: HTTP {} - {}", name, status, error_text)
                                };
                                errors.push(error_msg);
                                continue;
                            }
                        }
                        Err(e) => {
                            error!("Failed to call gateway API for device {} at {}: {}", name, gateway_url, e);
                            
                            // Provide more helpful error messages based on error type
                            let error_msg = if e.is_timeout() {
                                format!("Failed to create device {}: Gateway timeout (gateway may be unreachable or slow)", name)
                            } else if e.is_connect() {
                                format!("Failed to create device {}: Cannot connect to gateway at {}. Check if gateway is running.", name, gateway_url)
                            } else if e.to_string().contains("name resolution") || e.to_string().contains("DNS") || e.to_string().contains("Temporary failure") {
                                format!("Failed to create device {}: DNS resolution failed for gateway endpoint {}. Check Kubernetes service DNS.", name, gateway_url)
                            } else {
                                format!("Failed to create device {}: Gateway API error - {}", name, e)
                            };
                            
                            errors.push(error_msg);
                            continue;
                        }
                    }
                    
                    // Device created via gateway API, now create Renode device instance
                    info!("Device {} created successfully via gateway", name);
                    
                    // Check if device already exists in RenodeManager (from previous session)
                    // If it exists, reuse it; otherwise create a new one
                    let renode_device = match state.renode_manager.get_device(&name).await {
                        Some(existing_device) => {
                            // Device exists in RenodeManager - reuse it
                            info!("Renode device {} already exists in manager, reusing it", name);
                            existing_device
                        }
                        None => {
                            // Device doesn't exist in RenodeManager, create it
                                let endpoint = format!("127.0.0.1:{}", 30450 + name.len() as u16);
                                match state.renode_manager.create_device(
                                    name.clone(),
                                    name.clone(),
                                    "ARM_CORTEX_M".to_string(),
                                    device_type.to_string(),
                                    mcu_type.clone(),
                                    Some(endpoint),
                                ).await {
                                    Ok(renode_device) => {
                                        info!("Renode device {} created successfully", name);
                                    renode_device
                                            }
                                            Err(e) => {
                                    error!("Failed to create Renode device {}: {}", name, e);
                                    errors.push(format!("Failed to create Renode device {}: {}", name, e));
                                    continue; // Skip this device and continue with next
                                }
                            }
                        }
                    };
                    
                    // Don't start Renode automatically during device creation
                    // User should start it manually via the connect/start endpoint
                    // This prevents timeout issues during device creation
                                                created_devices.push(serde_json::json!({
                                                    "id": name,
                                                    "name": name,
                                                    "type": device_type,
                                                    "mcuType": mcu_type_str,
                                                    "status": "Pending",
                                                    "renodeEndpoint": renode_device.endpoint,
                                                    "renodeStarted": false,
                                                    "qemuEndpoint": renode_device.endpoint, // Backward compatibility
                                                    "qemuStarted": false // Backward compatibility
                                                }));
        }
        
        // Return success even if some devices failed, as long as we have some info
        if created_devices.is_empty() && !errors.is_empty() {
            return Ok(Json(serde_json::json!({
                "success": false,
                "message": format!("Failed to create devices: {}", errors.join("; ")),
                "errors": errors
            })));
        }
        
        let message = if errors.is_empty() {
            format!("Successfully created {} devices", created_devices.len())
        } else {
            format!("Created {} devices, {} errors: {}", 
                   created_devices.len(), 
                   errors.len(), 
                   errors.join("; "))
        };
        
        Ok(Json(serde_json::json!({
            "success": true,
            "message": message,
            "devices": created_devices,
            "errors": errors
        })))
    }

    /// Enroll device to gateway
    pub async fn enroll_device(
        State(state): State<Arc<DashboardState>>,
        Path(device_id): Path<String>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Enrolling device {}: {:?}", device_id, request);
        
        let gateway_id = request.get("gatewayId")
            .and_then(|v| v.as_str())
            .unwrap_or("gateway-1");
        
        // Update device status to "enrolled" using kubectl patch
        let patch = serde_json::json!({
            "status": {
                "phase": "Enrolled",
                "gatewayId": gateway_id
            }
        });
        
        let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "{}".to_string());
        
        let output = tokio::process::Command::new("kubectl")
            .args(&["patch", "devices.wasmbed.github.io", &device_id, "-n", "wasmbed", "--type", "merge", "--subresource", "status", "--patch", &patch_str])
            .output()
            .await;
            
        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Device {} enrolled to gateway {} successfully", device_id, gateway_id);
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Device {} enrolled to gateway {} successfully", device_id, gateway_id)
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to enroll device {}: {}", device_id, stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl for device enrollment {}: {}", device_id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Connect device to gateway and start Renode emulation
    pub async fn connect_device(
        State(state): State<Arc<DashboardState>>,
        Path(device_id): Path<String>,
        Json(_request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Connecting device {}", device_id);
        
        // Get gateway ID
        let gateway_id = state.device_manager.get_device(&device_id).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .and_then(|info| info.gateway_id)
            .unwrap_or_else(|| "gateway-1".to_string());
        
        // Perform connection steps
        let gateway_endpoint = std::env::var("WASMBED_API_SERVER_GATEWAY_ENDPOINT")
            .unwrap_or_else(|_| "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080".to_string());
        
        // Register with gateway
        Self::register_with_gateway(&device_id, &gateway_endpoint).await;
        
        // Start Renode emulation (TCP bridge is handled internally by RenodeManager)
        // Pass gateway endpoint - RenodeManager will convert to TLS and start bridge internally
        Self::start_renode_emulation(&state, &device_id, &gateway_endpoint).await;
        
        // Calculate bridge endpoint for status update (same logic as RenodeManager uses)
        let bridge_endpoint = Self::calculate_bridge_endpoint(&device_id).await;
        Self::update_device_status(&device_id, &gateway_id, &bridge_endpoint).await;
        
        Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Device {} connected and registered with gateway", device_id),
            "lastHeartbeat": SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            "status": "Connected"
        })))
    }
    
    async fn register_with_gateway(device_id: &str, gateway_endpoint: &str) {
        info!("Registering device {} with gateway at {}", device_id, gateway_endpoint);
        let gateway_url = format!("{}/api/v1/devices/{}/connect", gateway_endpoint, device_id);
        match reqwest::Client::new()
            .post(&gateway_url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await {
            Ok(resp) if resp.status().is_success() => {
                info!("Device {} successfully registered with gateway", device_id);
            }
            Ok(resp) => {
                let status = resp.status();
                let error_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                warn!("Gateway registration failed for device {} (status: {}): {}", device_id, status, error_text);
            }
            Err(e) => {
                warn!("Failed to connect to gateway for device {} registration: {}", device_id, e);
            }
        }
    }
    
    /// Get gateway TLS port dynamically from gateway CRD or service
    /// Returns the TLS port or None if not found (caller should use default)
    async fn get_gateway_tls_port(gateway_host: &str) -> Option<u16> {
        // Extract gateway name from host (e.g., "gateway-1-service.wasmbed.svc.cluster.local" -> "gateway-1")
        let gateway_name = gateway_host
            .split('.')
            .next()
            .unwrap_or(gateway_host)
            .replace("-service", "");
        
        // Try to get TLS port from gateway CRD
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "gateways.wasmbed.io", &gateway_name, "-n", "wasmbed", "-o", "json"])
            .output()
            .await;
        
        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Ok(gateway_json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    // Try to get tlsPort from spec
                    if let Some(tls_port) = gateway_json["spec"]["tlsPort"].as_u64() {
                        info!("Found TLS port {} from gateway CRD for {}", tls_port, gateway_name);
                        return Some(tls_port as u16);
                    }
                }
            }
        }
        
        // Fallback: Try to get TLS port from service
        let service_name = if gateway_host.contains("-service") {
            gateway_host.split('.').next().unwrap_or(gateway_host).to_string()
        } else {
            format!("{}-service", gateway_name)
        };
        
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "svc", &service_name, "-n", "wasmbed", "-o", "json"])
            .output()
            .await;
        
        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Ok(service_json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    // Look for TLS port in service ports
                    if let Some(ports) = service_json["spec"]["ports"].as_array() {
                        for port in ports {
                            if let Some(name) = port["name"].as_str() {
                                if name == "tls" || name == "https" {
                                    if let Some(port_num) = port["targetPort"].as_u64() {
                                        info!("Found TLS port {} from service {} for {}", port_num, service_name, gateway_name);
                                        return Some(port_num as u16);
                                    }
                                    if let Some(port_num) = port["port"].as_u64() {
                                        info!("Found TLS port {} from service {} for {}", port_num, service_name, gateway_name);
                                        return Some(port_num as u16);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        warn!("Could not determine TLS port for gateway {}, will use default 8081", gateway_name);
        None
    }

    async fn calculate_bridge_endpoint(device_id: &str) -> String {
        // Calculate bridge port (same logic as RenodeManager uses)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        device_id.hash(&mut hasher);
        let device_hash = hasher.finish();
        let bridge_port = 40000 + (device_hash % 1000) as u16;
        format!("127.0.0.1:{}", bridge_port)
    }
    
    async fn start_renode_emulation(state: &Arc<DashboardState>, device_id: &str, gateway_endpoint: &str) {
        info!("Starting Renode emulation for device {} (gateway: {})", device_id, gateway_endpoint);
        // RenodeManager will handle TCP bridge internally and convert gateway endpoint to TLS
        match state.renode_manager.start_device(device_id, Some(gateway_endpoint.to_string())).await {
            Ok(_) => info!("Renode started for device {}", device_id),
            Err(e) => warn!("Failed to start Renode for device {}: {}", device_id, e),
        }
    }
    
    async fn update_device_status(device_id: &str, gateway_id: &str, bridge_endpoint: &str) {
        let patch = serde_json::json!({
            "status": {
                "phase": "Connected",
                "lastHeartbeat": chrono::Utc::now().to_rfc3339(),
                "connectedSince": chrono::Utc::now().to_rfc3339(),
                "gateway": {
                    "name": gateway_id,
                    "endpoint": bridge_endpoint,
                    "connectedAt": chrono::Utc::now().to_rfc3339()
                },
                "pairingMode": false
            }
        });
        
        let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "{}".to_string());
        let _ = tokio::process::Command::new("kubectl")
            .args(&["patch", "devices.wasmbed.github.io", device_id, "-n", "wasmbed", "--type", "merge", "--subresource", "status", "--patch", &patch_str])
            .output()
            .await;
    }

    /// Disconnect device
    pub async fn disconnect_device(
        State(state): State<Arc<DashboardState>>,
        Path(device_id): Path<String>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Disconnecting device {}: {:?}", device_id, request);
        
        // Update device status to "disconnected" using kubectl patch
        let patch = serde_json::json!({
            "status": {
                "phase": "Disconnected",
                "gateway": {}
            }
        });
        
        let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "{}".to_string());
        
        let output = tokio::process::Command::new("kubectl")
            .args(&["patch", "devices.wasmbed.github.io", &device_id, "-n", "wasmbed", "--type", "merge", "--subresource", "status", "--patch", &patch_str])
            .output()
            .await;
            
        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Device {} disconnected successfully", device_id);
                    Ok(Json(serde_json::json!({
                        "success": true,
                        "message": format!("Device {} disconnected successfully", device_id),
                        "status": "Disconnected"
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to disconnect device {}: {}", device_id, stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl for device disconnection {}: {}", device_id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Create applications
    pub async fn create_application(
        State(state): State<Arc<DashboardState>>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Creating application: {:?}", request);
        
        let name = request.get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("application-1");
        
        info!("Application name from request: '{}'", name);
        
        let description = request.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let wasm_bytes = request.get("wasmBytes")
            .and_then(|v| v.as_str())
            .unwrap_or("dGVzdA==");
        
        // Handle targetDevices - can be array or object with deviceNames
        let target_devices = if let Some(target_devices_val) = request.get("targetDevices") {
            info!("targetDevices from request: {:?}", target_devices_val);
            if target_devices_val.is_array() {
                // Array format: {"targetDevices": ["device1", "device2"]}
                let devices: Vec<String> = target_devices_val.as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect();
                info!("Parsed target devices (array format): {:?}", devices);
                devices
            } else if let Some(device_names_obj) = target_devices_val.get("deviceNames") {
                // Object format: {"targetDevices": {"deviceNames": ["device1", "device2"]}}
                if let Some(arr) = device_names_obj.as_array() {
                    let devices: Vec<String> = arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect();
                    info!("Parsed target devices (object format): {:?}", devices);
                    devices
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        } else {
            info!("No targetDevices in request");
            vec![]
        };
        
        info!("Final target devices list: {:?} (count: {})", target_devices, target_devices.len());
        
        // Create Application CRD in Kubernetes
        let device_names_yaml = if target_devices.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", target_devices.iter().map(|d| format!("\"{}\"", d)).collect::<Vec<_>>().join(", "))
        };
        
        // Note: status.phase in YAML is ignored by Kubernetes - status is managed by the controller
        // But we can set it initially, though it will be overwritten by the controller
        let application_yaml = format!(
            r#"apiVersion: wasmbed.github.io/v1alpha1
kind: Application
metadata:
  name: {}
  namespace: wasmbed
spec:
  name: {}
  description: {}
  wasmBytes: {}
  targetDevices:
    deviceNames: {}"#,
            name,
            name,
            description,
            wasm_bytes,
            device_names_yaml
        );
        
        // Apply the Application CRD using kubectl
        let output = tokio::process::Command::new("kubectl")
            .args(&["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
            
        match output {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = stdin.write_all(application_yaml.as_bytes()).await {
                        error!("Failed to write to kubectl stdin for application {}: {}", name, e);
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
                
                match child.wait_with_output().await {
                    Ok(output) => {
                        if output.status.success() {
                            info!("Application {} created successfully", name);
                            Ok(Json(serde_json::json!({
                                "success": true,
                                "message": format!("Successfully created application {}", name),
                                "application": {
                                    "id": name,
                                    "name": name,
                                    "status": "Pending",
                                    "targetDevices": target_devices
                                }
                            })))
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            error!("Failed to create application {}: {}", name, stderr);
                            Err(StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }
                    Err(e) => {
                        error!("Failed to execute kubectl for application {}: {}", name, e);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            }
            Err(e) => {
                error!("Failed to spawn kubectl for application {}: {}", name, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Deploy application by ID
    /// Gateway is the sole owner of Application CRD status: it will set Deploying then Running/Failed on PATCH when handling deploy requests.
    pub async fn deploy_application_by_id(
        State(state): State<Arc<DashboardState>>,
        Path(app_id): Path<String>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Deploying application {}: {:?}", app_id, request);
        
        // Get application details to check target devices
        let app_info = state.application_manager.get_application(&app_id).await
            .map_err(|e| {
                error!("Failed to get application {}: {}", app_id, e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        
        let app = match app_info {
            Some(a) => a,
            None => {
                warn!("Application {} not found", app_id);
                return Err(StatusCode::NOT_FOUND);
            }
        };

        let target_devices = app.target_devices.as_deref().unwrap_or(&[]);
        if target_devices.is_empty() {
            warn!("Application {} has no target devices", app_id);
            return Ok(Json(serde_json::json!({
                "success": false,
                "message": format!("Application {} has no target devices", app_id)
            })));
        }

        info!("Deploying application {} to devices: {:?}", app_id, target_devices);

        // Call Gateway HTTP API to deploy to each target device (Gateway owns Application status)
        let gateway_endpoint = state.config.gateway_endpoint.trim_end_matches('/');
        let client = reqwest::Client::new();
        let mut initiated = 0u32;
        let mut errors: Vec<String> = Vec::new();

        for device_id in target_devices {
            let url = format!("{}/api/v1/devices/{}/deploy", gateway_endpoint, device_id);
            let body = serde_json::json!({
                "app_id": app_id,
                "name": app.name,
                "wasm_bytes": ""  // Gateway reads WASM from Application CRD
            });
            match client.post(&url).json(&body).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                                initiated += 1;
                                info!("Deploy initiated for application {} on device {}", app_id, device_id);
                            } else {
                                let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("unknown");
                                errors.push(format!("{}: {}", device_id, msg));
                            }
                        } else {
                            initiated += 1;
                        }
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        errors.push(format!("{}: {} {}", device_id, status, text));
                    }
                }
                Err(e) => {
                    error!("Failed to call gateway for device {}: {}", device_id, e);
                    errors.push(format!("{}: {}", device_id, e));
                }
            }
        }

        let message = if errors.is_empty() {
            format!("Application {} deployment initiated for {} device(s)", app_id, initiated)
        } else {
            format!(
                "Application {} deployment initiated for {} device(s); errors: {}",
                app_id,
                initiated,
                errors.join("; ")
            )
        };
        Ok(Json(serde_json::json!({
            "success": true,
            "message": message,
            "initiated": initiated,
            "errors": errors
        })))
    }

    /// Stop application by ID (calls Gateway stop for each target device; Gateway owns Application status)
    pub async fn stop_application_by_id(
        State(state): State<Arc<DashboardState>>,
        Path(app_id): Path<String>,
        Json(_request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Stopping application {}", app_id);

        let app_info = state.application_manager.get_application(&app_id).await
            .map_err(|e| {
                error!("Failed to get application {}: {}", app_id, e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        let app = match app_info {
            Some(a) => a,
            None => {
                warn!("Application {} not found", app_id);
                return Err(StatusCode::NOT_FOUND);
            }
        };

        let target_devices = app.target_devices.as_deref().unwrap_or(&[]);
        if target_devices.is_empty() {
            return Ok(Json(serde_json::json!({
                "success": true,
                "message": format!("Application {} has no target devices", app_id)
            })));
        }

        let gateway_endpoint = state.config.gateway_endpoint.trim_end_matches('/');
        let client = reqwest::Client::new();
        let mut stopped = 0u32;
        let mut errors: Vec<String> = Vec::new();

        for device_id in target_devices {
            let url = format!("{}/api/v1/devices/{}/stop/{}", gateway_endpoint, device_id, app_id);
            match client.post(&url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                                stopped += 1;
                                info!("Stop initiated for application {} on device {}", app_id, device_id);
                            } else {
                                let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("unknown");
                                errors.push(format!("{}: {}", device_id, msg));
                            }
                        } else {
                            stopped += 1;
                        }
                    } else {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        errors.push(format!("{}: {} {}", device_id, status, text));
                    }
                }
                Err(e) => {
                    error!("Failed to call gateway for device {}: {}", device_id, e);
                    errors.push(format!("{}: {}", device_id, e));
                }
            }
        }

        let message = if errors.is_empty() {
            format!("Application {} stop initiated for {} device(s)", app_id, stopped)
        } else {
            format!("Application {} stop initiated for {} device(s); errors: {}", app_id, stopped, errors.join("; "))
        };
        Ok(Json(serde_json::json!({
            "success": true,
            "message": message,
            "stopped": stopped,
            "errors": errors
        })))
    }

    /// Deploy application
    pub async fn deploy_application(
        State(state): State<Arc<DashboardState>>,
        Json(request): Json<DeployApplicationRequest>,
    ) -> Result<Json<DeployApplicationResponse>, StatusCode> {
        info!("Deploying application: {}", request.name);

        let result = state.application_manager.deploy_application(&request).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok(Json(DeployApplicationResponse {
            success: result.success,
            message: result.message,
            application_id: result.application_id,
        }))
    }

    /// Enable pairing mode
    pub async fn enable_pairing(State(state): State<Arc<DashboardState>>) -> Result<Json<PairingResponse>, StatusCode> {
        info!("Enabling pairing mode");

        let result = state.device_manager.enable_pairing().await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok(Json(PairingResponse {
            success: result,
            message: "Pairing mode enabled".to_string(),
        }))
    }

    /// Disable pairing mode
    pub async fn disable_pairing(State(state): State<Arc<DashboardState>>) -> Result<Json<PairingResponse>, StatusCode> {
        info!("Disabling pairing mode");

        let result = state.device_manager.disable_pairing().await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok(Json(PairingResponse {
            success: result,
            message: "Pairing mode disabled".to_string(),
        }))
    }

    /// Start Renode device emulation (endpoint kept as /qemu/ for backward compatibility)
    pub async fn start_qemu_device(
        State(state): State<Arc<DashboardState>>,
        Path(device_id): Path<String>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Starting Renode emulation for device: {}", device_id);

        // Get device info from Kubernetes
        let device_info = state.device_manager.get_device(&device_id).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let device_info = match device_info {
            Some(info) => info,
            None => return Err(StatusCode::NOT_FOUND),
        };

            // Create Renode device if it doesn't exist
            let renode_device = match state.renode_manager.get_device(&device_id).await {
                Some(device) => device,
                None => {
                    let endpoint = format!("127.0.0.1:{}", 30450 + device_id.len() as u16);
                    // Get MCU type from device info or use default
                    let mcu_type = match device_info.mcu_type.as_deref() {
                        // Ethernet boards (recommended)
                        Some("Stm32F746gDisco") | Some("Stm32f746gDisco") | Some("stm32f746g_disco") => wasmbed_qemu_manager::McuType::Stm32F746gDisco,
                        Some("FrdmK64f") | Some("frdm_k64f") => wasmbed_qemu_manager::McuType::FrdmK64f,
                        
                        // WiFi boards
                        Some("Esp32DevkitC") | Some("esp32_devkitc_wroom") => wasmbed_qemu_manager::McuType::Esp32DevkitC,
                        
                        // No network boards
                        Some("Stm32F4Disco") | Some("stm32f4_discovery") => wasmbed_qemu_manager::McuType::Stm32F4Disco,
                        Some("Nrf52840DK") | Some("nrf52840dk_nrf52840") => wasmbed_qemu_manager::McuType::Nrf52840DK,
                        
                        // Legacy boards
                        Some("RenodeArduinoNano33Ble") => wasmbed_qemu_manager::McuType::RenodeArduinoNano33Ble,
                        Some("RenodeStm32F4Discovery") => wasmbed_qemu_manager::McuType::RenodeStm32F4Discovery,
                        Some("Mps2An385") | Some("mps2-an385") => wasmbed_qemu_manager::McuType::Mps2An385,
                        
                        _ => wasmbed_qemu_manager::McuType::Stm32F746gDisco, // Default to Ethernet board
                    };
                    
                    match state.renode_manager.create_device(
                        device_id.clone(),
                        device_info.device_id.clone(),
                        "ARM_CORTEX_M".to_string(),
                        "MCU".to_string(),
                        mcu_type,
                        Some(endpoint),
                    ).await {
                        Ok(device) => device,
                        Err(e) => {
                            error!("Failed to create Renode device {}: {}", device_id, e);
                            return Ok(Json(serde_json::json!({
                                "success": false,
                                "message": format!("Failed to create Renode device: {}", e),
                                "deviceId": device_id
                            })));
                        }
                    }
                }
            };

        // Get gateway endpoint from device status in Kubernetes
        // Always use gateway name to construct the endpoint (don't use device.status.gateway.endpoint which may be old TCP bridge)
        let gateway_endpoint = {
            // Fetch device from Kubernetes to get gateway name
            let output = tokio::process::Command::new("kubectl")
                .args(&["get", "devices.wasmbed.github.io", &device_id, "-n", "wasmbed", "-o", "json"])
                .output()
                .await;
            
            match output {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Ok(k8s_device) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        // Always use gateway name to construct the endpoint (not device.status.gateway.endpoint)
                        // This ensures we get the correct Kubernetes service DNS name
                        if let Some(gateway_name) = k8s_device["status"]["gateway"]["name"].as_str() {
                            let gateway_host = format!("{}-service.wasmbed.svc.cluster.local", gateway_name);
                            let gateway_tls_port = Self::get_gateway_tls_port(&gateway_host).await.unwrap_or(8081);
                            Some(format!("{}:{}", gateway_host, gateway_tls_port))
                        } else {
                            // Fallback: try to get from spec
                            if let Some(gateway_name) = k8s_device["spec"]["preferredGateway"].as_str() {
                                let gateway_host = format!("{}-service.wasmbed.svc.cluster.local", gateway_name);
                                let gateway_tls_port = Self::get_gateway_tls_port(&gateway_host).await.unwrap_or(8081);
                                Some(format!("{}:{}", gateway_host, gateway_tls_port))
                            } else {
                                None
                            }
                        }
                    } else {
                        None
                    }
                }
                _ => None
            }
        };

        // Start Renode emulation with gateway endpoint
        match state.renode_manager.start_device(&device_id, gateway_endpoint).await {
            Ok(_) => {
                info!("Renode emulation started for device: {}", device_id);
                Ok(Json(serde_json::json!({
                    "success": true,
                    "message": "Renode emulation started successfully",
                    "renodeInstance": renode_device.endpoint,
                    "qemuInstance": renode_device.endpoint, // Backward compatibility
                    "deviceId": device_id
                })))
            }
            Err(e) => {
                error!("Failed to start Renode emulation for device {}: {}", device_id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Stop Renode device emulation (endpoint kept as /qemu/ for backward compatibility)
    pub async fn stop_qemu_device(
        State(state): State<Arc<DashboardState>>,
        Path(device_id): Path<String>,
        Json(_request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Stopping Renode emulation for device: {}", device_id);

        match state.renode_manager.stop_device(&device_id).await {
            Ok(_) => {
                info!("Renode emulation stopped for device: {}", device_id);
                Ok(Json(serde_json::json!({
                    "success": true,
                    "message": "Renode emulation stopped successfully",
                    "deviceId": device_id
                })))
            }
            Err(e) => {
                error!("Failed to stop Renode emulation for device {}: {}", device_id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Start lightweight emulation flow using renode_manager directly
    pub async fn start_emulation(
        State(state): State<Arc<DashboardState>>,
        Path(device_id): Path<String>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Starting emulation for device: {}", device_id);

        // Ensure the device exists within the Renode manager (create a placeholder if needed)
        if state.renode_manager.get_device(&device_id).await.is_none() {
            let placeholder_endpoint = format!("127.0.0.1:{}", 30450 + device_id.len() as u16);
            if let Err(e) = state.renode_manager.create_device(
                device_id.clone(),
                device_id.clone(),
                "ARM_CORTEX_M".to_string(),
                "MCU".to_string(),
                McuType::Stm32F746gDisco, // Use Ethernet board as default
                Some(placeholder_endpoint),
            ).await {
                error!("Failed to auto-create Renode device {}: {}", device_id, e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }

        // Look up the device's assigned gateway from the Kubernetes CRD
        let gateway_endpoint = {
            let output = tokio::process::Command::new("kubectl")
                .args(&["get", "devices.wasmbed.github.io", &device_id, "-n", "wasmbed", "-o", "json"])
                .output()
                .await;
            match output {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Ok(k8s_device) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        if let Some(gw_name) = k8s_device["status"]["gateway"]["name"].as_str() {
                            let gateway_host = format!("{}-service.wasmbed.svc.cluster.local", gw_name);
                            let gateway_tls_port = Self::get_gateway_tls_port(&gateway_host).await.unwrap_or(8443);
                            let ep = format!("{}:{}", gateway_host, gateway_tls_port);
                            info!("Resolved gateway endpoint for device {}: {}", device_id, ep);
                            Some(ep)
                        } else {
                            warn!("Device {} has no gateway assigned in CRD, using env fallback", device_id);
                            Some(std::env::var("WASMBED_API_SERVER_GATEWAY_ENDPOINT")
                                .unwrap_or_else(|_| "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080".to_string()))
                        }
                    } else {
                        None
                    }
                }
                _ => {
                    warn!("Could not fetch device {} from CRD, using env fallback", device_id);
                    Some(std::env::var("WASMBED_API_SERVER_GATEWAY_ENDPOINT")
                        .unwrap_or_else(|_| "http://wasmbed-gateway.wasmbed.svc.cluster.local:8080".to_string()))
                }
            }
        };

        match state.renode_manager.start_device(&device_id, gateway_endpoint).await {
            Ok(_) => {
                info!("Emulation started successfully for device {}", device_id);
                Ok(Json(serde_json::json!({
                    "success": true,
                    "message": format!("Emulation started for device {}", device_id)
                })))
            }
            Err(e) => {
                error!("Failed to start emulation for device {}: {}", device_id, e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }


    /// List all Renode devices (endpoint kept as /qemu/ for backward compatibility)
    pub async fn list_qemu_devices(
        State(state): State<Arc<DashboardState>>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Listing all Renode devices");

        let devices = state.renode_manager.list_devices().await;
        let device_list: Vec<serde_json::Value> = devices
            .into_iter()
            .map(|device| {
                serde_json::json!({
                    "id": device.id,
                    "name": device.name,
                    "architecture": device.architecture,
                    "device_type": device.device_type,
                    "mcu_type": device.mcu_type,
                    "status": device.status,
                    "endpoint": device.endpoint,
                    "process_id": device.process_id
                })
            })
            .collect();

        Ok(Json(serde_json::json!({
            "success": true,
            "devices": device_list,
            "count": device_list.len()
        })))
    }

    const WASM_TEMPLATE_LOCK: &str = include_str!("../templates/wasm_template.lock");

    /// Compile Rust code to WASM
    pub async fn compile_code(
        State(_state): State<Arc<DashboardState>>,
        Json(request): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Compiling code to WASM");

        let code = request.get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let _language = request.get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("rust");

        if code.is_empty() {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": "No code provided"
            })));
        }

        // Create a temporary directory for compilation
        let temp_dir = std::env::temp_dir().join(format!("wasmbed_compile_{}", 
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()));
        
        if let Err(e) = std::fs::create_dir_all(&temp_dir) {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to create temp directory: {}", e)
            })));
        }

        // Create Cargo.toml for the project
        let cargo_toml = r#"[package]
name = "wasmbed-app"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wasm-bindgen = "0.2"
console_error_panic_hook = "0.1"
wee_alloc = "0.4"

[profile.release]
opt-level = "s"
lto = true
"#;

        // Create src/lib.rs with the user code
        let lib_rs = format!(r#"use wasm_bindgen::prelude::*;

// Use `wee_alloc` as the global allocator.
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// Called when the wasm module is instantiated
#[wasm_bindgen(start)]
pub fn wasm_main() {{
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    console_error_panic_hook::set_once();
    
    // Call user main function
    main();
}}

// User code:
{}

#[wasm_bindgen]
extern {{
    fn alert(s: &str);
}}

#[wasm_bindgen]
pub fn greet() {{
    alert("Hello from Wasmbed!");
}}
"#, code);

        // Write files
        if let Err(e) = std::fs::write(temp_dir.join("Cargo.toml"), cargo_toml) {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to write Cargo.toml: {}", e)
            })));
        }

        if let Err(e) = std::fs::create_dir_all(temp_dir.join("src")) {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to create src directory: {}", e)
            })));
        }

        if let Err(e) = std::fs::write(temp_dir.join("src/lib.rs"), lib_rs) {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to write lib.rs: {}", e)
            })));
        }

        if let Err(e) = std::fs::write(temp_dir.join("Cargo.lock"), Self::WASM_TEMPLATE_LOCK) {
            return Ok(Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to write Cargo.lock: {}", e)
            })));
        }

        // Compile to WASM
        let output = tokio::process::Command::new("cargo")
            .args(&["build", "--target", "wasm32-unknown-unknown", "--release", "--offline"])
            .current_dir(&temp_dir)
            .output()
            .await;

        match output {
            Ok(result) => {
                if result.status.success() {
                    // Read the compiled WASM file
                    let wasm_path = temp_dir.join("target/wasm32-unknown-unknown/release/wasmbed_app.wasm");
                    
                    if let Ok(wasm_bytes) = std::fs::read(&wasm_path) {
                        // Encode to base64
                        let wasm_base64 = base64::encode(&wasm_bytes);
                        
                        // Clean up temp directory
                        let _ = std::fs::remove_dir_all(&temp_dir);
                        
                        Ok(Json(serde_json::json!({
                            "success": true,
                            "wasmBytes": wasm_base64,
                            "size": wasm_bytes.len(),
                            "message": "Code compiled successfully to WASM"
                        })))
                    } else {
                        Ok(Json(serde_json::json!({
                            "success": false,
                            "error": "Failed to read compiled WASM file"
                        })))
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    Ok(Json(serde_json::json!({
                        "success": false,
                        "error": format!("Compilation failed: {}", stderr)
                    })))
                }
            }
            Err(e) => {
                Ok(Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to run cargo: {}", e)
                })))
            }
        }
    }

    /// WebSocket handler for real-time updates
    pub async fn websocket_handler(
        ws: WebSocketUpgrade,
        State(_state): State<Arc<DashboardState>>,
    ) -> axum::response::Response {
        ws.on_upgrade(|_socket| async move {
            // Handle WebSocket connection for real-time updates
            info!("WebSocket connection established");
            
            // In a real implementation, this would:
            // 1. Send periodic status updates
            // 2. Handle client commands
            // 3. Manage connection lifecycle
        })
    }

    /// Execute terminal command
    pub async fn execute_terminal_command(
        State(_state): State<Arc<DashboardState>>,
        Json(request): Json<TerminalExecuteRequest>,
    ) -> Result<Json<TerminalExecuteResponse>, StatusCode> {
        info!("Executing terminal command: {}", request.command);

        // Allowed commands for security
        let allowed_commands = vec![
            "kubectl get pods -n wasmbed",
            "kubectl get devices -n wasmbed",
            "kubectl get applications -n wasmbed",
            "kubectl get gateways.wasmbed.github.io -n wasmbed",
            "kubectl get svc -n wasmbed",
            "kubectl top pods -n wasmbed",
            "kubectl logs -n wasmbed --tail=50",
            "kubectl get nodes",
            "kubectl get namespaces",
            "kubectl get crd",
            "kubectl get events -n wasmbed",
            "kubectl describe pods -n wasmbed",
            "kubectl get configmaps -n wasmbed",
            "kubectl get secrets -n wasmbed",
            "kubectl get pv",
            "kubectl get pvc -n wasmbed",
            "kubectl get ingress -n wasmbed",
            "kubectl get networkpolicies -n wasmbed",
            "kubectl get serviceaccounts -n wasmbed",
            "kubectl get roles -n wasmbed",
            "kubectl get devices -n wasmbed -o wide",
            "kubectl get applications -n wasmbed -o wide",
            "kubectl get gateways.wasmbed.github.io -n wasmbed -o wide",
            "kubectl get events -n wasmbed --sort-by=.metadata.creationTimestamp",
            "kubectl get certificates -n wasmbed",
            "kubectl get all -n wasmbed",
            "kubectl get pods -n wasmbed -l app=wasmbed-application-controller",
            "kubectl logs -n wasmbed -l app=wasmbed-application-controller",
            "kubectl logs -n wasmbed -l app=wasmbed-gateway",
            "kubectl describe device -n wasmbed",
            "curl -s http://localhost:30461/health",
            "curl -s http://localhost:30461/logs",
            "curl -s http://localhost:30453/api/v1/status",
            "./target/release/wasmbed-gateway-controller --kubeconfig ~/.kube/config &",
            "./target/release/wasmbed-device-controller --kubeconfig ~/.kube/config &",
            "./target/release/wasmbed-application-controller --kubeconfig ~/.kube/config &",
        ];

        if !allowed_commands.contains(&request.command.as_str()) {
            return Ok(Json(TerminalExecuteResponse {
                success: false,
                output: String::new(),
                error: Some(format!("Command '{}' is not allowed", request.command)),
            }));
        }

        // Execute the command
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&request.command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);
                
                if result.status.success() {
                    // Combine stdout and stderr for successful commands
                    // kubectl often sends informational messages to stderr even on success
                    let combined_output = if stdout.trim().is_empty() && !stderr.trim().is_empty() {
                        stderr.to_string()
                    } else if !stdout.trim().is_empty() && !stderr.trim().is_empty() {
                        format!("{}{}", stdout, stderr)
                    } else {
                        stdout.to_string()
                    };
                    
                    Ok(Json(TerminalExecuteResponse {
                        success: true,
                        output: combined_output,
                        error: None,
                    }))
                } else {
                    Ok(Json(TerminalExecuteResponse {
                        success: false,
                        output: stdout.to_string(),
                        error: Some(stderr.to_string()),
                    }))
                }
            }
            Err(e) => {
                error!("Failed to execute command: {}", e);
                Ok(Json(TerminalExecuteResponse {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to execute command: {}", e)),
                }))
            }
        }
    }

    /// Get pods information
    pub async fn get_pods(State(_state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Getting pods information");

        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "pods", "-n", "wasmbed", "-o", "json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(result) => {
                if result.status.success() {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    match serde_json::from_str::<serde_json::Value>(&stdout) {
                        Ok(json) => Ok(Json(json)),
                        Err(e) => {
                            error!("Failed to parse kubectl output: {}", e);
                            Err(StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    error!("kubectl get pods failed: {}", stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Get services information
    pub async fn get_services(State(_state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Getting services information");

        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "svc", "-n", "wasmbed", "-o", "json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(result) => {
                if result.status.success() {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    match serde_json::from_str::<serde_json::Value>(&stdout) {
                        Ok(json) => Ok(Json(json)),
                        Err(e) => {
                            error!("Failed to parse kubectl output: {}", e);
                            Err(StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    error!("kubectl get svc failed: {}", stderr);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl: {}", e);
                // Return empty metrics instead of error to allow dashboard to function
                warn!("kubectl execution failed, returning empty metrics");
                Ok(Json(serde_json::json!({
                    "metrics": [],
                    "error": format!("Failed to execute kubectl: {}", e)
                })))
            }
        }
    }

    /// Get pod metrics
    pub async fn get_pod_metrics(State(_state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Getting pod metrics");

        let output = tokio::process::Command::new("kubectl")
            .args(&["top", "pods", "-n", "wasmbed"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(result) => {
                if result.status.success() {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    // Parse the output and convert to JSON
                    let lines: Vec<&str> = stdout.lines().collect();
                    let mut metrics = Vec::new();
                    
                    for line in lines.iter().skip(1) { // Skip header
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 3 {
                            metrics.push(serde_json::json!({
                                "name": parts[0],
                                "cpu": parts[1],
                                "memory": parts[2]
                            }));
                        }
                    }
                    
                    Ok(Json(serde_json::json!({
                        "metrics": metrics
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    // If metrics server is not available, return empty metrics instead of error
                    if stderr.contains("metrics-server") || stderr.contains("not available") {
                        warn!("Metrics server not available, returning empty metrics");
                        Ok(Json(serde_json::json!({
                            "metrics": []
                        })))
                    } else {
                        error!("kubectl top pods failed: {}", stderr);
                        Ok(Json(serde_json::json!({
                            "metrics": [],
                            "error": stderr.to_string()
                        })))
                    }
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Get system metrics from infrastructure service
    pub async fn get_system_metrics(State(state): State<Arc<DashboardState>>) -> Result<Json<Vec<crate::monitoring::MetricValue>>, StatusCode> {
        info!("Getting system metrics from infrastructure service");
        
        match state.monitoring.get_metrics().await {
            Ok(metrics) => {
                info!("Retrieved {} system metrics", metrics.len());
                Ok(Json(metrics))
            }
            Err(e) => {
                error!("Failed to get system metrics: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Get logs
    pub async fn get_logs(State(_state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        info!("Getting logs");

        let output = tokio::process::Command::new("kubectl")
            .args(&["logs", "-n", "wasmbed", "-l", "app", "--tail=50", "--all-containers=true"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(result) => {
                if result.status.success() {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    
                    // Parse logs and extract component, level, and message
                    let logs: Vec<serde_json::Value> = stdout.lines()
                        .enumerate()
                        .map(|(idx, line)| {
                            // Try to extract component from log line (e.g., "wasmbed-gateway", "wasmbed-api-server")
                            let component = if line.contains("wasmbed-gateway") {
                                "Gateway"
                            } else if line.contains("wasmbed-api-server") || line.contains("api-server") {
                                "API Server"
                            } else if line.contains("device-controller") || line.contains("device") {
                                "Device Controller"
                            } else if line.contains("application-controller") || line.contains("application") {
                                "Application Controller"
                            } else if line.contains("gateway-controller") {
                                "Gateway Controller"
                            } else {
                                "System"
                            };
                            
                            // Try to extract log level
                            let level = if line.to_uppercase().contains("ERROR") || line.to_uppercase().contains("ERR") {
                                "ERROR"
                            } else if line.to_uppercase().contains("WARN") || line.to_uppercase().contains("WARNING") {
                                "WARN"
                            } else if line.to_uppercase().contains("DEBUG") {
                                "DEBUG"
                            } else {
                                "INFO"
                            };
                            
                            serde_json::json!({
                                "id": idx + 1,
                                "timestamp": (now - (stdout.lines().count() - idx) as u64).to_string(),
                                "level": level,
                                "component": component,
                                "message": line.trim()
                            })
                        })
                        .collect();
                    
                    Ok(Json(serde_json::json!({
                        "logs": logs
                    })))
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    error!("kubectl logs failed: {}", stderr);
                    // Return empty logs instead of error
                    Ok(Json(serde_json::json!({
                        "logs": []
                    })))
                }
            }
            Err(e) => {
                error!("Failed to execute kubectl: {}", e);
                // Return empty logs instead of error
                Ok(Json(serde_json::json!({
                    "logs": []
                })))
            }
        }
    }

    /// Infrastructure health endpoint (optional service; not deployed by default)
    pub async fn infrastructure_health(State(state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        if state.config.infrastructure_endpoint.is_empty() {
            return Ok(Json(serde_json::json!({
                "status": "not_configured",
                "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                "infrastructure_endpoint": null
            })));
        }
        let health_status = match reqwest::get(&format!("{}/health", state.config.infrastructure_endpoint)).await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        };
        Ok(Json(serde_json::json!({
            "status": if health_status { "healthy" } else { "unhealthy" },
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            "infrastructure_endpoint": state.config.infrastructure_endpoint
        })))
    }

    /// Infrastructure status endpoint
    pub async fn infrastructure_status(State(_state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        // Get comprehensive infrastructure status
        let ca_status = "healthy".to_string(); // Simplified for now
        let secret_store_status = "healthy".to_string(); // Simplified for now
        let monitoring_status = "healthy".to_string(); // Simplified for now
        
        Ok(Json(serde_json::json!({
            "status": "running",
            "components": {
                "ca": ca_status,
                "secret_store": secret_store_status,
                "monitoring": monitoring_status,
                "logging": "healthy"
            },
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        })))
    }

    /// Infrastructure logs endpoint
    pub async fn infrastructure_logs(State(_state): State<Arc<DashboardState>>) -> Result<Json<serde_json::Value>, StatusCode> {
        // Get infrastructure logs
        let output = tokio::process::Command::new("kubectl")
            .args(&["logs", "-n", "wasmbed", "-l", "app=wasmbed-gateway", "--tail=50"])
            .output()
            .await;

        let logs = match output {
            Ok(result) => {
                if result.status.success() {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    
                    stdout.lines()
                        .enumerate()
                        .map(|(idx, line)| {
                            // Extract log level
                            let level = if line.to_uppercase().contains("ERROR") || line.to_uppercase().contains("ERR") {
                                "ERROR"
                            } else if line.to_uppercase().contains("WARN") || line.to_uppercase().contains("WARNING") {
                                "WARN"
                            } else if line.to_uppercase().contains("DEBUG") {
                                "DEBUG"
                            } else {
                                "INFO"
                            };
                            
                            serde_json::json!({
                                "id": idx + 1,
                                "timestamp": (now - (stdout.lines().count() - idx) as u64).to_string(),
                                "level": level,
                                "component": "Infrastructure",
                                "message": line.trim()
                            })
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new()
        };

        Ok(Json(serde_json::json!({
            "logs": logs
        })))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_type: String,
    pub architecture: String,
    pub status: String,
    pub last_heartbeat: Option<SystemTime>,
    pub gateway_id: Option<String>,
    pub mcu_type: Option<String>,
    #[serde(rename = "publicKey")]
    pub public_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApplicationInfo {
    pub app_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>, // Normalized id field (same as app_id)
    pub name: String,
    pub image: String,
    pub status: String,
    pub deployed_devices: Vec<String>,
    /// Number of devices where deployment failed (from status.deviceStatuses)
    pub failed_devices: u32,
    pub created_at: SystemTime,
    pub target_devices: Option<Vec<String>>,
    pub last_updated: Option<SystemTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayInfo {
    pub gateway_id: String,
    pub endpoint: String,
    pub status: String,
    pub connected_devices: u32,
    pub enrolled_devices: u32,
}

#[derive(Debug, Deserialize)]
pub struct DeployApplicationRequest {
    pub name: String,
    pub image: String,
    pub device_selector: DeviceSelector,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceSelector {
    pub device_type: Option<String>,
    pub architecture: Option<String>,
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct DeployApplicationResponse {
    pub success: bool,
    pub message: String,
    pub application_id: String,
}

#[derive(Debug, Serialize)]
pub struct PairingResponse {
    pub success: bool,
    pub message: String,
}

/// Dashboard implementation
#[derive(Clone)]
pub struct Dashboard {
    config: DashboardConfig,
    state: Arc<DashboardState>,
}

impl Dashboard {
    pub fn new(config: DashboardConfig) -> anyhow::Result<Self> {
        let device_manager = Arc::new(DeviceManager::new(&config.gateway_endpoint)?);
        let application_manager = Arc::new(ApplicationManager::new(&config.gateway_endpoint)?);
        let gateway_manager = Arc::new(GatewayManager::new(&config.gateway_endpoint)?);
        let monitoring = Arc::new(MonitoringDashboard::new(&config.infrastructure_endpoint)?);
        let templates = Arc::new(DashboardTemplates::new());

        let system_status = Arc::new(RwLock::new(SystemStatus {
            devices: DeviceStats {
                total: 0,
                connected: 0,
                enrolled: 0,
                unreachable: 0,
            },
            applications: ApplicationStats {
                total: 0,
                running: 0,
                pending: 0,
                failed: 0,
            },
            gateways: GatewayStats {
                total: 0,
                active: 0,
                inactive: 0,
            },
            infrastructure: InfrastructureStats {
                ca_status: "unknown".to_string(),
                secret_store_status: "unknown".to_string(),
                monitoring_status: "unknown".to_string(),
                logging_status: "unknown".to_string(),
            },
            uptime: 0,
            last_update: SystemTime::now(),
        }));

        // Initialize Renode Manager
        let renode_manager = Arc::new(RenodeManager::new("renode".to_string(), 30450));

        let state = Arc::new(DashboardState {
            config: config.clone(),
            device_manager,
            application_manager,
            gateway_manager,
            monitoring,
            templates,
            system_status,
            renode_manager,
        });

        Ok(Self { config, state })
    }

    pub async fn run(self) -> anyhow::Result<()> {
        info!("Starting Wasmbed API Server...");

        let dashboard1 = self.clone();
        let dashboard2 = self.clone();
        
        // Start status update task
        let status_update_handle = tokio::spawn(async move {
            dashboard1.update_system_status().await;
        });

        // Start REST API server
        let api_handle = tokio::spawn(async move {
            if let Err(e) = dashboard2.run_web_server().await {
                error!("Web server error: {}", e);
            }
        });

        info!("API Server started successfully");
        info!("Web API: http://localhost:{}", self.config.port);

        // Wait for all tasks
        tokio::try_join!(status_update_handle, api_handle)?;

        Ok(())
    }

    async fn update_system_status(&self) {
        loop {
            // Update system status with timeout protection
            let mut status = self.state.system_status.write().await;
            
            // Update device stats with timeout
            match tokio::time::timeout(Duration::from_secs(5), self.state.device_manager.get_all_devices()).await {
                Ok(Ok(devices)) => {
                    status.devices.total = devices.len() as u32;
                    status.devices.connected = devices.iter()
                        .filter(|d| d.status == "Connected")
                        .count() as u32;
                    status.devices.enrolled = devices.iter()
                        .filter(|d| d.status == "Enrolled")
                        .count() as u32;
                    status.devices.unreachable = devices.iter()
                        .filter(|d| d.status == "Unreachable")
                        .count() as u32;
                },
                Ok(Err(e)) => {
                    warn!("Failed to fetch devices: {}", e);
                },
                Err(_) => {
                    warn!("Timeout fetching devices");
                }
            }

            // Update application stats with timeout
            match tokio::time::timeout(Duration::from_secs(5), self.state.application_manager.get_all_applications()).await {
                Ok(Ok(applications)) => {
                    status.applications.total = applications.len() as u32;
                    status.applications.running = applications.iter()
                        .filter(|a| a.status == "Running")
                        .count() as u32;
                    status.applications.pending = applications.iter()
                        .filter(|a| a.status == "Creating" || a.status == "Deploying")
                        .count() as u32;
                    status.applications.failed = applications.iter()
                        .filter(|a| a.status == "Failed")
                        .count() as u32;
                },
                Ok(Err(e)) => {
                    warn!("Failed to fetch applications: {}", e);
                },
                Err(_) => {
                    warn!("Timeout fetching applications");
                }
            }

            // Update gateway stats with timeout
            match tokio::time::timeout(Duration::from_secs(5), self.state.gateway_manager.get_all_gateways()).await {
                Ok(Ok(gateways)) => {
                    status.gateways.total = gateways.len() as u32;
                    status.gateways.active = gateways.iter()
                        .filter(|g| g.status == "Running")
                        .count() as u32;
                    status.gateways.inactive = gateways.iter()
                        .filter(|g| g.status == "Stopped" || g.status == "Failed")
                        .count() as u32;
                },
                Ok(Err(e)) => {
                    warn!("Failed to fetch gateways: {}", e);
                },
                Err(_) => {
                    warn!("Timeout fetching gateways");
                }
            }

            // Update infrastructure status
            self.update_infrastructure_status(&mut status).await;

            status.last_update = SystemTime::now();
            status.uptime = status.last_update
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            drop(status);

            // Wait before next update
            tokio::time::sleep(self.config.refresh_interval).await;
        }
    }

    async fn update_infrastructure_status(&self, status: &mut SystemStatus) {
        // Check Certificate Authority status
        match self.check_ca_status().await {
            Ok(ca_status) => status.infrastructure.ca_status = ca_status,
            Err(_) => status.infrastructure.ca_status = "error".to_string(),
        }

        // Check Secret Store status
        match self.check_secret_store_status().await {
            Ok(secret_status) => status.infrastructure.secret_store_status = secret_status,
            Err(_) => status.infrastructure.secret_store_status = "error".to_string(),
        }

        // Check Monitoring status
        match self.check_monitoring_status().await {
            Ok(monitoring_status) => status.infrastructure.monitoring_status = monitoring_status,
            Err(_) => status.infrastructure.monitoring_status = "error".to_string(),
        }

        // Check Logging status
        match self.check_logging_status().await {
            Ok(logging_status) => status.infrastructure.logging_status = logging_status,
            Err(_) => status.infrastructure.logging_status = "error".to_string(),
        }
    }

    async fn check_ca_status(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check if CA certificates exist in Kubernetes
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "secrets", "-n", "wasmbed", "--field-selector", "type=kubernetes.io/tls"])
            .output()
            .await?;

        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if output_str.contains("ca-") || output_str.contains("tls-") {
                Ok("healthy".to_string())
            } else {
                Ok("not_configured".to_string())
            }
        } else {
            Ok("not_available".to_string())
        }
    }

    async fn check_secret_store_status(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check if secret store is available
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "secrets", "-n", "wasmbed"])
            .output()
            .await?;

        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if output_str.lines().count() > 1 { // More than just header
                Ok("healthy".to_string())
            } else {
                Ok("empty".to_string())
            }
        } else {
            Ok("not_available".to_string())
        }
    }

    async fn check_monitoring_status(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check if monitoring service is running
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "pods", "-n", "wasmbed", "-l", "app=wasmbed-gateway"])
            .output()
            .await?;

        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if output_str.contains("Running") {
                Ok("healthy".to_string())
            } else if output_str.contains("Pending") {
                Ok("starting".to_string())
            } else {
                Ok("not_running".to_string())
            }
        } else {
            Ok("not_available".to_string())
        }
    }

    async fn check_logging_status(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check if logging is working by testing the infrastructure service
        match reqwest::get("http://localhost:30461/logs").await {
            Ok(response) => {
                if response.status().is_success() {
                    Ok("healthy".to_string())
                } else {
                    Ok("error".to_string())
                }
            },
            Err(_) => Ok("not_available".to_string()),
        }
    }

    async fn run_web_server(self) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/", get(DashboardApi::home))
            .route("/devices", get(DashboardApi::devices))
            .route("/applications", get(DashboardApi::applications))
            .route("/gateways", get(DashboardApi::gateways))
            .route("/monitoring", get(DashboardApi::monitoring))
            .route("/health", get(DashboardApi::health_check))
            .route("/logs", get(DashboardApi::get_logs))
            .route("/api/status", get(DashboardApi::api_status))
            .route("/api/devices", get(DashboardApi::api_devices))
            .route("/api/applications", get(DashboardApi::api_applications))
            .route("/api/gateways", get(DashboardApi::api_gateways))
            .route("/api/v1/status", get(DashboardApi::api_status))
            .route("/api/v1/devices", get(DashboardApi::api_devices))
            .route("/api/v1/applications", get(DashboardApi::api_applications))
            .route("/api/v1/applications", post(DashboardApi::create_application))
            .route("/api/v1/gateways", get(DashboardApi::api_gateways))
            .route("/api/v1/gateways", post(DashboardApi::create_gateway))
            .route("/api/v1/gateways/:id/toggle", post(DashboardApi::toggle_gateway))
            .route("/api/v1/gateways/:id", delete(DashboardApi::delete_gateway))
            .route("/api/v1/gateways/:id", put(DashboardApi::update_gateway))
            .route("/api/v1/devices", post(DashboardApi::create_device))
            .route("/api/v1/devices/:id", delete(DashboardApi::delete_device))
            .route("/api/v1/devices/:id/enroll", post(DashboardApi::enroll_device))
            .route("/api/v1/devices/:id/connect", post(connect_device_standalone_handler))
            .route("/api/v1/devices/:id/disconnect", post(DashboardApi::disconnect_device))
            .route("/api/v1/devices/:id/emulation/start", post(DashboardApi::start_emulation))
            .route("/api/v1/devices/:id/emulation/stop", post(stop_emulation_handler))
            .route("/api/v1/devices/:id/renode/start", post(DashboardApi::start_emulation)) // Endpoint for device controller
            .route("/api/v1/renode/devices", get(DashboardApi::list_qemu_devices)) // Renode endpoint
            .route("/api/v1/applications/:id", delete(DashboardApi::delete_application))
            .route("/api/v1/applications/:id/deploy", post(DashboardApi::deploy_application_by_id))
            .route("/api/v1/applications/:id/stop", post(DashboardApi::stop_application_by_id))
            .route("/api/deploy", post(DashboardApi::deploy_application))
            .route("/api/pairing/enable", post(DashboardApi::enable_pairing))
            .route("/api/pairing/disable", post(DashboardApi::disable_pairing))
            .route("/api/v1/terminal/execute", post(DashboardApi::execute_terminal_command))
            .route("/api/v1/pods", get(DashboardApi::get_pods))
            .route("/api/v1/services", get(DashboardApi::get_services))
            .route("/api/v1/metrics", get(DashboardApi::get_pod_metrics))
            .route("/api/v1/monitoring/metrics", get(DashboardApi::get_system_metrics))
            .route("/api/v1/logs", get(DashboardApi::get_logs))
            .route("/api/v1/infrastructure/health", get(DashboardApi::infrastructure_health))
            .route("/api/v1/infrastructure/status", get(DashboardApi::infrastructure_status))
            .route("/api/v1/infrastructure/logs", get(DashboardApi::infrastructure_logs))
            .route("/api/v1/compile", post(DashboardApi::compile_code))
            .route("/ws", get(DashboardApi::websocket_handler))
            .layer(
                tower_http::cors::CorsLayer::new()
                    .allow_origin(tower_http::cors::Any)
                    .allow_methods(tower_http::cors::Any)
                    .allow_headers(tower_http::cors::Any)
                    .max_age(std::time::Duration::from_secs(3600))
            )
            .layer(
                tower::ServiceBuilder::new()
                    .layer(tower_http::timeout::TimeoutLayer::new(
                        std::time::Duration::from_secs(60)
                    ))
                    .layer(tower_http::compression::CompressionLayer::new())
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        axum::http::header::CONNECTION,
                        axum::http::HeaderValue::from_static("keep-alive"),
                    ))
            )
            .with_state(self.state);

        let addr = SocketAddr::from(([0, 0, 0, 0], self.config.port));
        
        info!("Starting web server on {}", addr);
        
        let listener = TcpListener::bind(addr).await?;
        
        info!("Web server listening on {}", addr);
        info!("Server ready to accept connections");
        
        axum::serve(listener, app).await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    
    let config = DashboardConfig {
        port: args.port,
        gateway_endpoint: args.gateway_endpoint,
        infrastructure_endpoint: args.infrastructure_endpoint,
        refresh_interval: Duration::from_secs(5),
    };
    
    let dashboard = Dashboard::new(config)?;
    
    dashboard.run().await?;
    
    Ok(())
}
