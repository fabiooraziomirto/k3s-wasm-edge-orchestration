// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use kube::{Client, ResourceExt};
use kube::api::Api;
use tokio_util::sync::CancellationToken;
use tracing::{Level, error, info, warn, debug};
use tracing_subscriber::FmtSubscriber;
use base64;

use wasmbed_k8s_resource::{
    Application, ApplicationPhase, ApplicationStatusUpdate, Device, DeviceApplicationPhase,
    DeviceApplicationStatus, DevicePhase, DeviceStatusUpdate, Gateway,
};
use wasmbed_protocol::{ClientMessage, ServerMessage, DeviceUuid};
use wasmbed_tls_utils::{TlsUtils, GatewayServer, GatewayServerConfig, ServerIdentity, AuthorizationResult, MessageContextWithKey, OnClientConnectWithKey, OnClientDisconnectWithKey, OnClientMessageWithKey, OnConnectionReadyWithKey};
use wasmbed_types::{GatewayReference, PublicKey};
use rustls;

mod http_api;
use http_api::{HttpApiServer, DeviceCapabilities};

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
struct Args {
    #[arg(long, env = "WASMBED_GATEWAY_BIND_ADDR")]
    bind_addr: SocketAddr,
    #[arg(long, env = "WASMBED_GATEWAY_HTTP_ADDR", default_value = "0.0.0.0:8080")]
    http_addr: SocketAddr,
    #[arg(long, env = "WASMBED_GATEWAY_PRIVATE_KEY")]
    private_key: PathBuf,
    #[arg(long, env = "WASMBED_GATEWAY_CERTIFICATE")]
    certificate: PathBuf,
    #[arg(long, env = "WASMBED_GATEWAY_CLIENT_CA")]
    client_ca: PathBuf,
    #[arg(long, env = "WASMBED_GATEWAY_NAMESPACE")]
    namespace: String,
    #[arg(long, env = "WASMBED_GATEWAY_POD_NAMESPACE")]
    pod_namespace: String,
    #[arg(long, env = "WASMBED_GATEWAY_POD_NAME")]
    pod_name: String,
    /// Require devices to present a certificate issued by the client CA.
    /// Off by default so the fleet can be migrated to provisioned identities
    /// before the handshake starts refusing anonymous devices.
    #[arg(long, env = "WASMBED_GATEWAY_REQUIRE_CLIENT_AUTH", default_value = "false")]
    require_client_auth: bool,
    #[arg(long, env = "WASMBED_GATEWAY_PAIRING_MODE", default_value = "false")]
    pairing_mode: bool,
    #[arg(long, env = "WASMBED_GATEWAY_PAIRING_TIMEOUT", default_value = "300")]
    pairing_timeout_seconds: u64,
    #[arg(long, env = "WASMBED_GATEWAY_HEARTBEAT_TIMEOUT", default_value = "90")]
    heartbeat_timeout_seconds: u64,
}

struct Callbacks {
    require_client_auth: bool,
    api: Api<Device>,
    gateway_reference: GatewayReference,
    http_server: Arc<HttpApiServer>,
}

impl Callbacks {
    fn on_connect(&self) -> OnClientConnectWithKey {
        let api = self.api.clone();
        let gateway_reference = self.gateway_reference.clone();
        let http_server = self.http_server.clone();
        let require_client_auth = self.require_client_auth;
        Box::new(move |public_key: Vec<u8>| {
            let api = api.clone();
            let gateway_reference = gateway_reference.clone();
            let http_server = http_server.clone();
            Box::pin(async move {
                println!("[on_connect] public_key={} bytes", public_key.len());
                // Convert Vec<u8> to PublicKey for device lookup
                let public_key_obj = PublicKey::from(public_key.as_slice());
                
                // Verify TLS client authentication by checking if the public key
                // from the client certificate matches a registered device
                match Device::find(api.clone(), public_key_obj.clone()).await {
                    Ok(Some(device)) => {
                        // Verify that the public key from the certificate matches the stored device public key
                        if device.spec.public_key == public_key_obj.to_string() {
                            // Device exists and public key matches, mark as connected
                            info!("TLS client certificate verification successful: public key matches stored device {}", device.name_any());
                            
                            // Validate state transition and update to Connected
                            let current_phase = device.status.as_ref().map(|s| s.phase.clone()).unwrap_or_default();
                            if DeviceStatusUpdate::validate_transition(current_phase.clone(), DevicePhase::Connected) {
                                if let Err(e) = DeviceStatusUpdate::default()
                                    .mark_connected(gateway_reference.clone())
                                    .apply(api.clone(), device.clone())
                                    .await
                                {
                                    error!("Error updating DeviceStatus to Connected: {e}");
                                    return AuthorizationResult::Unauthorized;
                                }
                                info!("Device state transitioned to Connected: {:?}", public_key_obj);
                            } else {
                                warn!("Invalid state transition from {:?} to Connected for device: {:?}", current_phase, public_key_obj);
                                // Still allow connection but log the invalid transition
                                if let Err(e) = DeviceStatusUpdate::default()
                                    .mark_connected(gateway_reference.clone())
                                    .apply(api.clone(), device.clone())
                                    .await
                                {
                                    error!("Error updating DeviceStatus: {e}");
                                    return AuthorizationResult::Unauthorized;
                                }
                            }
                            
                            // Register device in HTTP API
                            let device_id = device.name_any();
                            let public_key_str = public_key_obj.to_base64();
                            let capabilities = DeviceCapabilities {
                                available_memory: 1024 * 1024 * 1024, // 1GB default
                                cpu_arch: "riscv32".to_string(),
                                wasm_features: vec!["core".to_string()],
                                max_app_size: 1024 * 1024, // 1MB default
                            };
                            http_server.register_device(device_id, public_key_str, capabilities).await;
                            
                            info!("TLS client authentication successful for existing device: {:?}", public_key_obj);
                            AuthorizationResult::Authorized
                        } else {
                            error!("TLS client authentication failed: public key mismatch for device {}", device.name_any());
                            error!("Expected: {}, Got: {}", device.spec.public_key, public_key_obj.to_string());
                            AuthorizationResult::Unauthorized
                        }
                    },
                    Ok(None) => {
                        // No TLS client cert. Only tolerated while the fleet is
                        // being migrated to provisioned identities; with client
                        // auth required the handshake never gets this far, but
                        // refuse explicitly rather than relying on that.
                        if public_key.is_empty() {
                            if require_client_auth {
                                error!("Anonymous TLS connection rejected: client certificate required");
                                return AuthorizationResult::Unauthorized;
                            }
                            println!("[on_connect] Anonymous connection → Authorized");
                            debug!("Anonymous TLS connection accepted; awaiting CBOR enrollment");
                            AuthorizationResult::Authorized
                        } else {
                            // Device sent a cert that's not registered yet; check pairing mode.
                            let pairing_mode = *http_server.pairing_mode.read().await;
                            if pairing_mode {
                                warn!("Unknown device attempting connection for enrollment: {:?}", public_key_obj);
                                AuthorizationResult::Authorized
                            } else {
                                error!("Unknown device and pairing mode disabled: {:?}", public_key_obj);
                                AuthorizationResult::Unauthorized
                            }
                        }
                    },
                    Err(e) => {
                        println!("[on_connect] K8s error → Unauthorized: {}", e);
                        error!("TLS client authentication failed: unable to check Device status: {e}");
                        AuthorizationResult::Unauthorized
                    },
                }
            })
        })
    }

    fn on_disconnect(&self) -> OnClientDisconnectWithKey {
        let api = self.api.clone();
        let http_server = self.http_server.clone();
        Box::new(move |public_key: Vec<u8>, connection_id: String| {
            let api = api.clone();
            let http_server = http_server.clone();
            Box::pin(async move {
                // Clean up in-memory connection state unconditionally.
                http_server.remove_connection(&connection_id).await;

                // Mark device as disconnected in K8s (only when we have a key to look it up).
                if !public_key.is_empty() {
                    let public_key_obj = PublicKey::from(public_key.as_slice());
                    match Device::find(api.clone(), public_key_obj.clone()).await {
                        Ok(Some(device)) => {
                            if let Err(e) = DeviceStatusUpdate::default()
                                .mark_disconnected()
                                .apply(api.clone(), device.clone())
                                .await
                            {
                                error!("Error updating DeviceStatus on disconnect: {e}");
                            } else {
                                info!("Device marked as disconnected: {:?}", public_key_obj);
                            }
                        },
                        Ok(None) => {
                            debug!("Unknown device disconnected: {:?}", public_key_obj);
                        },
                        Err(e) => {
                            error!("Error checking device status on disconnect: {e}");
                        },
                    }
                } else {
                    info!("Anonymous device disconnected (connection {})", connection_id);
                }
            })
        })
    }

    fn on_connection_ready(&self) -> OnConnectionReadyWithKey {
        let http_server = self.http_server.clone();
        Box::new(move |public_key: Vec<u8>, connection_id: String, sender: tokio::sync::mpsc::Sender<ServerMessage>| {
            let http_server = http_server.clone();
            Box::pin(async move {
                // Always park the sender under the connection_id so that CBOR enrollment
                // can activate it once the device's identity is known.
                http_server.store_pending_sender(&connection_id, sender.clone()).await;
                // If the device connected with a TLS client certificate (already enrolled),
                // also register by public key so existing lookup paths continue to work.
                if !public_key.is_empty() {
                    http_server.set_device_tls_sender(&public_key, sender).await;
                }
            })
        })
    }

    fn on_message(&self) -> OnClientMessageWithKey {
        let api = self.api.clone();
        let gateway_reference = self.gateway_reference.clone();
        let http_server = self.http_server.clone();
        let require_client_auth = self.require_client_auth;
        Box::new(move |ctx: MessageContextWithKey| {
            let api = api.clone();
            let gateway_reference = gateway_reference.clone();
            let http_server = http_server.clone();
            Box::pin(async move {
                use base64::{engine::general_purpose::STANDARD as BASE64_STD, Engine as _};
                let key_b64_top = BASE64_STD.encode(ctx.client_public_key());

                // Resolve device_id for this message (works for both cert-authenticated
                // and anonymous CBOR-enrolled devices).
                let resolved_device_id = http_server
                    .resolve_device_id(&ctx.connection_id, &key_b64_top)
                    .await;

                match ctx.message() {
                    Some(ClientMessage::Heartbeat) => {
                        let _ = ctx.reply(ServerMessage::HeartbeatAck);
                        // Process heartbeat against K8s only when we know the device.
                        if let Some(ref device_id) = resolved_device_id {
                            http_server.update_heartbeat(device_id).await;
                            match api.get(device_id).await {
                                Ok(device) => {
                                    let update = if matches!(device.status.as_ref(), Some(s) if s.phase == DevicePhase::Unreachable) {
                                        info!("Device {} sent heartbeat while Unreachable, recovering to Connected", device_id);
                                        DeviceStatusUpdate::default()
                                            .mark_connected(gateway_reference.clone())
                                            .last_heartbeat(Some(chrono::Utc::now()))
                                    } else {
                                        DeviceStatusUpdate::default().update_heartbeat()
                                    };
                                    if let Err(e) = update.apply(api.clone(), device).await {
                                        error!("Error updating heartbeat for {}: {e}", device_id);
                                    }
                                },
                                Err(e) => error!("K8s error on heartbeat for {}: {e}", device_id),
                            }
                        } else {
                            debug!("Heartbeat from connection {} — device not yet identified", ctx.connection_id);
                        }
                    },
                    Some(ClientMessage::EnrollmentRequest) => {
                        info!("Received enrollment request");
                        // Accept unconditionally here; the pairing-mode gate is applied
                        // in the PublicKey handler when the device reveals its key.
                        let _ = ctx.reply(ServerMessage::EnrollmentAccepted);
                    },
                    Some(ClientMessage::PublicKey { key }) => {
                        info!("Received public key during enrollment: {} bytes", key.len());

                        // The key announced over CBOR must be the one the device
                        // authenticated with at the transport layer, otherwise it
                        // could enroll as any identity it likes over a channel it
                        // holds under a different one.
                        let tls_public_key_bytes = ctx.client_public_key();
                        if require_client_auth && tls_public_key_bytes.is_empty() {
                            error!("Enrollment rejected: no TLS client certificate to bind the announced key to");
                            let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                                reason: "Client certificate required".as_bytes().to_vec()
                            });
                            return;
                        }
                        if !tls_public_key_bytes.is_empty() && tls_public_key_bytes != key {
                            error!("Public key mismatch between TLS certificate and CBOR message");
                            let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                                reason: "Public key mismatch with TLS certificate".as_bytes().to_vec()
                            });
                            return;
                        }
                        // Do not trust the announced key yet: a public key is not a
                        // secret, so anyone can replay one. Ask the device to sign a
                        // fresh nonce with the matching private key and defer every
                        // enrollment side effect until that signature checks out.
                        let nonce = http_server.issue_challenge(&ctx.connection_id, key).await;
                        info!(
                            "Issued proof-of-possession challenge to connection {}",
                            ctx.connection_id
                        );
                        let _ = ctx.reply(ServerMessage::Challenge { nonce });
                    },
                    Some(ClientMessage::ChallengeResponse { signature }) => {
                        // take_challenge consumes the nonce whatever happens next,
                        // so a captured signature cannot be replayed onto a second
                        // connection or a second attempt.
                        let Some(challenge) = http_server.take_challenge(&ctx.connection_id).await else {
                            warn!(
                                "ChallengeResponse on connection {} with no valid challenge outstanding",
                                ctx.connection_id
                            );
                            let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                                reason: "No challenge outstanding".as_bytes().to_vec()
                            });
                            return;
                        };

                        if let Err(e) = verify_pop_signature(
                            &challenge.announced_key,
                            &challenge.nonce,
                            signature,
                        ) {
                            error!(
                                "Proof of possession failed on connection {}: {}",
                                ctx.connection_id, e
                            );
                            let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                                reason: "Proof of possession failed".as_bytes().to_vec()
                            });
                            return;
                        }

                        info!(
                            "Proof of possession verified for connection {}",
                            ctx.connection_id
                        );
                        complete_enrollment(
                            &challenge.announced_key,
                            api.clone(),
                            gateway_reference.clone(),
                            http_server.clone(),
                            &ctx.connection_id,
                            &ctx,
                        )
                        .await;
                    },
                    Some(ClientMessage::EnrollmentAcknowledgment) => {
                        info!("Received enrollment acknowledgment");
                        
                        // Mark enrollment as completed
                        let _ = ctx.reply(ServerMessage::EnrollmentCompleted);
                        info!("Enrollment completed successfully");
                    },
                    Some(ClientMessage::ApplicationStatus { app_id, status: _status, error, metrics }) => {
                        info!("Received application status for {}: {:?}", app_id, _status);
                        if let Some(err) = error {
                            warn!("Application {} error: {}", app_id, err);
                        }
                        if let Some(m) = metrics {
                            debug!("Application {} metrics: memory={}, cpu={}%, uptime={}s, calls={}", 
                                   app_id, m.memory_usage, m.cpu_usage, m.uptime, m.function_calls);
                        }
                        let key_b64 = BASE64_STD.encode(ctx.client_public_key());
                        let device_id = http_server.resolve_device_id(&ctx.connection_id, &key_b64).await;
                        if let Some(device_id) = device_id {
                            if let Ok(app) = http_server.application_api.get(&app_id).await {
                                let dev_phase = if error.is_some() { DeviceApplicationPhase::Failed } else { DeviceApplicationPhase::Running };
                                let metrics_opt = metrics.as_ref().map(|m| wasmbed_k8s_resource::ApplicationMetrics {
                                    memory_usage: Some(m.memory_usage),
                                    cpu_usage: Some(m.cpu_usage as f64),
                                    uptime: Some(m.uptime),
                                    function_calls: Some(m.function_calls),
                                });
                                let dev_status = DeviceApplicationStatus {
                                    status: dev_phase,
                                    last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                                    metrics: metrics_opt,
                                    error: error.clone(),
                                    restart_count: 0,
                                };
                                if let Err(e) = ApplicationStatusUpdate::default()
                                    .device_status(device_id, dev_status)
                                    .apply(&http_server.application_api, &app)
                                    .await
                                {
                                    error!("Failed to patch Application {} status from telemetry: {}", app_id, e);
                                }
                            }
                        }
                    },
                    Some(ClientMessage::ApplicationDeployAck { app_id, success, error }) => {
                        info!("Received deployment acknowledgment for {}: success={}", app_id, success);
                        let key_b64 = BASE64_STD.encode(ctx.client_public_key());
                        let device_id = http_server.resolve_device_id(&ctx.connection_id, &key_b64).await;
                        let (phase, dev_phase, err_msg) = if *success {
                            (ApplicationPhase::Running, DeviceApplicationPhase::Running, None as Option<String>)
                        } else {
                            error!("Application {} deployment failed: {}", app_id, error.as_deref().unwrap_or("Unknown error"));
                            (ApplicationPhase::Failed, DeviceApplicationPhase::Failed, error.clone())
                        };
                        if let Ok(app) = http_server.application_api.get(&app_id).await {
                            let mut update = ApplicationStatusUpdate::default().phase(phase).error(err_msg.clone());
                            if let Some(ref did) = device_id {
                                let dev_status = DeviceApplicationStatus {
                                    status: dev_phase,
                                    last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                                    metrics: None,
                                    error: err_msg,
                                    restart_count: 0,
                                };
                                update = update.device_status(did.clone(), dev_status);
                            }
                            if let Err(e) = update.apply(&http_server.application_api, &app).await {
                                error!("Error updating Application CRD status (DeployAck): {}", e);
                            }
                        } else {
                            let _ = update_application_status(&http_server.application_api, &app_id, phase, error.as_deref()).await;
                        }
                    },
                    Some(ClientMessage::ApplicationStopAck { app_id, success, error }) => {
                        info!("Received stop acknowledgment for {}: success={}", app_id, success);
                        let key_b64 = BASE64_STD.encode(ctx.client_public_key());
                        let device_id = http_server.resolve_device_id(&ctx.connection_id, &key_b64).await;
                        let (phase, dev_phase, err_msg) = if *success {
                            (ApplicationPhase::Stopped, DeviceApplicationPhase::Stopped, None as Option<String>)
                        } else {
                            error!("Application {} stop failed: {}", app_id, error.as_deref().unwrap_or("Unknown error"));
                            (ApplicationPhase::Failed, DeviceApplicationPhase::Failed, error.clone())
                        };
                        if let Ok(app) = http_server.application_api.get(&app_id).await {
                            let mut update = ApplicationStatusUpdate::default().phase(phase).error(err_msg.clone());
                            if let Some(ref did) = device_id {
                                let dev_status = DeviceApplicationStatus {
                                    status: dev_phase,
                                    last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                                    metrics: None,
                                    error: err_msg,
                                    restart_count: 0,
                                };
                                update = update.device_status(did.clone(), dev_status);
                            }
                            if let Err(e) = update.apply(&http_server.application_api, &app).await {
                                error!("Error updating Application CRD status (StopAck): {}", e);
                            }
                        } else {
                            let _ = update_application_status(&http_server.application_api, &app_id, phase, error.as_deref()).await;
                        }
                    },
                    Some(ClientMessage::DeviceInfo { available_memory, cpu_arch, wasm_features, max_app_size }) => {
                        info!("Received device info: arch={}, memory={}MB, max_app_size={}KB, features={:?}", 
                              cpu_arch, available_memory / 1024 / 1024, max_app_size / 1024, wasm_features);
                        let key_b64 = BASE64_STD.encode(ctx.client_public_key());
                        if let Some(device_id) = http_server.resolve_device_id(&ctx.connection_id, &key_b64).await {
                            let capabilities = DeviceCapabilities {
                                available_memory: *available_memory,
                                cpu_arch: cpu_arch.clone(),
                                wasm_features: wasm_features.clone(),
                                max_app_size: *max_app_size,
                            };
                            http_server.update_device_capabilities(&device_id, capabilities).await;
                        }
                    },
                    None => {
                        debug!("Received message without content from device");
                    },
                }
            })
        })
    }
}

/// Create a new Device CRD in Kubernetes during enrollment
async fn create_device_crd(
    public_key: &[u8],
    device_uuid: &DeviceUuid,
    api: &Api<Device>,
    gateway_reference: &GatewayReference,
) -> Result<String, anyhow::Error> {
    // Convert public key to base64 for storage — must use URL_SAFE_NO_PAD to match
    // PublicKey::to_base64() used in Device::find() comparisons.
    let public_key_b64 = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, public_key);
    
    // Create device name from UUID
    let device_name = format!("device-{}", device_uuid.to_string().replace("-", ""));
    
    // Create Device spec
    let device_spec = wasmbed_k8s_resource::DeviceSpec {
        public_key: public_key_b64,
        mcu_type: Some("Stm32F746gDisco".to_string()),
        preferred_gateway: None,
        device_class: None,
        runtime_target: None,
    };
    
    // Create Device status
    let device_status = wasmbed_k8s_resource::DeviceStatus {
        phase: wasmbed_k8s_resource::DevicePhase::Pending,
        gateway: Some(gateway_reference.clone()),
        connected_since: None,
        last_heartbeat: None,
        pairing_mode: false,
    };
    
    // Create Device object
    let device = Device {
        metadata: kube::api::ObjectMeta {
            name: Some(device_name.clone()),
            namespace: Some("wasmbed".to_string()),
            ..Default::default()
        },
        spec: device_spec,
        status: Some(device_status),
    };
    
    // Apply to Kubernetes (get-or-create: tolerate 409 Conflict on reconnect).
    match api.create(&kube::api::PostParams::default(), &device).await {
        Ok(d) => Ok(d.name_any()),
        Err(kube::Error::Api(e)) if e.code == 409 => {
            // Device CRD already exists (e.g. reconnect after gateway restart).
            // Look it up by public key and return its name.
            let public_key_obj = PublicKey::from(public_key);
            match Device::find(api.clone(), public_key_obj).await? {
                Some(d) => {
                    info!("Device CRD already exists (409), reusing: {}", d.name_any());
                    Ok(d.name_any())
                }
                None => Err(anyhow::anyhow!("Device 409 but could not find existing CRD")),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Update Application CRD status based on MCU feedback
async fn update_application_status(
    api: &Api<Application>,
    app_id: &str,
    phase: ApplicationPhase,
    error_message: Option<&str>,
) -> Result<()> {
    use kube::api::{ListParams, Patch, PatchParams};
    use serde_json::json;
    
    // Find the application by app_id (this would need to be stored in metadata or labels)
    // For now, we'll search by name or use a different approach
    let apps = api.list(&ListParams::default()).await?;
    
    for app in apps {
        // Check if this is the application we're looking for
        // In a real implementation, we'd store the app_id in metadata or labels
        if app.spec.name.contains(app_id) || app.metadata.name.as_ref().unwrap().contains(app_id) {
            // Validate state transition
            let current_phase = app.status.as_ref().map(|s| s.phase).unwrap_or(ApplicationPhase::Creating);
            if !ApplicationPhase::validate_transition(current_phase, phase) {
                warn!("Invalid state transition from {:?} to {:?} for application {}", current_phase, phase, app.metadata.name.as_ref().unwrap());
                // Still proceed with the update but log the invalid transition
            }
            
            let mut status_patch = json!({
                "phase": phase,
                "lastUpdated": chrono::Utc::now().to_rfc3339()
            });
            
            if let Some(error) = error_message {
                status_patch["error"] = json!(error);
            }
            
            let patch = Patch::Merge(json!({
                "status": status_patch
            }));
            
            let patch_params = PatchParams::default();
            if let Err(e) = api.patch_status(&app.metadata.name.as_ref().unwrap(), &patch_params, &patch).await {
                error!("Failed to patch Application {} status: {}", app.metadata.name.as_ref().unwrap(), e);
                return Err(e.into());
            } else {
                info!("Updated Application {} status to {:?}", app.metadata.name.as_ref().unwrap(), phase);
                break;
            }
        }
    }
    
    Ok(())
}

/// Check for devices with expired heartbeats and mark them as unreachable
async fn check_heartbeat_timeouts(api: &Api<Device>, timeout_duration: Duration) -> Result<()> {
    use chrono::Utc;
    use kube::api::ListParams;
    
    // Try to list devices with retry logic
    let devices = match api.list(&ListParams::default()).await {
        Ok(devices) => devices,
        Err(e) => {
            warn!("Failed to list devices for heartbeat check: {}", e);
            return Err(e.into());
        }
    };
    
    let now = Utc::now();
    
    for device in devices {
        if let Some(status) = &device.status {
            if let Some(last_heartbeat) = status.last_heartbeat {
                let time_since_heartbeat = now.signed_duration_since(last_heartbeat);
                
                // Check if heartbeat has timed out
                if time_since_heartbeat.num_seconds() > timeout_duration.as_secs() as i64 {
                    // Only mark as unreachable if device is currently connected
                    if status.phase == DevicePhase::Connected {
                        info!("Device {} heartbeat timed out, marking as unreachable", device.name_any());
                        
                        if let Err(e) = DeviceStatusUpdate::default()
                            .mark_unreachable()
                            .apply(api.clone(), device.clone())
                            .await
                        {
                            warn!("Error marking device {} as unreachable: {}", device.name_any(), e);
                            // Don't fail the entire heartbeat check for individual device errors
                        } else {
                            info!("Device {} marked as unreachable due to heartbeat timeout", device.name_any());
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    // Bridge `log` crate → tracing so wasmbed-tls-utils messages are visible.
    tracing_log::LogTracer::init().ok();

    let args = Args::parse();

    let private_key_bytes =
        std::fs::read(&args.private_key).with_context(|| {
            format!(
                "Failed to read private key from {}",
                args.private_key.display()
            )
        })?;
    let certificate_bytes =
        std::fs::read(&args.certificate).with_context(|| {
            format!(
                "Failed to read certificate from {}",
                args.certificate.display()
            )
        })?;
    let client_ca_bytes =
        std::fs::read(&args.client_ca).with_context(|| {
            format!(
                "Failed to read client CA certificate from {}",
                args.client_ca.display()
            )
        })?;

    // Parse PEM certificates using our custom TLS utils
    let private_key = TlsUtils::parse_private_key(&private_key_bytes)
        .with_context(|| "Failed to parse private key")?;
    
    let certificate = TlsUtils::parse_certificate(&certificate_bytes)
        .with_context(|| "Failed to parse certificate")?;
    
    let client_ca_certs = TlsUtils::parse_certificates(&client_ca_bytes)
        .with_context(|| "Failed to parse client CA certificates")?;

    let server_key = match private_key {
        rustls_pki_types::PrivateKeyDer::Pkcs8(pkcs8) => pkcs8,
        _ => return Err(anyhow::anyhow!("Only PKCS8 private keys are supported")),
    };
    
    let client_ca = client_ca_certs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No CA certificate found in PEM file"))?;

    let identity = ServerIdentity::from_parts(server_key, certificate);

    let gateway_reference =
        GatewayReference::new(&args.pod_namespace, &args.pod_name);

    let shutdown = CancellationToken::new();
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received Ctrl+C, shutting down...");
                shutdown_clone.cancel();
            },
            Err(err) => {
                error!("Unable to listen for shutdown signal: {}", err);
            },
        }
    });

    // Create Kubernetes client with robust configuration and retry logic
    let client = match Client::try_default().await {
        Ok(client) => {
            info!("Kubernetes client created successfully with default config");
            client
        }
        Err(e) => {
            warn!("Failed to create Kubernetes client with default config: {}", e);
            // Try to create client with explicit configuration
            info!("Attempting to create client with explicit configuration");
            let config = kube::Config::infer().await.map_err(|e| {
                error!("Failed to infer Kubernetes config: {}", e);
                e
            })?;
            let client = Client::try_from(config).map_err(|e| {
                error!("Failed to create client from config: {}", e);
                e
            })?;
            info!("Kubernetes client created successfully with explicit config");
            client
        }
    };
    
    // Test the client connection
    info!("Testing Kubernetes client connection...");
    match client.list_api_groups().await {
        Ok(_) => info!("Kubernetes client connection test successful"),
        Err(e) => warn!("Kubernetes client connection test failed: {}", e),
    }
    let api: Api<Device> = Api::namespaced(client.clone(), &args.namespace);
    let application_api: Api<Application> = Api::namespaced(client.clone(), &args.namespace);
    let gateway_api: Api<Gateway> = Api::namespaced(client.clone(), &args.namespace);

    // Create HTTP API server
    let http_server = HttpApiServer::new(api.clone(), application_api, gateway_api)?;
    
    // Initialize pairing mode configuration
    {
        let mut pairing_mode = http_server.pairing_mode.write().await;
        *pairing_mode = args.pairing_mode;
    }
    {
        let mut pairing_timeout = http_server.pairing_timeout_seconds.write().await;
        *pairing_timeout = args.pairing_timeout_seconds;
    }
    {
        let mut heartbeat_timeout = http_server.heartbeat_timeout_seconds.write().await;
        *heartbeat_timeout = args.heartbeat_timeout_seconds;
    }
    
    let http_server = Arc::new(http_server);

    let callbacks = Callbacks {
        require_client_auth: args.require_client_auth,
        api: api.clone(),
        gateway_reference: gateway_reference.clone(),
        http_server: http_server.clone(),
    };

    let config = GatewayServerConfig {
        bind_addr: args.bind_addr,
        identity,
        client_ca,
        require_client_auth: args.require_client_auth,
        on_client_connect: Arc::new(callbacks.on_connect()),
        on_client_disconnect: Arc::new(callbacks.on_disconnect()),
        on_client_message: Arc::new(callbacks.on_message()),
        on_connection_ready: Some(Arc::new(callbacks.on_connection_ready())),
        shutdown: shutdown.clone(),
    };

    info!("Creating GatewayServer with config");
    let server = GatewayServer::new(config);
    info!("GatewayServer created successfully");
    
    // Start HTTP API server
    let http_router = http_server.router();
    let http_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(args.http_addr).await.unwrap();
        info!("Starting HTTP API server on {}", args.http_addr);
        
        match axum::serve(listener, http_router)
            .with_graceful_shutdown(async move {
                http_shutdown.cancelled().await;
            })
            .await
        {
            Ok(_) => info!("HTTP API server stopped gracefully"),
            Err(e) => error!("HTTP API server error: {}", e),
        }
    });

    // Start heartbeat monitor task with robust error handling
    let heartbeat_monitor_api = api.clone();
    let heartbeat_monitor_shutdown = shutdown.clone();
    let heartbeat_timeout = Duration::from_secs(args.heartbeat_timeout_seconds);
    tokio::spawn(async move {
        info!("Starting heartbeat monitor with timeout: {:?}", heartbeat_timeout);
        
        loop {
            tokio::select! {
                _ = heartbeat_monitor_shutdown.cancelled() => {
                    info!("Heartbeat monitor shutting down");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    match check_heartbeat_timeouts(&heartbeat_monitor_api, heartbeat_timeout).await {
                        Ok(_) => {
                            debug!("Heartbeat check completed successfully");
                        }
                        Err(e) => {
                            // Log error but don't crash - this is a monitoring function
                            warn!("Heartbeat monitor warning: {}", e);
                            // Continue monitoring even if there are temporary connection issues
                            // The system should be resilient to temporary Kubernetes API issues
                        }
                    }
                }
            }
        }
    });

    info!("Starting TLS server on {}", args.bind_addr);
    info!("About to call server.run().await");
    if let Err(e) = server.run().await {
        error!("Server error: {}", e);
    }
    info!("server.run().await returned");

    Ok(())
}
/// Finish enrollment for a device that has proved possession of its key:
/// register it, wire its sender, and reflect the new state into Kubernetes.
///
/// Split out of the `PublicKey` handler so that none of it can run before the
/// signature over the challenge has been verified.
async fn complete_enrollment(
    key: &Vec<u8>,
    api: Api<Device>,
    gateway_reference: GatewayReference,
    http_server: Arc<HttpApiServer>,
    connection_id: &str,
    ctx: &MessageContextWithKey,
) {

    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    let key_b64 = BASE64.encode(key);
    let public_key_obj = PublicKey::from(key.as_slice());

    match Device::find(api.clone(), public_key_obj.clone()).await {
        Ok(Some(existing_device)) => {
            // RECONNECT: device was previously enrolled — skip pairing mode.
            let device_id = existing_device.name_any();
            // Reconstruct the UUID from the device name ("device-<32hexchars>").
            let uuid_hex = device_id.trim_start_matches("device-");
            let existing_uuid = uuid::Uuid::parse_str(uuid_hex)
                .map(|u| DeviceUuid::new(*u.as_bytes()))
                .unwrap_or_else(|_| DeviceUuid::new(*uuid::Uuid::new_v4().as_bytes()));

            info!("Device {} reconnected (previously enrolled)", device_id);

            // Upsert device in HTTP registry.
            let capabilities = DeviceCapabilities {
                available_memory: 0,
                cpu_arch: "unknown".to_string(),
                wasm_features: vec![],
                max_app_size: 0,
            };
            http_server.register_device(device_id.clone(), key_b64.clone(), capabilities).await;
            http_server.activate_device_sender(connection_id, &device_id, &key_b64).await;

            // Update K8s status to Connected.
            if let Err(e) = DeviceStatusUpdate::default()
                .mark_connected(gateway_reference.clone())
                .apply(api.clone(), existing_device.clone())
                .await
            {
                error!("Error updating device status to Connected on reconnect: {e}");
            }

            let _ = ctx.reply(ServerMessage::DeviceUuid { uuid: existing_uuid });
        },
        Ok(None) => {
            // NEW ENROLLMENT: check pairing mode.
            let pairing_mode = *http_server.pairing_mode.read().await;
            if !pairing_mode {
                error!("Enrollment rejected: pairing mode disabled");
                let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                    reason: "Pairing mode disabled".as_bytes().to_vec()
                });
                return;
            }

            let uuid = uuid::Uuid::new_v4();
            let device_uuid = DeviceUuid::new(*uuid.as_bytes());

            match create_device_crd(key, &device_uuid, &api, &gateway_reference).await {
                Ok(device_name) => {
                    info!("Created Device CRD: {}", device_name);

                    // Register and activate sender.
                    let capabilities = DeviceCapabilities {
                        available_memory: 0,
                        cpu_arch: "unknown".to_string(),
                        wasm_features: vec![],
                        max_app_size: 0,
                    };
                    http_server.register_device(device_name.clone(), key_b64.clone(), capabilities).await;
                    http_server.activate_device_sender(connection_id, &device_name, &key_b64).await;

                    // Mark device as enrolled in K8s.
                    if let Ok(Some(device)) = Device::find(api.clone(), public_key_obj.clone()).await {
                        if let Err(e) = DeviceStatusUpdate::default()
                            .mark_enrolled()
                            .apply(api.clone(), device.clone())
                            .await
                        {
                            error!("Error updating device status to Enrolled: {e}");
                        }
                    }

                    let _ = ctx.reply(ServerMessage::DeviceUuid { uuid: device_uuid });
                },
                Err(e) => {
                    error!("Failed to create Device CRD: {}", e);
                    let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                        reason: format!("Failed to create device: {}", e).into_bytes()
                    });
                }
            }
        },
        Err(e) => {
            error!("K8s error while looking up device: {}", e);
            let _ = ctx.reply(ServerMessage::EnrollmentRejected {
                reason: "Internal error".as_bytes().to_vec()
            });
        }
    }
}

/// Domain separation tag for the proof-of-possession transcript. Keeps a
/// signature produced here from being meaningful in any other context.
const POP_CONTEXT: &[u8] = b"wasmbed-pop-v1";

/// The bytes a device signs to prove it holds the private key for `spki`.
///
/// Binding the announced key into the transcript alongside the nonce means a
/// signature captured from one device cannot be presented by another that
/// claims a different key.
fn pop_transcript(nonce: &[u8], spki: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(POP_CONTEXT);
    hasher.update(nonce);
    hasher.update(spki);
    hasher.finalize().to_vec()
}

/// Extract the uncompressed EC point from a SubjectPublicKeyInfo DER blob.
///
/// `TlsUtils::extract_public_key` and `Device.spec.publicKey` both carry a full
/// SPKI, while ring verifies against the raw point, so unwrap one to the other.
fn ec_point_from_spki(spki: &[u8]) -> anyhow::Result<Vec<u8>> {
    use x509_parser::prelude::FromDer;
    let (_, info) = x509_parser::x509::SubjectPublicKeyInfo::from_der(spki)
        .map_err(|e| anyhow::anyhow!("not a valid SubjectPublicKeyInfo: {e}"))?;
    Ok(info.subject_public_key.data.to_vec())
}

/// Check a device's signature over the challenge transcript.
fn verify_pop_signature(spki: &[u8], nonce: &[u8], signature: &[u8]) -> anyhow::Result<()> {
    let point = ec_point_from_spki(spki)?;
    let transcript = pop_transcript(nonce, spki);
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_ASN1,
        point,
    )
    .verify(&transcript, signature)
    .map_err(|_| anyhow::anyhow!("signature does not verify against the announced key"))
}

#[cfg(test)]
mod pop_tests {
    use super::{ec_point_from_spki, pop_transcript, verify_pop_signature};

    /// A P-256 identity as a device would be provisioned with: PKCS#8 private
    /// key plus the SPKI that goes into `Device.spec.publicKey`.
    fn identity() -> (Vec<u8>, Vec<u8>) {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        (key_pair.serialize_der(), key_pair.public_key_der())
    }

    fn sign(pkcs8: &[u8], transcript: &[u8]) -> Vec<u8> {
        let rng = ring::rand::SystemRandom::new();
        let key = ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            pkcs8,
            &rng,
        )
        .unwrap();
        key.sign(&rng, transcript).unwrap().as_ref().to_vec()
    }

    #[test]
    fn accepts_a_signature_from_the_announced_key() {
        let (pkcs8, spki) = identity();
        let nonce = vec![0x11; 32];
        let sig = sign(&pkcs8, &pop_transcript(&nonce, &spki));

        assert!(verify_pop_signature(&spki, &nonce, &sig).is_ok());
    }

    #[test]
    fn rejects_a_signature_from_a_different_key() {
        let (_, spki) = identity();
        let (other_pkcs8, _) = identity();
        let nonce = vec![0x22; 32];
        let sig = sign(&other_pkcs8, &pop_transcript(&nonce, &spki));

        assert!(verify_pop_signature(&spki, &nonce, &sig).is_err());
    }

    /// The replay a stolen public key alone would otherwise allow: an attacker
    /// who watched a session cannot reuse its signature against a fresh nonce.
    #[test]
    fn rejects_a_signature_replayed_against_a_new_nonce() {
        let (pkcs8, spki) = identity();
        let captured_nonce = vec![0x33; 32];
        let sig = sign(&pkcs8, &pop_transcript(&captured_nonce, &spki));

        let fresh_nonce = vec![0x44; 32];
        assert!(verify_pop_signature(&spki, &fresh_nonce, &sig).is_err());
    }

    /// The key is bound into the transcript, so a signature made while claiming
    /// one identity does not verify while claiming another.
    #[test]
    fn transcript_is_bound_to_the_announced_key() {
        let (pkcs8, spki) = identity();
        let (_, other_spki) = identity();
        let nonce = vec![0x55; 32];
        let sig = sign(&pkcs8, &pop_transcript(&nonce, &other_spki));

        assert!(verify_pop_signature(&spki, &nonce, &sig).is_err());
    }

    #[test]
    fn unwraps_the_ec_point_from_the_spki() {
        let (_, spki) = identity();
        let point = ec_point_from_spki(&spki).unwrap();

        // Uncompressed P-256 point: 0x04 followed by two 32-byte coordinates.
        assert_eq!(point.len(), 65);
        assert_eq!(point[0], 0x04);
        assert!(spki.ends_with(&point));
    }

    #[test]
    fn rejects_a_public_key_that_is_not_an_spki() {
        assert!(ec_point_from_spki(&[0xab; 32]).is_err());
    }
}
