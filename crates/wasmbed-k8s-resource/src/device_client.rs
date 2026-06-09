// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use chrono::{DateTime, Utc};
use kube::{Api, Error};
use kube::api::{ListParams, Patch, PatchParams};
use serde_json::json;

use crate::device::{Device, DevicePhase};
use wasmbed_types::{GatewayReference, PublicKey};

impl Device {
    pub async fn find(
        api: Api<Device>,
        public_key: PublicKey<'_>,
    ) -> Result<Option<Self>, Error> {
        // List all devices and find the one with matching public key
        // Field selectors don't work with custom spec fields in Kubernetes
        let devices = api.list(&ListParams::default()).await?;
        for device in devices {
            if device.spec.public_key == public_key.to_base64() {
                return Ok(Some(device));
            }
        }
        Ok(None)
    }
}

// This builder uses Option<Option<T>> to distinguish between "don't update"
// (None) and "set to None" (Some(None))
#[derive(Default)]
pub struct DeviceStatusUpdate {
    phase: Option<DevicePhase>,
    gateway: Option<Option<GatewayReference>>,
    connected_since: Option<Option<DateTime<Utc>>>,
    last_heartbeat: Option<Option<DateTime<Utc>>>,
}

impl DeviceStatusUpdate {
    pub fn phase(mut self, phase: DevicePhase) -> Self {
        self.phase = Some(phase);
        self
    }

    /// Validate state transition from current phase to new phase
    pub fn validate_transition(current_phase: DevicePhase, new_phase: DevicePhase) -> bool {
        match (current_phase, new_phase) {
            // Valid transitions
            (DevicePhase::Pending, DevicePhase::Enrolling) => true,
            (DevicePhase::Enrolling, DevicePhase::Enrolled) => true,
            (DevicePhase::Enrolled, DevicePhase::Connected) => true,
            (DevicePhase::Connected, DevicePhase::Disconnected) => true,
            (DevicePhase::Connected, DevicePhase::Unreachable) => true,
            (DevicePhase::Disconnected, DevicePhase::Connected) => true,
            (DevicePhase::Unreachable, DevicePhase::Connected) => true,
            (DevicePhase::Unreachable, DevicePhase::Disconnected) => true,
            
            // Self-transitions are valid
            (a, b) if a == b => true,
            
            // Invalid transitions
            _ => false,
        }
    }

    pub fn gateway(mut self, gateway: Option<GatewayReference>) -> Self {
        self.gateway = Some(gateway);
        self
    }

    pub fn connected_since(mut self, timestamp: Option<DateTime<Utc>>) -> Self {
        self.connected_since = Some(timestamp);
        self
    }

    pub fn last_heartbeat(mut self, timestamp: Option<DateTime<Utc>>) -> Self {
        self.last_heartbeat = Some(timestamp);
        self
    }

    pub fn mark_connected(self, gateway: GatewayReference) -> Self {
        self.phase(DevicePhase::Connected)
            .gateway(Some(gateway))
            .connected_since(Some(Utc::now()))
    }

    pub fn mark_enrolling(self) -> Self {
        self.phase(DevicePhase::Enrolling)
    }

    pub fn mark_enrolled(self) -> Self {
        self.phase(DevicePhase::Enrolled)
    }

    pub fn mark_disconnected(self) -> Self {
        self.phase(DevicePhase::Disconnected)
            .gateway(None)
            .connected_since(None)
    }

    pub fn mark_unreachable(self) -> Self {
        self.phase(DevicePhase::Unreachable)
            .gateway(None)
    }

    pub fn update_heartbeat(self) -> Self {
        self.last_heartbeat(Some(Utc::now()))
    }

    pub async fn apply(
        self,
        api: Api<Device>,
        device: Device,
    ) -> Result<Device, Error> {
        use serde_json::json;

        let name = device.metadata.name.as_ref().ok_or_else(|| {
            Error::Service(
                format!("Device {:?} has no name", device.spec.public_key)
                    .into(),
            )
        })?;

        let mut status_patch = json!({});

        if let Some(map) = status_patch.as_object_mut() {
            if let Some(phase) = self.phase {
                map.insert("phase".to_string(), json!(phase));
            }
            if let Some(gateway) = self.gateway {
                map.insert("gateway".to_string(), json!(gateway));
            }
            if let Some(connected_since) = self.connected_since {
                map.insert(
                    "connected_since".to_string(),
                    json!(connected_since),
                );
            }
            if let Some(last_heartbeat) = self.last_heartbeat {
                map.insert("last_heartbeat".to_string(), json!(last_heartbeat));
            }
        } else {
            return Err(Error::Service(
                "status_patch is not a JSON object".into(),
            ));
        }

        let patch = json!({
            "status": status_patch
        });

        api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
            .await
    }
}
