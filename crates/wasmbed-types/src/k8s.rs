// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use alloc::borrow::ToOwned;
use alloc::string::String;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayReference {
    pub name: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(alias = "connectedAt", skip_serializing_if = "Option::is_none")]
    pub connected_at: Option<String>,
}

impl GatewayReference {
    pub fn new(_namespace: &str, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            endpoint: String::new(),
            connected_at: None,
        }
    }
}
