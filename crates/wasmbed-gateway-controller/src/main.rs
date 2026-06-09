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
use k8s_openapi::api::core::v1::{Service, Secret};
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
    #[error("Gateway error: {0}")]
    GatewayError(String),
    #[error("Anyhow error: {0}")]
    AnyhowError(#[from] anyhow::Error),
}

/// Gateway Controller
#[derive(Clone)]
pub struct GatewayController {
    client: Client,
    gateways: Api<wasmbed_k8s_resource::Gateway>,
    devices: Api<wasmbed_k8s_resource::Device>,
    deployments: Api<Deployment>,
    services: Api<Service>,
    secrets: Api<Secret>,
}

impl GatewayController {
    pub fn new(client: Client) -> Self {
        Self {
            gateways: Api::<wasmbed_k8s_resource::Gateway>::namespaced(client.clone(), "wasmbed"),
            devices: Api::<wasmbed_k8s_resource::Device>::namespaced(client.clone(), "wasmbed"),
            deployments: Api::<Deployment>::namespaced(client.clone(), "wasmbed"),
            services: Api::<Service>::namespaced(client.clone(), "wasmbed"),
            secrets: Api::<Secret>::namespaced(client.clone(), "wasmbed"),
            client,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let gateways_api = Api::<wasmbed_k8s_resource::Gateway>::namespaced(self.client.clone(), "wasmbed");
        let controller = self.clone();
        
        Controller::new(gateways_api, watcher::Config::default())
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

    pub async fn reconcile(&self, gateway: wasmbed_k8s_resource::Gateway) -> Result<Action, ControllerError> {
        let name = gateway.name_any();
        info!("Reconciling gateway: {}", name);

        // Get current status or initialize
        let current_status = self.get_gateway_status(&gateway).await?;
        
        match current_status.phase {
            wasmbed_k8s_resource::GatewayPhase::Pending => {
                self.handle_pending(&gateway).await?;
            }
            wasmbed_k8s_resource::GatewayPhase::Running => {
                self.handle_running(&gateway).await?;
            }
            wasmbed_k8s_resource::GatewayPhase::Failed => {
                self.handle_failed(&gateway).await?;
            }
            wasmbed_k8s_resource::GatewayPhase::Stopped => {
                self.handle_stopped(&gateway).await?;
            }
        }

        Ok(Action::requeue(Duration::from_secs(30)))
    }

    async fn get_gateway_status(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<wasmbed_k8s_resource::GatewayStatus, ControllerError> {
        // Get status from gateway or return default
        if let Some(status) = &gateway.status {
            Ok(status.clone())
        } else {
            Ok(wasmbed_k8s_resource::GatewayStatus {
                phase: wasmbed_k8s_resource::GatewayPhase::Pending,
                connected_devices: Some(0),
                enrolled_devices: Some(0),
                last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
                conditions: None,
            })
        }
    }

    async fn handle_pending(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<(), ControllerError> {
        info!("Handling pending gateway: {}", gateway.name_any());
        
        // Ensure certificates secret exists
        info!("Ensuring certificates secret exists for gateway {}", gateway.name_any());
        if let Err(e) = self.ensure_certificates_secret().await {
            error!("Failed to ensure certificates secret: {}", e);
            return Err(e);
        }
        info!("Certificates secret verified for gateway {}", gateway.name_any());
        
        // Create Deployment for the gateway
        info!("Creating deployment for gateway {}", gateway.name_any());
        if let Err(e) = self.create_gateway_deployment(gateway).await {
            error!("Failed to create gateway deployment: {}", e);
            return Err(e);
        }
        info!("Deployment created for gateway {}", gateway.name_any());
        
        // Create Service for the gateway
        info!("Creating service for gateway {}", gateway.name_any());
        if let Err(e) = self.create_gateway_service(gateway).await {
            error!("Failed to create gateway service: {}", e);
            return Err(e);
        }
        info!("Service created for gateway {}", gateway.name_any());
        
        // Update gateway endpoint to use the Kubernetes service DNS name
        // This is the correct endpoint for Kubernetes-internal communication
        // For Renode/TCP bridge, the API server will convert this to localhost
        let service_name = format!("{}-service", gateway.name_any());
        let correct_endpoint = format!("{}.wasmbed.svc.cluster.local:8080", service_name);
        
        // Only update if the endpoint is different (e.g., it's still using localhost, 127.0.0.1, or wrong format)
        let needs_update = gateway.spec.endpoint != correct_endpoint && 
            (gateway.spec.endpoint.starts_with("127.0.0.1") || 
             gateway.spec.endpoint.starts_with("localhost") ||
             !gateway.spec.endpoint.contains("svc.cluster.local"));
        
        if needs_update {
            info!("Updating gateway {} endpoint from '{}' to '{}'", 
                gateway.name_any(), gateway.spec.endpoint, correct_endpoint);
            
            // Patch the gateway spec to update the endpoint
            let patch = serde_json::json!({
                "apiVersion": "wasmbed.io/v1",
                "kind": "Gateway",
                "spec": {
                    "endpoint": correct_endpoint
                }
            });
            
            match self.gateways.patch(
                &gateway.name_any(),
                &PatchParams::default(),
                &Patch::Merge(patch),
            ).await {
                Ok(_) => {
                    info!("Successfully updated gateway {} endpoint to Kubernetes service DNS", gateway.name_any());
                }
                Err(e) => {
                    warn!("Failed to update gateway {} endpoint: {}. Continuing...", gateway.name_any(), e);
                }
            }
        } else {
            info!("Gateway {} endpoint is already correct: {}", gateway.name_any(), gateway.spec.endpoint);
        }
        
        // Count connected devices
        info!("Listing devices for gateway {}", gateway.name_any());
        let (connected_count, enrolled_count) = match self.devices.list(&ListParams::default()).await {
            Ok(devices) => {
                info!("Successfully listed {} devices for gateway {}", devices.items.len(), gateway.name_any());
                let connected = devices.items.iter()
                    .filter(|device| {
                        if let Some(status) = &device.status {
                            matches!(status.phase, wasmbed_k8s_resource::DevicePhase::Connected)
                        } else {
                            false
                        }
                    })
                    .count();
                (connected, devices.items.len())
            },
            Err(e) => {
                warn!("Failed to list devices for gateway {}: {}. Continuing with empty device list.", gateway.name_any(), e);
                // Continue with zero counts if listing fails - don't fail the reconcile
                (0, 0)
            }
        };

        let status = wasmbed_k8s_resource::GatewayStatus {
            phase: wasmbed_k8s_resource::GatewayPhase::Running,
            connected_devices: Some(connected_count as i32),
            enrolled_devices: Some(enrolled_count as i32),
            last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
            conditions: Some(vec![wasmbed_k8s_resource::GatewayCondition {
                r#type: "Ready".to_string(),
                status: "True".to_string(),
                reason: Some("GatewayStarted".to_string()),
                message: Some("Gateway is running and accepting connections".to_string()),
                last_transition_time: Some(chrono::Utc::now().to_rfc3339()),
            }]),
        };

        info!("Updating gateway status for {}", gateway.name_any());
        if let Err(e) = self.update_gateway_status(gateway, status).await {
            error!("Failed to update gateway status for {}: {}", gateway.name_any(), e);
            // Don't fail the reconcile - status update is best effort
            warn!("Continuing reconcile despite status update failure");
        } else {
            info!("Gateway {} moved to running phase", gateway.name_any());
        }
        Ok(())
    }

    async fn handle_running(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<(), ControllerError> {
        info!("Gateway {} is running", gateway.name_any());
        
        // Ensure endpoint is correct (update if needed)
        // Try to find the service - it might be named {gateway-name}-service or just {gateway-name}
        let service_name_with_suffix = format!("{}-service", gateway.name_any());
        let service_name_simple = gateway.name_any();
        
        // Check which service actually exists
        let service_name = match (self.services.get(&service_name_with_suffix).await, self.services.get(&service_name_simple).await) {
            (Ok(_), _) => service_name_with_suffix,
            (_, Ok(_)) => service_name_simple,
            _ => service_name_with_suffix, // Default to -service pattern if neither exists
        };
        
        let correct_endpoint = format!("{}.wasmbed.svc.cluster.local:8080", service_name);
        
        if gateway.spec.endpoint != correct_endpoint {
            info!("Updating gateway {} endpoint from '{}' to '{}'", 
                gateway.name_any(), gateway.spec.endpoint, correct_endpoint);
            
            let patch = serde_json::json!({
                "apiVersion": "wasmbed.io/v1",
                "kind": "Gateway",
                "spec": {
                    "endpoint": correct_endpoint
                }
            });
            
            match self.gateways.patch(
                &gateway.name_any(),
                &PatchParams::default(),
                &Patch::Merge(patch),
            ).await {
                Ok(_) => {
                    info!("Successfully updated gateway {} endpoint", gateway.name_any());
                }
                Err(e) => {
                    warn!("Failed to update gateway {} endpoint: {}. Continuing...", gateway.name_any(), e);
                }
            }
        }
        
        // Update device counts
        let devices = match self.devices.list(&ListParams::default()).await {
            Ok(devices) => devices,
            Err(e) => {
                error!("Failed to list devices for gateway {}: {}", gateway.name_any(), e);
                // Continue with empty device list if listing fails - use default counts
                return Ok(()); // Return early if we can't list devices
            }
        };
        let connected_count = devices.items.iter()
            .filter(|device| {
                if let Some(status) = &device.status {
                    matches!(status.phase, wasmbed_k8s_resource::DevicePhase::Connected)
                } else {
                    false
                }
            })
            .count();

        let enrolled_count = devices.items.iter()
            .filter(|device| {
                if let Some(status) = &device.status {
                    matches!(status.phase, wasmbed_k8s_resource::DevicePhase::Enrolled | wasmbed_k8s_resource::DevicePhase::Connected)
                } else {
                    false
                }
            })
            .count();

        let status = wasmbed_k8s_resource::GatewayStatus {
            phase: wasmbed_k8s_resource::GatewayPhase::Running,
            connected_devices: Some(connected_count as i32),
            enrolled_devices: Some(enrolled_count as i32),
            last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
            conditions: Some(vec![wasmbed_k8s_resource::GatewayCondition {
                r#type: "Ready".to_string(),
                status: "True".to_string(),
                reason: Some("GatewayRunning".to_string()),
                message: Some(format!("Gateway is running with {} connected devices", connected_count)),
                last_transition_time: Some(chrono::Utc::now().to_rfc3339()),
            }]),
        };
        
        if let Err(e) = self.update_gateway_status(gateway, status).await {
            error!("Failed to update gateway status for {}: {}", gateway.name_any(), e);
            // Don't fail the reconcile - status update is best effort
            warn!("Continuing reconcile despite status update failure");
        }
        Ok(())
    }

    async fn handle_failed(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<(), ControllerError> {
        warn!("Gateway {} is in failed state", gateway.name_any());
        Ok(())
    }

    async fn handle_stopped(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<(), ControllerError> {
        info!("Gateway {} is stopped", gateway.name_any());
        Ok(())
    }

    async fn update_gateway_status(&self, gateway: &wasmbed_k8s_resource::Gateway, status: wasmbed_k8s_resource::GatewayStatus) -> Result<(), ControllerError> {
        info!("Attempting to update status for gateway {}", gateway.name_any());
        
        // Serialize status to JSON manually to ensure camelCase conversion
        let status_json = serde_json::to_value(&status)
            .map_err(|e| ControllerError::SerializationError(e))?;
        
        let patch = serde_json::json!({
            "status": status_json
        });

        let params = PatchParams::apply("wasmbed-gateway-controller");
        let patch = Patch::Merge(patch);
        
        // Try patch_status first, fallback to regular patch
        match self.gateways
            .patch_status(&gateway.name_any(), &params, &patch)
            .await {
            Ok(_) => {
                info!("Successfully updated gateway status using patch_status");
                Ok(())
            },
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                info!("Status subresource not available, trying regular patch");
                // Status subresource not available, use regular patch
                match self.gateways
                    .patch(&gateway.name_any(), &params, &patch)
                    .await {
                    Ok(_) => {
                        info!("Successfully updated gateway status using regular patch");
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to update gateway status with regular patch: {}", e);
                        Err(ControllerError::KubeError(e))
                    }
                }
            }
            Err(e) => {
                error!("Failed to update gateway status: {}", e);
                // Don't fail the reconcile - log and continue
                warn!("Gateway status update failed, but continuing reconcile. Error: {}", e);
                Ok(())
            },
        }
    }

    async fn create_gateway_deployment(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<(), ControllerError> {
        let deployment_name = format!("{}-deployment", gateway.name_any());
        
        // Check if deployment already exists
        match self.deployments.get(&deployment_name).await {
            Ok(existing) => {
                // Deployment exists, check if it needs updates (e.g., imagePullPolicy)
                let needs_update = existing.spec.as_ref()
                    .and_then(|spec| spec.template.spec.as_ref())
                    .and_then(|pod_spec| pod_spec.containers.first())
                    .map(|container| {
                        container.image_pull_policy.as_ref()
                            .map(|policy| policy != "Never")
                            .unwrap_or(true)
                    })
                    .unwrap_or(false);
                
                if needs_update {
                    info!("Updating deployment {} with imagePullPolicy", deployment_name);
                    let patch = serde_json::json!({
                        "spec": {
                            "template": {
                                "spec": {
                                    "serviceAccountName": "wasmbed-gateway",
                                    "containers": [{
                                        "name": "gateway",
                                        "imagePullPolicy": "Never"
                                    }]
                                }
                            }
                        }
                    });
                    let params = PatchParams::apply("wasmbed-gateway-controller");
                    self.deployments.patch(&deployment_name, &params, &Patch::Merge(patch)).await?;
                    info!("Updated deployment {} with imagePullPolicy", deployment_name);
                } else {
                    info!("Deployment {} already exists and is up to date", deployment_name);
                }
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
                    labels.insert("app".to_string(), "wasmbed-gateway".to_string());
                    labels.insert("gateway".to_string(), gateway.name_any());
                    labels
                }),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::apps::v1::DeploymentSpec {
                replicas: Some(1),
                selector: k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector {
                    match_labels: Some({
                        let mut labels = std::collections::BTreeMap::new();
                        labels.insert("app".to_string(), "wasmbed-gateway".to_string());
                        labels.insert("gateway".to_string(), gateway.name_any());
                        labels
                    }),
                    ..Default::default()
                },
                template: k8s_openapi::api::core::v1::PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some({
                            let mut labels = std::collections::BTreeMap::new();
                            labels.insert("app".to_string(), "wasmbed-gateway".to_string());
                            labels.insert("gateway".to_string(), gateway.name_any());
                            labels
                        }),
                        ..Default::default()
                    }),
                    spec: Some(k8s_openapi::api::core::v1::PodSpec {
                        service_account_name: Some("wasmbed-gateway".to_string()),
                        containers: vec![k8s_openapi::api::core::v1::Container {
                            name: "gateway".to_string(),
                            image: Some("localhost:5000/wasmbed/gateway:latest".to_string()),
                            image_pull_policy: Some("Never".to_string()),
                            command: Some(vec![
                                "/usr/local/bin/wasmbed-gateway".to_string(),
                                "--bind-addr".to_string(),
                                format!("0.0.0.0:{}", 8443),
                                "--http-addr".to_string(),
                                "0.0.0.0:8080".to_string(),
                                "--private-key".to_string(),
                                "/certs/server-key.pem".to_string(),
                                "--certificate".to_string(),
                                "/certs/server-cert.pem".to_string(),
                                "--client-ca".to_string(),
                                "/certs/ca-cert.pem".to_string(),
                                "--namespace".to_string(),
                                "wasmbed".to_string(),
                                "--pod-namespace".to_string(),
                                "wasmbed".to_string(),
                                "--pod-name".to_string(),
                                gateway.name_any(),
                            ]),
                            ports: Some(vec![
                                k8s_openapi::api::core::v1::ContainerPort {
                                    container_port: 8080,
                                    name: Some("http".to_string()),
                                    ..Default::default()
                                },
                                k8s_openapi::api::core::v1::ContainerPort {
                                    container_port: 8443,
                                    name: Some("https".to_string()),
                                    ..Default::default()
                                },
                            ]),
                            env: Some(vec![
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "GATEWAY_NAME".to_string(),
                                    value: Some(gateway.name_any()),
                                    ..Default::default()
                                },
                                k8s_openapi::api::core::v1::EnvVar {
                                    name: "GATEWAY_ENDPOINT".to_string(),
                                    value: Some(gateway.spec.endpoint.clone()),
                                    ..Default::default()
                                },
                            ]),
                            volume_mounts: Some(vec![
                                k8s_openapi::api::core::v1::VolumeMount {
                                    name: "certificates".to_string(),
                                    mount_path: "/certs".to_string(),
                                    read_only: Some(true),
                                    ..Default::default()
                                },
                            ]),
                            ..Default::default()
                        }],
                        volumes: Some(vec![
                            k8s_openapi::api::core::v1::Volume {
                                name: "certificates".to_string(),
                                secret: Some(k8s_openapi::api::core::v1::SecretVolumeSource {
                                    secret_name: Some("gateway-certificates".to_string()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        let params = PostParams::default();
        self.deployments.create(&params, &deployment).await?;
        info!("Created deployment for gateway: {}", gateway.name_any());
        Ok(())
    }

    async fn create_gateway_service(&self, gateway: &wasmbed_k8s_resource::Gateway) -> Result<(), ControllerError> {
        let service_name = format!("{}-service", gateway.name_any());
        
        // Check if service already exists
        match self.services.get(&service_name).await {
            Ok(_) => {
                info!("Service {} already exists", service_name);
                return Ok(());
            }
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                // Service doesn't exist, create it
            }
            Err(e) => return Err(ControllerError::KubeError(e)),
        }

        let service = Service {
            metadata: ObjectMeta {
                name: Some(service_name.clone()),
                namespace: Some("wasmbed".to_string()),
                labels: Some({
                    let mut labels = std::collections::BTreeMap::new();
                    labels.insert("app".to_string(), "wasmbed-gateway".to_string());
                    labels.insert("gateway".to_string(), gateway.name_any());
                    labels
                }),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                selector: Some({
                    let mut labels = std::collections::BTreeMap::new();
                    labels.insert("app".to_string(), "wasmbed-gateway".to_string());
                    labels.insert("gateway".to_string(), gateway.name_any());
                    labels
                }),
                ports: Some(vec![
                    k8s_openapi::api::core::v1::ServicePort {
                        name: Some("http".to_string()),
                        port: 8080,
                        target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(8080)),
                        ..Default::default()
                    },
                    k8s_openapi::api::core::v1::ServicePort {
                        name: Some("https".to_string()),
                        port: 8443,
                        target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(8443)),
                        ..Default::default()
                    },
                ]),
                type_: Some("ClusterIP".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let params = PostParams::default();
        self.services.create(&params, &service).await?;
        info!("Created service for gateway: {}", gateway.name_any());
        Ok(())
    }

    async fn ensure_certificates_secret(&self) -> Result<(), ControllerError> {
        let secret_name = "gateway-certificates";
        
        // Check if secret already exists
        match self.secrets.get(secret_name).await {
            Ok(_) => {
                info!("Secret {} already exists", secret_name);
                return Ok(());
            }
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                // Secret doesn't exist, we need to create it
                // Note: In production, certificates should be generated or provided externally
                // For now, we'll create an empty secret and log a warning
                warn!("Secret {} not found. Certificates should be provided externally or generated.", secret_name);
                warn!("Creating empty secret. Gateway pods will fail until certificates are added.");
                
                let secret = Secret {
                    metadata: ObjectMeta {
                        name: Some(secret_name.to_string()),
                        namespace: Some("wasmbed".to_string()),
                        ..Default::default()
                    },
                    data: Some(std::collections::BTreeMap::new()),
                    ..Default::default()
                };
                
                let params = PostParams::default();
                match self.secrets.create(&params, &secret).await {
                    Ok(_) => {
                        info!("Created empty secret {}. Please add certificates manually.", secret_name);
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to create secret {}: {}", secret_name, e);
                        Err(ControllerError::KubeError(e))
                    }
                }
            }
            Err(e) => Err(ControllerError::KubeError(e)),
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
    let controller = GatewayController::new(client);
    
    info!("Starting Gateway Controller...");
    controller.run().await?;
    
    Ok(())
}