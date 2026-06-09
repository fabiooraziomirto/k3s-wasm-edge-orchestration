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
        
        // Collect CPU usage
        metrics.insert("cpu_usage".to_string(), MetricValue {
            name: "cpu_usage".to_string(),
            value: 45.0, // Simulate 45% CPU usage
            timestamp: SystemTime::now(),
            labels: HashMap::new(),
        });
        
        // Collect memory usage
        metrics.insert("memory_usage".to_string(), MetricValue {
            name: "memory_usage".to_string(),
            value: 60.0, // Simulate 60% memory usage
            timestamp: SystemTime::now(),
            labels: HashMap::new(),
        });
        
        // Collect disk usage
        metrics.insert("disk_usage".to_string(), MetricValue {
            name: "disk_usage".to_string(),
            value: 30.0, // Simulate 30% disk usage
            timestamp: SystemTime::now(),
            labels: HashMap::new(),
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
    
    pub async fn get_system_metrics(&self) -> HashMap<String, f64> {
        let metrics = self.metrics.read().await;
        metrics.iter().map(|(name, metric)| (name.clone(), metric.value)).collect()
    }
}
