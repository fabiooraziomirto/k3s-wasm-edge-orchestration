// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::sync::Arc;
use std::time::SystemTime;
use tracing::{info, warn};

/// Heartbeat Manager for device health monitoring
pub struct HeartbeatManager {
    pub last_heartbeats: std::collections::HashMap<String, SystemTime>,
}

impl HeartbeatManager {
    pub fn new() -> Self {
        Self {
            last_heartbeats: std::collections::HashMap::new(),
        }
    }

    pub async fn run(self) {
        info!("Starting heartbeat manager");
        
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            
            let now = SystemTime::now();
            let timeout = std::time::Duration::from_secs(60);
            
            for (device_id, last_heartbeat) in &self.last_heartbeats {
                if now.duration_since(*last_heartbeat).unwrap_or(timeout) > timeout {
                    warn!("Device {} heartbeat timeout", device_id);
                }
            }
        }
    }

    pub async fn update_heartbeat(&mut self, device_id: String) {
        self.last_heartbeats.insert(device_id.clone(), SystemTime::now());
        info!("Updated heartbeat for device: {}", device_id);
    }
}
