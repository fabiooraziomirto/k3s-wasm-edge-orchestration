// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use kube::CustomResource;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

/// Gateway CRD definition
#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[kube(
    group = "wasmbed.io",
    version = "v1",
    kind = "Gateway",
    plural = "gateways",
    shortname = "gw",
    namespaced,
    status = "GatewayStatus"
)]
#[kube(derive = "PartialEq")]
pub struct GatewaySpec {
    pub endpoint: String,
    pub capabilities: Option<Vec<String>>,
    pub config: Option<GatewayConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct GatewayConfig {
    pub connection_timeout: Option<String>,
    pub enrollment_timeout: Option<String>,
    pub heartbeat_interval: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub phase: GatewayPhase,
    pub connected_devices: Option<i32>,
    pub enrolled_devices: Option<i32>,
    pub last_heartbeat: Option<String>,
    pub conditions: Option<Vec<GatewayCondition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum GatewayPhase {
    #[serde(rename = "Pending")]
    Pending,
    #[serde(rename = "Running")]
    Running,
    #[serde(rename = "Failed")]
    Failed,
    #[serde(rename = "Stopped")]
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCondition {
    #[serde(rename = "type")]
    pub r#type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition_time: Option<String>,
}
