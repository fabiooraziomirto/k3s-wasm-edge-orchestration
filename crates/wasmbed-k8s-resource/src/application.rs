// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Application CRD for managing WASM applications on devices
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "wasmbed.github.io",
    version = "v1alpha1",
    kind = "Application",
    namespaced,
    status = "ApplicationStatus"
)]
pub struct ApplicationSpec {
    /// Application name
    pub name: String,
    
    /// Application description
    #[serde(default)]
    pub description: Option<String>,
    
    /// WASM bytecode (base64 encoded)
    #[serde(rename = "wasmBytes")]
    pub wasm_bytes: String,
    
    /// Target devices (device names or selectors)
    #[serde(rename = "targetDevices")]
    pub target_devices: TargetDevices,
    
    /// Application configuration
    #[serde(default)]
    pub config: Option<ApplicationConfig>,
    
    /// Application metadata
    #[serde(default)]
    pub metadata: Option<ApplicationMetadata>,

    /// Target runtime for WASM execution on the device.
    /// Defaults to WamrRaw (current behaviour) when absent.
    #[serde(default, rename = "targetRuntime")]
    pub target_runtime: Option<TargetRuntime>,
}

/// Target devices specification
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TargetDevices {
    /// Device names (exact match)
    #[serde(default, rename = "deviceNames")]
    pub device_names: Option<Vec<String>>,
    
    /// Device selectors (label-based)
    #[serde(default)]
    pub selectors: Option<DeviceSelectors>,
    
    /// All devices in namespace
    #[serde(default, rename = "allDevices")]
    pub all_devices: Option<bool>,
}

/// Device selectors for targeting devices
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeviceSelectors {
    /// Match devices by labels
    #[serde(default)]
    pub match_labels: Option<std::collections::BTreeMap<String, String>>,
    
    /// Match devices by expressions
    #[serde(default)]
    pub match_expressions: Option<Vec<DeviceSelectorRequirement>>,
}

/// Device selector requirement
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeviceSelectorRequirement {
    /// Key to match
    pub key: String,
    
    /// Operator (In, NotIn, Exists, DoesNotExist)
    pub operator: String,
    
    /// Values to match
    #[serde(default)]
    pub values: Option<Vec<String>>,
}

/// Application configuration
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ApplicationConfig {
    /// Memory limit in bytes
    #[serde(default = "default_memory_limit")]
    pub memory_limit: u64,
    
    /// CPU time limit in milliseconds
    #[serde(default = "default_cpu_time_limit")]
    pub cpu_time_limit: u64,
    
    /// Environment variables
    #[serde(default)]
    pub env_vars: Option<std::collections::BTreeMap<String, String>>,
    
    /// Startup arguments
    #[serde(default)]
    pub args: Option<Vec<String>>,
    
    /// Auto-restart on failure
    #[serde(default = "default_auto_restart")]
    pub auto_restart: bool,
    
    /// Maximum restart attempts
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
}

/// Application metadata
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ApplicationMetadata {
    /// Application version
    #[serde(default)]
    pub version: Option<String>,
    
    /// Application author
    #[serde(default)]
    pub author: Option<String>,
    
    /// Application license
    #[serde(default)]
    pub license: Option<String>,
    
    /// Application tags
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Application status
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationStatus {
    /// Application phase
    pub phase: ApplicationPhase,
    
    /// Deployment status per device
    #[serde(default)]
    pub device_statuses: Option<std::collections::BTreeMap<String, DeviceApplicationStatus>>,
    
    /// Overall deployment statistics
    #[serde(default)]
    pub statistics: Option<ApplicationStatistics>,
    
    /// Last update timestamp
    #[serde(default)]
    pub last_updated: Option<String>,
    
    /// Error message if any
    #[serde(default)]
    pub error: Option<String>,
}

/// Runtime used to execute WASM on the target device
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum TargetRuntime {
    /// WAMR interpreter/AOT — default, current behaviour
    WamrRaw,
    /// OCRE container runtime on Zephyr (MCU constrained)
    OcreZephyr,
    /// OCRE container runtime on Linux (MPU rich)
    OcreLinux,
}

impl Default for TargetRuntime {
    fn default() -> Self {
        TargetRuntime::WamrRaw
    }
}

/// Application phase
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum ApplicationPhase {
    /// Application is being created
    Creating,
    /// Application is being deployed to devices
    Deploying,
    /// Application is running on all target devices
    Running,
    /// Application is partially running
    PartiallyRunning,
    /// Application deployment failed
    Failed,
    /// Application is being stopped
    Stopping,
    /// Application is stopped
    Stopped,
    /// Application is being deleted
    Deleting,
}

impl ApplicationPhase {
    /// Validate state transition from current phase to new phase
    pub fn validate_transition(current_phase: ApplicationPhase, new_phase: ApplicationPhase) -> bool {
        match (current_phase, new_phase) {
            // Valid transitions
            (ApplicationPhase::Creating, ApplicationPhase::Deploying) => true,
            (ApplicationPhase::Deploying, ApplicationPhase::Running) => true,
            (ApplicationPhase::Deploying, ApplicationPhase::PartiallyRunning) => true,
            (ApplicationPhase::Deploying, ApplicationPhase::Failed) => true,
            (ApplicationPhase::PartiallyRunning, ApplicationPhase::Running) => true,
            (ApplicationPhase::PartiallyRunning, ApplicationPhase::Failed) => true,
            (ApplicationPhase::Running, ApplicationPhase::Stopping) => true,
            (ApplicationPhase::Running, ApplicationPhase::PartiallyRunning) => true,
            (ApplicationPhase::Running, ApplicationPhase::Failed) => true,
            (ApplicationPhase::Stopping, ApplicationPhase::Stopped) => true,
            (ApplicationPhase::Stopping, ApplicationPhase::Failed) => true,
            (ApplicationPhase::Stopped, ApplicationPhase::Deploying) => true,
            (ApplicationPhase::Failed, ApplicationPhase::Deploying) => true,
            (ApplicationPhase::Failed, ApplicationPhase::Deleting) => true,
            (ApplicationPhase::Stopped, ApplicationPhase::Deleting) => true,
            (ApplicationPhase::Running, ApplicationPhase::Deleting) => true,
            (ApplicationPhase::PartiallyRunning, ApplicationPhase::Deleting) => true,
            
            // Self-transitions are valid
            (a, b) if a == b => true,
            
            // Invalid transitions
            _ => false,
        }
    }
    
    /// Get the default phase for new applications
    pub fn default() -> Self {
        ApplicationPhase::Creating
    }
}

/// Device application status
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeviceApplicationStatus {
    /// Status on this device
    pub status: DeviceApplicationPhase,
    
    /// Last heartbeat timestamp
    #[serde(default)]
    pub last_heartbeat: Option<String>,
    
    /// Application metrics
    #[serde(default)]
    pub metrics: Option<ApplicationMetrics>,
    
    /// Error message if any
    #[serde(default)]
    pub error: Option<String>,
    
    /// Restart count
    #[serde(default)]
    pub restart_count: u32,
}

/// Device application phase
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum DeviceApplicationPhase {
    /// Application is being deployed
    Deploying,
    /// Application is running
    Running,
    /// Application has failed
    Failed,
    /// Application is stopped
    Stopped,
}

impl DeviceApplicationPhase {
    /// Validate state transition from current phase to new phase
    pub fn validate_transition(current_phase: DeviceApplicationPhase, new_phase: DeviceApplicationPhase) -> bool {
        match (current_phase, new_phase) {
            // Valid transitions
            (DeviceApplicationPhase::Deploying, DeviceApplicationPhase::Running) => true,
            (DeviceApplicationPhase::Deploying, DeviceApplicationPhase::Failed) => true,
            (DeviceApplicationPhase::Running, DeviceApplicationPhase::Stopped) => true,
            (DeviceApplicationPhase::Running, DeviceApplicationPhase::Failed) => true,
            (DeviceApplicationPhase::Stopped, DeviceApplicationPhase::Deploying) => true,
            (DeviceApplicationPhase::Failed, DeviceApplicationPhase::Deploying) => true,
            
            // Self-transitions are valid
            (a, b) if a == b => true,
            
            // Invalid transitions
            _ => false,
        }
    }
    
    /// Get the default phase for new device applications
    pub fn default() -> Self {
        DeviceApplicationPhase::Deploying
    }
}

/// Application metrics
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ApplicationMetrics {
    /// Memory usage in bytes
    #[serde(default)]
    pub memory_usage: Option<u64>,
    
    /// CPU usage percentage
    #[serde(default)]
    pub cpu_usage: Option<f64>,
    
    /// Uptime in seconds
    #[serde(default)]
    pub uptime: Option<u64>,
    
    /// Number of function calls
    #[serde(default)]
    pub function_calls: Option<u64>,
}

/// Application statistics
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct ApplicationStatistics {
    /// Total number of target devices
    #[serde(default)]
    pub total_devices: u32,
    
    /// Number of devices where application is deployed
    #[serde(default)]
    pub deployed_devices: u32,
    
    /// Number of devices where application is running
    #[serde(default)]
    pub running_devices: u32,
    
    /// Number of devices where application failed
    #[serde(default)]
    pub failed_devices: u32,
    
    /// Number of devices where application is stopped
    #[serde(default)]
    pub stopped_devices: u32,
}

// Default values
fn default_memory_limit() -> u64 {
    1024 * 1024 // 1MB
}

fn default_cpu_time_limit() -> u64 {
    1000 // 1 second
}

fn default_auto_restart() -> bool {
    true
}

fn default_max_restarts() -> u32 {
    3
}

// Add status field to Application
impl Application {
    pub fn status(&self) -> Option<&ApplicationStatus> {
        // This is a placeholder - in a real implementation, this would access the status field
        // For now, we'll return None since the status is managed by the controller
        None
    }

    /// Check if application targets a specific device
    pub fn targets_device(&self, device_name: &str) -> bool {
        if let Some(device_names) = &self.spec.target_devices.device_names {
            device_names.contains(&device_name.to_string())
        } else if self.spec.target_devices.all_devices.unwrap_or(false) {
            true
        } else {
            false
        }
    }
}
