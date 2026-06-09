// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

//! Deployment state and status for WASM applications.
//!
//! Actual deploy/stop is performed by `HttpApiServer::deploy_application_to_device` and
//! `HttpApiServer::stop_application_on_device`, which send `ServerMessage::DeployApplication` and
//! `ServerMessage::StopApplication` over the TLS/CBOR connection to the device. Reported state
//! is tracked in `HttpApiServer::applications` (and optionally in Application CRD status).

use std::collections::HashMap;
use std::time::SystemTime;
use tracing::info;

/// Deployment status for a single app on a single device (reported state).
#[derive(Debug, Clone, PartialEq)]
pub enum DeploymentStatus {
    Pending,
    Deploying,
    Running,
    Failed,
    Stopped,
}

/// Record of a deployment (app_id + device_id) for querying status.
#[derive(Debug, Clone)]
pub struct DeploymentInfo {
    pub app_id: String,
    pub device_id: String,
    pub status: DeploymentStatus,
    pub deployed_at: SystemTime,
    pub error: Option<String>,
}

/// Deployment state tracker (optional; primary state lives in HttpApiServer::applications).
pub struct DeploymentService {
    pub deployments: HashMap<String, DeploymentInfo>,
}

impl DeploymentService {
    pub fn new() -> Self {
        Self {
            deployments: HashMap::new(),
        }
    }

    /// Record that a deployment was requested (status = Deploying). The actual send is done by
    /// HttpApiServer when handling POST /api/v1/devices/:device_id/deploy.
    pub fn record_deploy_started(&mut self, app_id: &str, device_id: &str) {
        let key = format!("{}-{}", app_id, device_id);
        self.deployments.insert(
            key,
            DeploymentInfo {
                app_id: app_id.to_string(),
                device_id: device_id.to_string(),
                status: DeploymentStatus::Deploying,
                deployed_at: SystemTime::now(),
                error: None,
            },
        );
        info!("Deployment started: app={} device={}", app_id, device_id);
    }

    /// Record deployment outcome (Running or Failed).
    pub fn record_deploy_outcome(
        &mut self,
        app_id: &str,
        device_id: &str,
        status: DeploymentStatus,
        error: Option<String>,
    ) {
        let key = format!("{}-{}", app_id, device_id);
        if let Some(d) = self.deployments.get_mut(&key) {
            d.status = status.clone();
            d.error = error;
        } else {
            self.deployments.insert(
                key,
                DeploymentInfo {
                    app_id: app_id.to_string(),
                    device_id: device_id.to_string(),
                    status,
                    deployed_at: SystemTime::now(),
                    error: None,
                },
            );
        }
    }

    pub fn get_deployment_status(&self, app_id: &str, device_id: &str) -> Option<DeploymentStatus> {
        self.deployments
            .get(&format!("{}-{}", app_id, device_id))
            .map(|d| d.status.clone())
    }
}
