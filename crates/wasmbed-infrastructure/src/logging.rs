// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Logging Service for centralized logging
#[derive(Debug, Clone)]
pub struct LoggingService {
    logs: Arc<RwLock<Vec<LogEntry>>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub component: String,
    pub message: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LoggingService {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn run(&self) {
        info!("Starting Logging Service...");
        
        loop {
            // Process log entries
            self.process_logs().await;
            
            // Wait before next processing
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    async fn process_logs(&self) {
        let logs = self.logs.read().await;
        
        // In a real implementation, this would:
        // 1. Send logs to external logging system (ELK, Fluentd, etc.)
        // 2. Apply log filtering and routing
        // 3. Handle log retention policies
        
        info!("Processed {} log entries", logs.len());
    }

    pub async fn log(&self, level: LogLevel, component: &str, message: &str, metadata: HashMap<String, String>) {
        let entry = LogEntry {
            timestamp: SystemTime::now(),
            level,
            component: component.to_string(),
            message: message.to_string(),
            metadata,
        };

        let mut logs = self.logs.write().await;
        logs.push(entry);
        
        // Keep only last 1000 entries to prevent memory issues
        if logs.len() > 1000 {
            let len = logs.len();
            logs.drain(0..len - 1000);
        }
    }

    pub async fn get_logs(&self, limit: Option<usize>) -> Vec<LogEntry> {
        let logs = self.logs.read().await;
        let limit = limit.unwrap_or(100);
        logs.iter().rev().take(limit).cloned().collect()
    }

    pub async fn get_logs_by_level(&self, level: LogLevel, limit: Option<usize>) -> Vec<LogEntry> {
        let logs = self.logs.read().await;
        let limit = limit.unwrap_or(100);
        logs.iter()
            .rev()
            .filter(|entry| entry.level == level)
            .take(limit)
            .cloned()
            .collect()
    }
}
