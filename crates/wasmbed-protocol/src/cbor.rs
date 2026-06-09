// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

#[cfg(feature = "alloc")]
extern crate alloc;

use derive_more::{Display, Error};
use minicbor::{Decode, Decoder, Encode, Encoder};
use minicbor::encode::{Error as EncodeError, Write};
use minicbor::decode::Error as DecodeError;
use crate::{ClientMessage, ServerMessage, DeviceUuid, ApplicationStatus, ApplicationConfig, ApplicationMetrics};
use alloc::string::ToString;

// Client message tags
const CLIENT_HEARTBEAT: u32 = 0;
const CLIENT_ENROLLMENT_REQUEST: u32 = 1;
const CLIENT_PUBLIC_KEY: u32 = 2;
const CLIENT_ENROLLMENT_ACKNOWLEDGMENT: u32 = 3;
const CLIENT_APPLICATION_STATUS: u32 = 4;
const CLIENT_APPLICATION_DEPLOY_ACK: u32 = 5;
const CLIENT_APPLICATION_STOP_ACK: u32 = 6;
const CLIENT_DEVICE_INFO: u32 = 7;

// Server message tags
const SERVER_HEARTBEAT_ACK: u32 = 0;
const SERVER_ENROLLMENT_ACCEPTED: u32 = 1;
const SERVER_ENROLLMENT_REJECTED: u32 = 2;
const SERVER_DEVICE_UUID: u32 = 3;
const SERVER_ENROLLMENT_COMPLETED: u32 = 4;
const SERVER_DEPLOY_APPLICATION: u32 = 5;
const SERVER_STOP_APPLICATION: u32 = 6;
const SERVER_REQUEST_DEVICE_INFO: u32 = 7;
const SERVER_REQUEST_APPLICATION_STATUS: u32 = 8;

#[derive(Debug, Display, Error)]
enum MessageDecodeError {
    #[display(
        "Unexpected array length: it should be {expected} but it is {actual}"
    )]
    UnexpectedArrayLength {
        expected: u64,
        actual: u64,
    },
    #[display("Unknown tag: {tag}")]
    UnknownTag {
        tag: u32,
    },
    #[display("Unexpected indefinite length array")]
    UnexpectedIndefiniteLengthArray,
    #[display("Invalid UUID length: expected 16, got {actual}")]
    InvalidUuidLength {
        actual: usize,
    },
}

impl Encode<()> for ClientMessage {
    fn encode<W: Write>(
        &self,
        e: &mut Encoder<W>,
        _ctx: &mut (),
    ) -> Result<(), EncodeError<W::Error>> {
        match self {
            ClientMessage::Heartbeat => {
                e.array(1)?.u32(CLIENT_HEARTBEAT)?;
            },
            ClientMessage::EnrollmentRequest => {
                e.array(1)?.u32(CLIENT_ENROLLMENT_REQUEST)?;
            },
            ClientMessage::PublicKey { key } => {
                e.array(2)?.u32(CLIENT_PUBLIC_KEY)?.bytes(key)?;
            },
            ClientMessage::EnrollmentAcknowledgment => {
                e.array(1)?.u32(CLIENT_ENROLLMENT_ACKNOWLEDGMENT)?;
            },
            ClientMessage::ApplicationStatus { app_id, status, error, metrics } => {
                e.array(5)?.u32(CLIENT_APPLICATION_STATUS)?.str(app_id)?.u32(*status as u32)?;
                if let Some(err) = error {
                    e.str(err)?;
                } else {
                    e.null()?;
                }
                if let Some(m) = metrics {
                    e.u64(m.memory_usage)?.f32(m.cpu_usage)?.u64(m.uptime)?.u64(m.function_calls)?;
                } else {
                    e.null()?;
                }
            },
            ClientMessage::ApplicationDeployAck { app_id, success, error } => {
                e.array(4)?.u32(CLIENT_APPLICATION_DEPLOY_ACK)?.str(app_id)?.bool(*success)?;
                if let Some(err) = error {
                    e.str(err)?;
                } else {
                    e.null()?;
                }
            },
            ClientMessage::ApplicationStopAck { app_id, success, error } => {
                e.array(4)?.u32(CLIENT_APPLICATION_STOP_ACK)?.str(app_id)?.bool(*success)?;
                if let Some(err) = error {
                    e.str(err)?;
                } else {
                    e.null()?;
                }
            },
            ClientMessage::DeviceInfo { available_memory, cpu_arch, wasm_features, max_app_size } => {
                e.array(5)?.u32(CLIENT_DEVICE_INFO)?.u64(*available_memory)?.str(cpu_arch)?;
                e.array(wasm_features.len() as u64)?;
                for feature in wasm_features {
                    e.str(feature)?;
                }
                e.u64(*max_app_size)?;
            },
        }
        Ok(())
    }
}

impl<'b> Decode<'b, ()> for ClientMessage {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut ()) -> Result<Self, DecodeError> {
        let array_len = d.array()?.ok_or_else(|| {
            DecodeError::custom(
                MessageDecodeError::UnexpectedIndefiniteLengthArray,
            )
        })?;

        let tag = d.u32()?;
        match (tag, array_len) {
            (CLIENT_HEARTBEAT, 1) => Ok(ClientMessage::Heartbeat),
            (CLIENT_ENROLLMENT_REQUEST, 1) => Ok(ClientMessage::EnrollmentRequest),
            (CLIENT_PUBLIC_KEY, 2) => {
                let key = d.bytes()?.to_vec();
                Ok(ClientMessage::PublicKey { key })
            },
            (CLIENT_ENROLLMENT_ACKNOWLEDGMENT, 1) => Ok(ClientMessage::EnrollmentAcknowledgment),
            (CLIENT_APPLICATION_STATUS, 5) => {
                let app_id = d.str()?.to_string();
                let status_val = d.u32()?;
                let status = match status_val {
                    0 => ApplicationStatus::Deploying,
                    1 => ApplicationStatus::Running,
                    2 => ApplicationStatus::Stopped,
                    3 => ApplicationStatus::Failed,
                    4 => ApplicationStatus::Unknown,
                    _ => ApplicationStatus::Unknown,
                };
                let error = if d.datatype()? == minicbor::data::Type::Null {
                    d.skip()?;
                    None
                } else {
                    Some(d.str()?.to_string())
                };
                let metrics = if d.datatype()? == minicbor::data::Type::Null {
                    d.skip()?;
                    None
                } else {
                    Some(ApplicationMetrics {
                        memory_usage: d.u64()?,
                        cpu_usage: d.f32()?,
                        uptime: d.u64()?,
                        function_calls: d.u64()?,
                    })
                };
                Ok(ClientMessage::ApplicationStatus { app_id, status, error, metrics })
            },
            (CLIENT_APPLICATION_DEPLOY_ACK, 4) => {
                let app_id = d.str()?.to_string();
                let success = d.bool()?;
                let error = if d.datatype()? == minicbor::data::Type::Null {
                    d.skip()?;
                    None
                } else {
                    Some(d.str()?.to_string())
                };
                Ok(ClientMessage::ApplicationDeployAck { app_id, success, error })
            },
            (CLIENT_APPLICATION_STOP_ACK, 4) => {
                let app_id = d.str()?.to_string();
                let success = d.bool()?;
                let error = if d.datatype()? == minicbor::data::Type::Null {
                    d.skip()?;
                    None
                } else {
                    Some(d.str()?.to_string())
                };
                Ok(ClientMessage::ApplicationStopAck { app_id, success, error })
            },
            (CLIENT_DEVICE_INFO, 5) => {
                let available_memory = d.u64()?;
                let cpu_arch = d.str()?.to_string();
                let features_len = d.array()?.ok_or_else(|| {
                    DecodeError::custom(MessageDecodeError::UnexpectedIndefiniteLengthArray)
                })?;
                let mut wasm_features = alloc::vec::Vec::new();
                for _ in 0..features_len {
                    wasm_features.push(d.str()?.to_string());
                }
                let max_app_size = d.u64()?;
                Ok(ClientMessage::DeviceInfo { available_memory, cpu_arch, wasm_features, max_app_size })
            },
            (CLIENT_HEARTBEAT, _) | (CLIENT_ENROLLMENT_REQUEST, _) | (CLIENT_ENROLLMENT_ACKNOWLEDGMENT, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 1,
                        actual: array_len,
                    },
                ))
            },
            (CLIENT_PUBLIC_KEY, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 2,
                        actual: array_len,
                    },
                ))
            },
            (CLIENT_APPLICATION_STATUS, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 5,
                        actual: array_len,
                    },
                ))
            },
            (CLIENT_APPLICATION_DEPLOY_ACK, _) | (CLIENT_APPLICATION_STOP_ACK, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 4,
                        actual: array_len,
                    },
                ))
            },
            (CLIENT_DEVICE_INFO, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 5,
                        actual: array_len,
                    },
                ))
            },
            _ => {
                Err(DecodeError::custom(MessageDecodeError::UnknownTag { tag }))
            },
        }
    }
}

impl Encode<()> for ServerMessage {
    fn encode<W: Write>(
        &self,
        e: &mut Encoder<W>,
        _ctx: &mut (),
    ) -> Result<(), EncodeError<W::Error>> {
        match self {
            ServerMessage::HeartbeatAck => {
                e.array(1)?.u32(SERVER_HEARTBEAT_ACK)?;
            },
            ServerMessage::EnrollmentAccepted => {
                e.array(1)?.u32(SERVER_ENROLLMENT_ACCEPTED)?;
            },
            ServerMessage::EnrollmentRejected { reason } => {
                e.array(2)?.u32(SERVER_ENROLLMENT_REJECTED)?.bytes(reason)?;
            },
            ServerMessage::DeviceUuid { uuid } => {
                e.array(2)?.u32(SERVER_DEVICE_UUID)?.bytes(&uuid.bytes)?;
            },
            ServerMessage::EnrollmentCompleted => {
                e.array(1)?.u32(SERVER_ENROLLMENT_COMPLETED)?;
            },
            ServerMessage::DeployApplication { app_id, name, wasm_bytes, config } => {
                e.array(5)?.u32(SERVER_DEPLOY_APPLICATION)?.str(app_id)?.str(name)?.bytes(wasm_bytes)?;
                if let Some(cfg) = config {
                    e.u64(cfg.memory_limit)?.u64(cfg.cpu_time_limit)?;
                    e.map(cfg.env_vars.len() as u64)?;
                    for (k, v) in &cfg.env_vars {
                        e.str(k)?.str(v)?;
                    }
                    e.array(cfg.args.len() as u64)?;
                    for arg in &cfg.args {
                        e.str(arg)?;
                    }
                } else {
                    e.null()?;
                }
            },
            ServerMessage::StopApplication { app_id } => {
                e.array(2)?.u32(SERVER_STOP_APPLICATION)?.str(app_id)?;
            },
            ServerMessage::RequestDeviceInfo => {
                e.array(1)?.u32(SERVER_REQUEST_DEVICE_INFO)?;
            },
            ServerMessage::RequestApplicationStatus { app_id } => {
                e.array(2)?.u32(SERVER_REQUEST_APPLICATION_STATUS)?;
                if let Some(id) = app_id {
                    e.str(id)?;
                } else {
                    e.null()?;
                }
            },
        }
        Ok(())
    }
}

impl<'b> Decode<'b, ()> for ServerMessage {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut ()) -> Result<Self, DecodeError> {
        let array_len = d.array()?.ok_or_else(|| {
            DecodeError::custom(
                MessageDecodeError::UnexpectedIndefiniteLengthArray,
            )
        })?;

        let tag = d.u32()?;
        match (tag, array_len) {
            (SERVER_HEARTBEAT_ACK, 1) => Ok(ServerMessage::HeartbeatAck),
            (SERVER_ENROLLMENT_ACCEPTED, 1) => Ok(ServerMessage::EnrollmentAccepted),
            (SERVER_ENROLLMENT_REJECTED, 2) => {
                let reason = d.bytes()?.to_vec();
                Ok(ServerMessage::EnrollmentRejected { reason })
            },
            (SERVER_DEVICE_UUID, 2) => {
                let uuid_bytes = d.bytes()?;
                if uuid_bytes.len() != 16 {
                    return Err(DecodeError::custom(
                        MessageDecodeError::InvalidUuidLength {
                            actual: uuid_bytes.len(),
                        }
                    ));
                }
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(uuid_bytes);
                let uuid = DeviceUuid::new(bytes);
                Ok(ServerMessage::DeviceUuid { uuid })
            },
            (SERVER_ENROLLMENT_COMPLETED, 1) => Ok(ServerMessage::EnrollmentCompleted),
            (SERVER_DEPLOY_APPLICATION, 5) => {
                let app_id = d.str()?.to_string();
                let name = d.str()?.to_string();
                let wasm_bytes = d.bytes()?.to_vec();
                let config = if d.datatype()? == minicbor::data::Type::Null {
                    d.skip()?;
                    None
                } else {
                    let memory_limit = d.u64()?;
                    let cpu_time_limit = d.u64()?;
                    let env_map_len = d.map()?.ok_or_else(|| {
                        DecodeError::custom(MessageDecodeError::UnexpectedIndefiniteLengthArray)
                    })?;
                    let mut env_vars = alloc::collections::BTreeMap::new();
                    for _ in 0..env_map_len {
                        let k = d.str()?.to_string();
                        let v = d.str()?.to_string();
                        env_vars.insert(k, v);
                    }
                    let args_len = d.array()?.ok_or_else(|| {
                        DecodeError::custom(MessageDecodeError::UnexpectedIndefiniteLengthArray)
                    })?;
                    let mut args = alloc::vec::Vec::new();
                    for _ in 0..args_len {
                        args.push(d.str()?.to_string());
                    }
                    Some(ApplicationConfig { memory_limit, cpu_time_limit, env_vars, args })
                };
                Ok(ServerMessage::DeployApplication { app_id, name, wasm_bytes, config })
            },
            (SERVER_STOP_APPLICATION, 2) => {
                let app_id = d.str()?.to_string();
                Ok(ServerMessage::StopApplication { app_id })
            },
            (SERVER_REQUEST_DEVICE_INFO, 1) => Ok(ServerMessage::RequestDeviceInfo),
            (SERVER_REQUEST_APPLICATION_STATUS, 2) => {
                let app_id = if d.datatype()? == minicbor::data::Type::Null {
                    d.skip()?;
                    None
                } else {
                    Some(d.str()?.to_string())
                };
                Ok(ServerMessage::RequestApplicationStatus { app_id })
            },
            (SERVER_HEARTBEAT_ACK, _) | (SERVER_ENROLLMENT_ACCEPTED, _) | (SERVER_ENROLLMENT_COMPLETED, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 1,
                        actual: array_len,
                    },
                ))
            },
            (SERVER_ENROLLMENT_REJECTED, _) | (SERVER_DEVICE_UUID, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 2,
                        actual: array_len,
                    },
                ))
            },
            (SERVER_DEPLOY_APPLICATION, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 5,
                        actual: array_len,
                    },
                ))
            },
            (SERVER_STOP_APPLICATION, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 2,
                        actual: array_len,
                    },
                ))
            },
            (SERVER_REQUEST_DEVICE_INFO, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 1,
                        actual: array_len,
                    },
                ))
            },
            (SERVER_REQUEST_APPLICATION_STATUS, _) => {
                Err(DecodeError::custom(
                    MessageDecodeError::UnexpectedArrayLength {
                        expected: 2,
                        actual: array_len,
                    },
                ))
            },
            _ => {
                Err(DecodeError::custom(MessageDecodeError::UnknownTag { tag }))
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use wasmbed_test_utils::minicbor::assert_encode_decode;

    #[test]
    fn test_client_message_heartbeat() {
        assert_encode_decode(&ClientMessage::Heartbeat);
    }

    #[test]
    fn test_server_message_heartbeat_ack() {
        assert_encode_decode(&ServerMessage::HeartbeatAck);
    }
}
