// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Monitoring Service for system metrics
#[derive(Debug, Clone)]
pub struct MonitoringService {
    metrics: Arc<RwLock<HashMap<String, MetricValue>>>,
}

#[derive(Debug, Clone)]
pub struct MetricValue {
    pub name: String,
    pub value: f64,
    pub timestamp: SystemTime,
    pub labels: HashMap<String, String>,
    /// True when `value` is a hardcoded placeholder, not a real measurement.
    /// This service does not yet read `/proc`, cgroups, or any real collector;
    /// see doc/energy-tracking-assessment.md. Never treat a synthetic value
    /// as telemetry in experiment data.
    pub is_synthetic: bool,
}

impl MonitoringService {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(&self) {
        info!("Starting Monitoring Service...");
        
        loop {
            // Collect system metrics
            self.collect_metrics().await;
            
            // Wait before next collection
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }

    async fn collect_metrics(&self) {
        let mut metrics = self.metrics.write().await;

        // NOTE: no real collector is wired up yet (no /proc, no cgroups, no
        // metrics-server query). These are hardcoded placeholders so the
        // monitoring endpoints have something to return during development.
        // They are marked `is_synthetic: true` so nothing downstream (dashboards,
        // experiment scripts) can mistake them for telemetry. See
        // doc/energy-tracking-assessment.md, section 2.
        warn!("collect_metrics: emitting synthetic placeholder values (cpu_usage/memory_usage/disk_usage), not real telemetry");

        // Collect CPU usage
        metrics.insert("cpu_usage".to_string(), MetricValue {
            name: "cpu_usage".to_string(),
            value: 45.0, // Placeholder, not measured
            timestamp: SystemTime::now(),
            labels: HashMap::new(),
            is_synthetic: true,
        });

        // Collect memory usage
        metrics.insert("memory_usage".to_string(), MetricValue {
            name: "memory_usage".to_string(),
            value: 60.0, // Placeholder, not measured
            timestamp: SystemTime::now(),
            labels: HashMap::new(),
            is_synthetic: true,
        });

        // Collect disk usage
        metrics.insert("disk_usage".to_string(), MetricValue {
            name: "disk_usage".to_string(),
            value: 30.0, // Placeholder, not measured
            timestamp: SystemTime::now(),
            labels: HashMap::new(),
            is_synthetic: true,
        });

        info!("Metrics collected successfully");
    }

    pub async fn get_metrics(&self) -> HashMap<String, MetricValue> {
        let metrics = self.metrics.read().await;
        metrics.clone()
    }

    pub async fn get_metric(&self, name: &str) -> Option<MetricValue> {
        let metrics = self.metrics.read().await;
        metrics.get(name).cloned()
    }
}
