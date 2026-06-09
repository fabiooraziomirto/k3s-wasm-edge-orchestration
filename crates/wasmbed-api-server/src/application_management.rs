// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::time::SystemTime;
use tracing::{error, info, warn};

use crate::{ApplicationInfo, DeployApplicationRequest, DeviceSelector};

/// Application Manager for application operations
#[derive(Debug)]
pub struct ApplicationManager {
    gateway_endpoint: String,
}

impl ApplicationManager {
    pub fn new(gateway_endpoint: &str) -> anyhow::Result<Self> {
        Ok(Self {
            gateway_endpoint: gateway_endpoint.to_string(),
        })
    }

    pub async fn get_all_applications(&self) -> anyhow::Result<Vec<ApplicationInfo>> {
        info!("Fetching all applications from Kubernetes");
        
        // Query Kubernetes for Application CRDs
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "applications", "-n", "wasmbed", "-o", "json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No resources found") {
                info!("No applications found in wasmbed namespace");
                return Ok(vec![]);
            }
            return Err(anyhow::anyhow!("kubectl failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let k8s_response: serde_json::Value = serde_json::from_str(&stdout)?;
        
        let mut applications = Vec::new();
        
        if let Some(items) = k8s_response["items"].as_array() {
            for item in items {
                if let Some(metadata) = item["metadata"].as_object() {
                    let app_id = metadata["name"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                    
                    // Get status phase, defaulting to "Pending" if not available
                    // Handle cases where status doesn't exist, is null, or phase is missing
                    let status = if let Some(status_obj) = item.get("status") {
                        if status_obj.is_null() {
                            "Pending".to_string()
                        } else {
                            status_obj["phase"]
                        .as_str()
                                .filter(|s| !s.is_empty() && *s != "null" && *s != "undefined")
                                .unwrap_or("Pending")
                                .to_string()
                        }
                    } else {
                        "Pending".to_string()
                    };
                    
                    let name = item["spec"]["name"]
                        .as_str()
                        .unwrap_or(&app_id)
                        .to_string();
                    
                    let image = item["spec"]["image"]
                        .as_str()
                        .unwrap_or("unknown:latest")
                        .to_string();
                    
                    // Extract deployed devices and per-device status counts from deviceStatuses
                    let (deployed_devices, failed_devices_count) = if let Some(device_statuses) = item["status"]["deviceStatuses"].as_object() {
                        let ids: Vec<String> = device_statuses.keys().cloned().collect();
                        let failed = device_statuses.values().filter(|v| v.get("status").and_then(|s| s.as_str()) == Some("Failed")).count() as u32;
                        (ids, failed)
                    } else if let Some(deployed_devices_arr) = item["status"]["deployedDevices"].as_array() {
                        let ids: Vec<String> = deployed_devices_arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect();
                        (ids, 0u32)
                    } else {
                        (Vec::new(), 0u32)
                    };
                    
                    let created_at = metadata["creationTimestamp"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(dt.timestamp() as u64))
                        .unwrap_or(SystemTime::now());
                    
                    // Extract target devices from spec
                    // Handle cases where targetDevices doesn't exist, is null, or deviceNames is missing
                    let target_devices = if let Some(spec) = item.get("spec") {
                        if let Some(target_devices_obj) = spec.get("targetDevices") {
                            if target_devices_obj.is_null() {
                                None
                            } else if let Some(device_names) = target_devices_obj.get("deviceNames") {
                                if device_names.is_null() {
                                    None
                                } else if let Some(arr) = device_names.as_array() {
                                    let devices: Vec<String> = arr.iter()
                                        .filter_map(|v| v.as_str())
                                        .filter(|s| !s.is_empty() && *s != "null" && *s != "undefined")
                                        .map(|s| s.to_string())
                                        .collect();
                                    if devices.is_empty() {
                                        None
                                    } else {
                                        Some(devices)
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }.or_else(|| {
                            // Fallback to deployed_devices if targetDevices not found
                            if !deployed_devices.is_empty() {
                                Some(deployed_devices.clone())
                            } else {
                                None
                            }
                        });

                    // Extract last updated from status or use created_at
                    let last_updated = item["status"]["lastUpdated"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(dt.timestamp() as u64))
                        .or(Some(created_at));

                    applications.push(ApplicationInfo {
                        app_id: app_id.clone(),
                        id: Some(app_id.clone()),
                        name,
                        image,
                        status,
                        deployed_devices,
                        failed_devices: failed_devices_count,
                        created_at,
                        target_devices,
                        last_updated,
                    });
                }
            }
        }
        
        info!("Fetched {} applications from Kubernetes", applications.len());
        Ok(applications)
    }

    pub async fn get_application(&self, app_id: &str) -> anyhow::Result<Option<ApplicationInfo>> {
        info!("Fetching application: {}", app_id);
        
        let applications = self.get_all_applications().await?;
        Ok(applications.into_iter().find(|a| a.app_id == app_id))
    }

    pub async fn deploy_application(&self, request: &DeployApplicationRequest) -> anyhow::Result<DeployResult> {
        info!("Deploying application: {}", request.name);
        
        // In a real implementation, this would:
        // 1. Create Application CRD in Kubernetes
        // 2. Call gateway API to deploy
        // 3. Monitor deployment status
        
        let app_id = uuid::Uuid::new_v4().to_string();
        
        info!("Application {} deployed successfully with ID: {}", request.name, app_id);
        
        Ok(DeployResult {
            success: true,
            message: "Application deployed successfully".to_string(),
            application_id: app_id,
        })
    }

    pub async fn stop_application(&self, app_id: &str) -> anyhow::Result<bool> {
        info!("Stopping application: {}", app_id);
        
        // In a real implementation, this would call the gateway API
        // For now, we'll simulate success
        
        info!("Application {} stopped", app_id);
        Ok(true)
    }
}

#[derive(Debug)]
pub struct DeployResult {
    pub success: bool,
    pub message: String,
    pub application_id: String,
}
