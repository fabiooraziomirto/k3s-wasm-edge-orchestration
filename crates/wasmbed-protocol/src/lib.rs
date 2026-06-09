// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

mod cbor;

use minicbor::{Decode, Encode};

/// A protocol message wrapper that provides versioning and correlation tracking.
#[derive(Debug, Clone, PartialEq, Decode, Encode)]
pub struct Envelope<T> {
    /// Protocol version for backward compatibility
    #[cbor(n(0))]
    pub version: Version,
    /// Unique identifier for request/response correlation
    #[cbor(n(1))]
    pub message_id: MessageId,
    /// The actual message payload
    #[cbor(n(2))]
    pub message: T,
}

/// Type alias for envelopes containing client-originated messages
pub type ClientEnvelope = Envelope<ClientMessage>;

/// Type alias for envelopes containing server-originated messages
pub type ServerEnvelope = Envelope<ServerMessage>;

/// Protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Decode, Encode)]
#[cbor(index_only)]
pub enum Version {
    #[cbor(n(0))]
    V0,
}

/// Unique identifier for correlating requests with responses
#[derive(Debug, Default, Clone, Copy, PartialEq, Decode, Encode)]
#[cbor(transparent)]
pub struct MessageId(u32);

impl MessageId {
    pub fn next(&self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Device UUID type compatible with no_std
#[derive(Debug, Clone, PartialEq, Decode, Encode)]
pub struct DeviceUuid {
    /// UUID as bytes (16 bytes for UUID v4)
    #[cbor(n(0))]
    pub bytes: [u8; 16],
}

impl DeviceUuid {
    /// Create a new DeviceUuid from bytes
    pub fn new(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }
    
    /// Convert to string representation (requires alloc) - use Display trait instead
    #[cfg(feature = "alloc")]
    pub fn to_string_alt(&self) -> alloc::string::String {
        use alloc::format;
        let s = format!("{:02x}{:02x}{:02x}{:02x}", self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]);
        let s = format!("{}-{:02x}{:02x}", s, self.bytes[4], self.bytes[5]);
        let s = format!("{}-{:02x}{:02x}", s, self.bytes[6], self.bytes[7]);
        let s = format!("{}-{:02x}{:02x}", s, self.bytes[8], self.bytes[9]);
        format!("{}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", s, self.bytes[10], self.bytes[11], self.bytes[12], self.bytes[13], self.bytes[14], self.bytes[15])
    }
    
}

/// Display trait implementation for DeviceUuid
#[cfg(feature = "alloc")]
impl core::fmt::Display for DeviceUuid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use alloc::format;
        let s = format!("{:02x}{:02x}{:02x}{:02x}", self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]);
        let s = format!("{}-{:02x}{:02x}", s, self.bytes[4], self.bytes[5]);
        let s = format!("{}-{:02x}{:02x}", s, self.bytes[6], self.bytes[7]);
        let s = format!("{}-{:02x}{:02x}", s, self.bytes[8], self.bytes[9]);
        let s = format!("{}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", s, self.bytes[10], self.bytes[11], self.bytes[12], self.bytes[13], self.bytes[14], self.bytes[15]);
        write!(f, "{}", s)
    }
}

/// Messages sent from client to server
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    /// Periodic heartbeat to maintain connection liveness
    Heartbeat,
    
    /// Device enrollment request (sent with enrollment flag)
    EnrollmentRequest,
    
    /// Public key sent during enrollment
    PublicKey {
        /// Public key in DER format
        key: alloc::vec::Vec<u8>,
    },
    
    /// Acknowledgment of enrollment completion
    EnrollmentAcknowledgment,
    
    /// Application deployment status update
    ApplicationStatus {
        /// Application ID
        app_id: alloc::string::String,
        /// Current status
        status: ApplicationStatus,
        /// Optional error message
        error: Option<alloc::string::String>,
        /// Application metrics (memory usage, etc.)
        metrics: Option<ApplicationMetrics>,
    },
    
    /// Application deployment acknowledgment
    ApplicationDeployAck {
        /// Application ID
        app_id: alloc::string::String,
        /// Success status
        success: bool,
        /// Error message if failed
        error: Option<alloc::string::String>,
    },
    
    /// Application stop acknowledgment
    ApplicationStopAck {
        /// Application ID
        app_id: alloc::string::String,
        /// Success status
        success: bool,
        /// Error message if failed
        error: Option<alloc::string::String>,
    },
    
    /// Device capabilities and resources
    DeviceInfo {
        /// Available memory in bytes
        available_memory: u64,
        /// CPU architecture
        cpu_arch: alloc::string::String,
        /// Supported WASM features
        wasm_features: alloc::vec::Vec<alloc::string::String>,
        /// Maximum application size
        max_app_size: u64,
    },
}

/// Messages sent from server to client
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    /// Acknowledgment of a client heartbeat
    HeartbeatAck,
    
    /// Enrollment request accepted
    EnrollmentAccepted,
    
    /// Enrollment request rejected (pairing mode disabled)
    EnrollmentRejected {
        /// Reason for rejection
        reason: alloc::vec::Vec<u8>,
    },
    
    /// Device UUID assigned during enrollment
    DeviceUuid {
        /// Unique device identifier
        uuid: DeviceUuid,
    },
    
    /// Enrollment completed successfully
    EnrollmentCompleted,
    
    /// Deploy application to device
    DeployApplication {
        /// Application ID
        app_id: alloc::string::String,
        /// Application name
        name: alloc::string::String,
        /// WASM bytecode
        wasm_bytes: alloc::vec::Vec<u8>,
        /// Application configuration
        config: Option<ApplicationConfig>,
    },
    
    /// Stop application on device
    StopApplication {
        /// Application ID
        app_id: alloc::string::String,
    },
    
    /// Request device information
    RequestDeviceInfo,
    
    /// Request application status
    RequestApplicationStatus {
        /// Application ID (None for all applications)
        app_id: Option<alloc::string::String>,
    },
}

/// Application status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Decode, Encode)]
#[cbor(index_only)]
pub enum ApplicationStatus {
    #[cbor(n(0))]
    Deploying,
    #[cbor(n(1))]
    Running,
    #[cbor(n(2))]
    Stopped,
    #[cbor(n(3))]
    Failed,
    #[cbor(n(4))]
    Unknown,
}

/// Application configuration
#[derive(Debug, Clone, PartialEq, Decode, Encode)]
pub struct ApplicationConfig {
    /// Memory limit in bytes
    #[cbor(n(0))]
    pub memory_limit: u64,
    /// CPU time limit in milliseconds
    #[cbor(n(1))]
    pub cpu_time_limit: u64,
    /// Environment variables
    #[cbor(n(2))]
    pub env_vars: alloc::collections::BTreeMap<alloc::string::String, alloc::string::String>,
    /// Startup arguments
    #[cbor(n(3))]
    pub args: alloc::vec::Vec<alloc::string::String>,
}

/// Application metrics
#[derive(Debug, Clone, PartialEq, Decode, Encode)]
pub struct ApplicationMetrics {
    /// Memory usage in bytes
    #[cbor(n(0))]
    pub memory_usage: u64,
    /// CPU usage percentage
    #[cbor(n(1))]
    pub cpu_usage: f32,
    /// Uptime in seconds
    #[cbor(n(2))]
    pub uptime: u64,
    /// Number of function calls
    #[cbor(n(3))]
    pub function_calls: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    
    #[test]
    fn test_device_uuid_to_string() {
        let uuid = DeviceUuid::new([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        assert_eq!(uuid.to_string(), "12345678-9abc-def0-1122-334455667788");
    }
    
    #[test]
    fn test_enrollment_request_roundtrip() {
        let msg = ClientMessage::EnrollmentRequest;
        let envelope = ClientEnvelope {
            version: Version::V0,
            message_id: MessageId::default(),
            message: msg,
        };
        
        // Test that it compiles and can be created
        assert_eq!(envelope.version, Version::V0);
        assert_eq!(envelope.message_id, MessageId::default());
    }
}
