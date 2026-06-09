// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::collections::HashMap;
use tracing::info;

/// Local Cache for gateway data storage
pub struct LocalCache {
    pub cache: HashMap<String, String>,
}

impl LocalCache {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            cache: HashMap::new(),
        })
    }

    pub async fn set(&mut self, key: String, value: String) {
        self.cache.insert(key.clone(), value);
        info!("Cached key: {}", key);
    }

    pub async fn get(&self, key: &str) -> Option<&String> {
        self.cache.get(key)
    }

    pub async fn remove(&mut self, key: &str) -> Option<String> {
        self.cache.remove(key)
    }
}
