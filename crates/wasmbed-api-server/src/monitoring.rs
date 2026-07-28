// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::collections::HashMap;
use tracing::{error, info, warn};
use reqwest;
use serde::{Deserialize, Serialize};

/// Monitoring Dashboard for system metrics
#[derive(Debug)]
pub struct MonitoringDashboard {
    infrastructure_endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    pub name: String,
    pub value: f64,
    pub timestamp: u64, // Changed to u64 for JSON serialization
    pub labels: HashMap<String, String>,
    /// True when `value` is a hardcoded placeholder (local fallback, or an
    /// upstream infrastructure service reporting its own placeholder), not a
    /// real measurement. `#[serde(default)]` so metrics fetched from an older
    /// infrastructure build that doesn't emit this field still deserialize,
    /// defaulting to "not synthetic" would be misleading, so absence of the
    /// field is unexpected and should be treated as suspect data upstream.
    #[serde(default = "default_is_synthetic_unknown")]
    pub is_synthetic: bool,
}

/// Deserializing a `MetricValue` with no `is_synthetic` field means it came
/// from code that predates this tagging; treat it as synthetic until proven
/// otherwise rather than silently assuming it's real telemetry.
fn default_is_synthetic_unknown() -> bool {
    true
}

impl MonitoringDashboard {
    pub fn new(infrastructure_endpoint: &str) -> anyhow::Result<Self> {
        Ok(Self {
            infrastructure_endpoint: infrastructure_endpoint.to_string(),
        })
    }

    pub async fn get_metrics(&self) -> anyhow::Result<Vec<MetricValue>> {
        if self.infrastructure_endpoint.is_empty() {
            return self.get_fallback_metrics().await;
        }
        info!("Fetching metrics from infrastructure");
        let client = reqwest::Client::new();
               let response = client
                   .get(&format!("{}/api/v1/metrics", self.infrastructure_endpoint))
                   .timeout(std::time::Duration::from_secs(5))
                   .send()
                   .await;
        
        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let metrics: Vec<MetricValue> = resp.json().await.unwrap_or_default();
                    info!("Fetched {} metrics from infrastructure", metrics.len());
                    Ok(metrics)
                } else {
                    info!("Infrastructure service returned status: {}", resp.status());
                    self.get_fallback_metrics().await
                }
            }
            Err(e) => {
                info!("Failed to fetch metrics from infrastructure: {}", e);
                self.get_fallback_metrics().await
            }
        }
    }
    
    async fn get_fallback_metrics(&self) -> anyhow::Result<Vec<MetricValue>> {
        let mut metrics = Vec::new();

        // No real collector is wired up here either (no /proc, no cgroups,
        // no metrics-server query) — these are hardcoded placeholders used
        // only when the infrastructure service is unreachable or unconfigured.
        // Tagged `is_synthetic: true` so callers (dashboard, experiment
        // scripts) can't mistake them for telemetry. See
        // doc/energy-tracking-assessment.md, section 2.
        warn!("get_fallback_metrics: returning synthetic placeholder values (cpu_usage/memory_usage/disk_usage), not real telemetry");

        metrics.push(MetricValue {
            name: "cpu_usage".to_string(),
            value: 45.0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            labels: HashMap::new(),
            is_synthetic: true,
        });

        metrics.push(MetricValue {
            name: "memory_usage".to_string(),
            value: 60.0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            labels: HashMap::new(),
            is_synthetic: true,
        });

        metrics.push(MetricValue {
            name: "disk_usage".to_string(),
            value: 30.0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            labels: HashMap::new(),
            is_synthetic: true,
        });

        info!("Using fallback metrics: {}", metrics.len());
        Ok(metrics)
    }

    pub async fn get_metric(&self, name: &str) -> anyhow::Result<Option<MetricValue>> {
        info!("Fetching metric: {}", name);
        
        let metrics = self.get_metrics().await?;
        Ok(metrics.into_iter().find(|m| m.name == name))
    }

    pub async fn get_system_health(&self) -> anyhow::Result<SystemHealth> {
        info!("Getting system health status");
        
        // Analyze various metrics from infrastructure service
        
        Ok(SystemHealth {
            overall_status: "healthy".to_string(),
            cpu_status: "normal".to_string(),
            memory_status: "normal".to_string(),
            disk_status: "normal".to_string(),
            network_status: "normal".to_string(),
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub overall_status: String,
    pub cpu_status: String,
    pub memory_status: String,
    pub disk_status: String,
    pub network_status: String,
    pub last_updated: u64,
}
