// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use chrono::Utc;
use kube::api::{Patch, PatchParams};
use kube::{Api, Error};
use serde_json::json;

use crate::application::{Application, ApplicationPhase, DeviceApplicationStatus};

#[cfg(feature = "client")]
impl Application {
    /// Find applications by device name
    pub async fn find_by_device(
        api: kube::Api<Self>,
        device_name: &str,
    ) -> Result<Vec<Self>, kube::Error> {
        let apps = api.list(&kube::api::ListParams::default()).await?;

        Ok(apps
            .into_iter()
            .filter(|app| app.targets_device(device_name))
            .collect())
    }
}

/// Builder for patching Application status (phase, per-device status, error).
/// Use [`ApplicationStatusUpdate::apply`] to perform the patch via the API server.
#[derive(Default)]
pub struct ApplicationStatusUpdate {
    phase: Option<ApplicationPhase>,
    error: Option<Option<String>>,
    device_status: Option<(String, DeviceApplicationStatus)>,
}

impl ApplicationStatusUpdate {
    pub fn phase(mut self, phase: ApplicationPhase) -> Self {
        self.phase = Some(phase);
        self
    }

    pub fn error(mut self, error: Option<String>) -> Self {
        self.error = Some(error);
        self
    }

    /// Set or update status for a single device (merged into status.deviceStatuses).
    pub fn device_status(mut self, device_id: String, status: DeviceApplicationStatus) -> Self {
        self.device_status = Some((device_id, status));
        self
    }

    /// Build status JSON for one device (camelCase for CRD).
    fn device_status_to_json(s: &DeviceApplicationStatus) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert("status".to_string(), json!(s.status));
        if let Some(ref h) = s.last_heartbeat {
            obj.insert("lastHeartbeat".to_string(), json!(h));
        }
        if let Some(ref m) = s.metrics {
            obj.insert("metrics".to_string(), json!(m));
        }
        if let Some(ref e) = s.error {
            obj.insert("error".to_string(), json!(e));
        }
        obj.insert("restartCount".to_string(), json!(s.restart_count));
        json!(obj)
    }

    /// Apply this update to the Application: merges with current status and patches the status subresource.
    pub async fn apply(
        self,
        api: &Api<Application>,
        app: &Application,
    ) -> Result<Application, Error> {
        let name = app
            .metadata
            .name
            .as_ref()
            .ok_or_else(|| Error::Service("Application has no name".into()))?;

        let current = app.status.as_ref();
        let mut phase = current
            .and_then(|s| Some(s.phase))
            .unwrap_or(ApplicationPhase::Creating);
        let mut device_statuses: std::collections::BTreeMap<String, DeviceApplicationStatus> =
            current
                .and_then(|s| s.device_statuses.clone())
                .unwrap_or_default();
        let mut error = current.and_then(|s| s.error.clone());

        if let Some(p) = self.phase {
            phase = p;
        }
        if let Some(e) = self.error {
            error = e;
        }
        if let Some((device_id, dev_status)) = self.device_status {
            device_statuses.insert(device_id, dev_status);
        }

        let last_updated = Utc::now().to_rfc3339();
        let device_statuses_json: serde_json::Map<String, serde_json::Value> = device_statuses
            .iter()
            .map(|(k, v)| (k.clone(), Self::device_status_to_json(v)))
            .collect();

        let status_patch = json!({
            "phase": phase,
            "deviceStatuses": device_statuses_json,
            "lastUpdated": last_updated,
            "error": error
        });

        let patch = json!({ "status": status_patch });
        api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
            .await
    }
}
