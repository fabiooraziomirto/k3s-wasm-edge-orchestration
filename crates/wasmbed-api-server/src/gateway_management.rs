// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use tracing::{error, info, warn};

use crate::GatewayInfo;

/// Gateway Manager for gateway operations
#[derive(Debug)]
pub struct GatewayManager {
    gateway_endpoint: String,
}

impl GatewayManager {
    pub fn new(gateway_endpoint: &str) -> anyhow::Result<Self> {
        Ok(Self {
            gateway_endpoint: gateway_endpoint.to_string(),
        })
    }

    pub async fn get_all_gateways(&self) -> anyhow::Result<Vec<GatewayInfo>> {
        info!("Fetching all gateways from Kubernetes");
        
        // Query Kubernetes for Gateway CRDs
        let output = tokio::process::Command::new("kubectl")
            .args(&["get", "gateways.wasmbed.io", "-n", "wasmbed", "-o", "json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No resources found") {
                info!("No gateways found in wasmbed namespace");
                return Ok(vec![]);
            }
            return Err(anyhow::anyhow!("kubectl failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let k8s_response: serde_json::Value = serde_json::from_str(&stdout)?;
        
        let mut gateways = Vec::new();
        
        if let Some(items) = k8s_response["items"].as_array() {
            for item in items {
                if let Some(metadata) = item["metadata"].as_object() {
                    let gateway_id = metadata["name"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                    
                    let status = if item["status"].is_object() && !item["status"].is_null() {
                        item["status"]["phase"]
                            .as_str()
                            .unwrap_or("Pending")
                            .to_string()
                    } else {
                        // If no status field or status is null, return Pending
                        "Pending".to_string()
                    };
                    
                    let endpoint = item["spec"]["endpoint"]
                        .as_str()
                        .unwrap_or("wasmbed-gateway.wasmbed.svc.cluster.local:8080")
                        .to_string();
                    
                    let connected_devices = item["status"]["connectedDevices"]
                        .as_u64()
                        .unwrap_or(0) as u32;
                    
                    let enrolled_devices = item["status"]["enrolledDevices"]
                        .as_u64()
                        .unwrap_or(0) as u32;
                    
                    gateways.push(GatewayInfo {
                        gateway_id,
                        endpoint,
                        status,
                        connected_devices,
                        enrolled_devices,
                    });
                }
            }
        }
        
        info!("Fetched {} gateways from Kubernetes", gateways.len());
        Ok(gateways)
    }

    pub async fn get_gateway(&self, gateway_id: &str) -> anyhow::Result<Option<GatewayInfo>> {
        info!("Fetching gateway: {}", gateway_id);
        
        let gateways = self.get_all_gateways().await?;
        Ok(gateways.into_iter().find(|g| g.gateway_id == gateway_id))
    }

    pub async fn get_gateway_status(&self, gateway_id: &str) -> anyhow::Result<String> {
        info!("Getting gateway status: {}", gateway_id);
        
        // In a real implementation, this would call the gateway API
        // For now, we'll simulate status check
        
        Ok("active".to_string())
    }
}
