// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::time::SystemTime;
use tracing::{info, warn};

/// Enrollment Service for device registration
pub struct EnrollmentService {
    pub pairing_enabled: bool,
}

impl EnrollmentService {
    pub fn new() -> Self {
        Self {
            pairing_enabled: false,
        }
    }

    pub async fn enable_pairing(&mut self) {
        self.pairing_enabled = true;
        info!("Pairing mode enabled");
    }

    pub async fn disable_pairing(&mut self) {
        self.pairing_enabled = false;
        info!("Pairing mode disabled");
    }

    pub async fn enroll_device(&self, device_id: String, public_key: String) -> Result<String, String> {
        if !self.pairing_enabled {
            return Err("Pairing mode is disabled".to_string());
        }

        info!("Enrolling device: {}", device_id);
        
        // Simulate enrollment process
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        
        Ok(format!("device-uuid-{}", device_id))
    }
}
