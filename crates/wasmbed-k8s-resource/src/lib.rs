// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

mod device;
mod application;
mod gateway;

#[cfg(feature = "client")]
mod device_client;

#[cfg(feature = "client")]
mod application_client;

pub use device::{Device, DeviceSpec, DevicePhase, DeviceStatus};
pub use application::{Application, ApplicationSpec, ApplicationStatus, ApplicationPhase, DeviceApplicationStatus, DeviceApplicationPhase, ApplicationConfig, ApplicationMetadata, ApplicationMetrics, ApplicationStatistics, TargetDevices, DeviceSelectors, DeviceSelectorRequirement};
pub use gateway::{Gateway, GatewaySpec, GatewayStatus, GatewayPhase, GatewayCondition, GatewayConfig};

#[cfg(feature = "client")]
pub use device_client::DeviceStatusUpdate;
#[cfg(feature = "client")]
pub use application_client::ApplicationStatusUpdate;