// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use wasmbed_protocol::{ClientMessage, ServerMessage};

use crate::identity::DeviceIdentity;
use crate::wasm_runner::WasmRunner;

// ── TLS helpers ──────────────────────────────────────────────────────────────

/// Builds a TLS connector that skips server certificate verification.
/// Only safe for development/testing; not for production.
fn connector_no_verify() -> Result<TlsConnector> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
    use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

    #[derive(Debug)]
    struct SkipVerifier;

    impl ServerCertVerifier for SkipVerifier {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer,
            _intermediates: &[CertificateDer],
            _server_name: &ServerName,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, TlsError> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, TlsError> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, TlsError> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::ECDSA_NISTP521_SHA512,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
            ]
        }
    }

    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerifier))
        .with_no_client_auth();

    Ok(TlsConnector::from(Arc::new(config)))
}

/// Builds a TLS connector that verifies the server using the provided CA certificate (PEM).
fn connector_with_ca(ca_cert_pem: &Path) -> Result<TlsConnector> {
    use rustls::{ClientConfig, RootCertStore};

    let pem_data = std::fs::read(ca_cert_pem)
        .with_context(|| format!("Reading CA cert from {}", ca_cert_pem.display()))?;

    let certs = rustls_pemfile::certs(&mut pem_data.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .context("Parsing CA cert PEM")?;

    let mut root_store = RootCertStore::empty();
    for cert in certs {
        root_store.add(cert).context("Adding CA cert to root store")?;
    }

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(TlsConnector::from(Arc::new(config)))
}

// ── Wire framing ─────────────────────────────────────────────────────────────

/// Reads one length-prefixed CBOR frame: `[u32 BE length][payload]`.
async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await.context("Reading frame length")?;
    let msg_len = u32::from_be_bytes(len_buf);
    if msg_len == 0 || msg_len > 65_536 {
        anyhow::bail!("Invalid frame length: {}", msg_len);
    }
    let msg_len_usize = usize::try_from(msg_len).context("Frame length overflow")?;
    let mut payload = vec![0u8; msg_len_usize];
    reader.read_exact(&mut payload).await.context("Reading frame payload")?;
    Ok(payload)
}

/// Encodes a `ClientMessage` and sends it as a length-prefixed frame.
async fn send_message<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &ClientMessage) -> Result<()> {
    let cbor = minicbor::to_vec(msg).context("CBOR encoding ClientMessage")?;
    let len: u32 = u32::try_from(cbor.len()).context("Message too large for u32 length")?;
    writer.write_all(&len.to_be_bytes()).await.context("Writing frame length")?;
    writer.write_all(&cbor).await.context("Writing frame payload")?;
    writer.flush().await.context("Flushing TLS writer")?;
    Ok(())
}

/// Decodes a raw CBOR payload into a `ServerMessage`.
fn decode_server(payload: &[u8]) -> Result<ServerMessage> {
    minicbor::decode(payload).context("CBOR decoding ServerMessage")
}

// ── Enrollment ───────────────────────────────────────────────────────────────

/// Carries out the enrollment handshake, including the proof-of-possession
/// challenge: the device announces its SubjectPublicKeyInfo (the value stored in
/// `Device.spec.publicKey`) and then signs the gateway's nonce with the matching
/// private key.
async fn enroll<R, W>(reader: &mut R, writer: &mut W, identity: &DeviceIdentity) -> Result<()>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    // 1 — EnrollmentRequest
    tracing::info!("Enrollment: sending EnrollmentRequest");
    send_message(writer, &ClientMessage::EnrollmentRequest).await?;

    // 2 — EnrollmentAccepted
    let payload = read_frame(reader).await?;
    match decode_server(&payload)? {
        ServerMessage::EnrollmentAccepted => {
            tracing::info!("Enrollment: accepted by gateway");
        }
        ServerMessage::EnrollmentRejected { reason } => {
            anyhow::bail!(
                "Enrollment rejected: {}",
                String::from_utf8_lossy(&reason)
            );
        }
        other => {
            anyhow::bail!("Unexpected message during enrollment: {:?}", other);
        }
    }

    // 3 — PublicKey
    let public_key = identity.spki();
    tracing::info!("Enrollment: sending PublicKey ({} bytes)", public_key.len());
    send_message(
        writer,
        &ClientMessage::PublicKey {
            key: public_key.to_vec(),
        },
    )
    .await?;

    // 4 — Challenge, then the signature proving we hold the private key.
    // A gateway that predates the challenge exchange answers with DeviceUuid
    // straight away; accept that so a new client still talks to an old gateway.
    let payload = read_frame(reader).await?;
    match decode_server(&payload)? {
        ServerMessage::Challenge { nonce } => {
            tracing::info!("Enrollment: signing {}-byte challenge", nonce.len());
            let signature = identity.sign_challenge(&nonce)?;
            send_message(writer, &ClientMessage::ChallengeResponse { signature }).await?;

            let payload = read_frame(reader).await?;
            match decode_server(&payload)? {
                ServerMessage::DeviceUuid { uuid } => {
                    tracing::info!("Enrollment: received UUID {}", uuid);
                }
                ServerMessage::EnrollmentRejected { reason } => {
                    anyhow::bail!(
                        "Proof of possession rejected: {}",
                        String::from_utf8_lossy(&reason)
                    );
                }
                other => {
                    anyhow::bail!("Expected DeviceUuid after challenge, got {:?}", other);
                }
            }
        }
        ServerMessage::DeviceUuid { uuid } => {
            tracing::warn!(
                "Enrollment: gateway issued no challenge; identity is unproven (UUID {})",
                uuid
            );
        }
        other => {
            anyhow::bail!("Expected Challenge or DeviceUuid, got {:?}", other);
        }
    }

    // 5 — EnrollmentAcknowledgment
    tracing::info!("Enrollment: sending EnrollmentAcknowledgment");
    send_message(writer, &ClientMessage::EnrollmentAcknowledgment).await?;

    // 6 — EnrollmentCompleted (optional — gateway may or may not send it)
    let payload = read_frame(reader).await?;
    match decode_server(&payload)? {
        ServerMessage::EnrollmentCompleted => {
            tracing::info!("Enrollment: completed successfully");
        }
        other => {
            tracing::warn!("Expected EnrollmentCompleted, got {:?}; continuing", other);
        }
    }

    Ok(())
}

// ── Main event loop ───────────────────────────────────────────────────────────

/// Main loop after enrollment: heartbeat + handle incoming server messages.
async fn event_loop<R>(
    mut reader: R,
    tx: mpsc::Sender<ClientMessage>,
) -> Result<()>
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    let mut runner = WasmRunner::new();

    loop {
        let payload = match read_frame(&mut reader).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Connection closed: {}", e);
                break;
            }
        };

        let msg = match decode_server(&payload) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("CBOR decode error: {}", e);
                continue;
            }
        };

        match msg {
            ServerMessage::HeartbeatAck => {
                tracing::debug!("Heartbeat acknowledged");
            }

            // A challenge belongs to the enrollment handshake; reaching the
            // steady-state loop means the gateway re-challenged us and this
            // client cannot answer without re-enrolling.
            ServerMessage::Challenge { .. } => {
                tracing::warn!("Unexpected Challenge outside enrollment; ignoring");
            }

            ServerMessage::DeployApplication { app_id, name, module_hash, wasm_bytes, config } => {
                tracing::info!("Deploying application '{}' ({})", name, app_id);
                // Refuse a module whose bytes do not match the digest the
                // gateway announced, before it reaches the runtime.
                let actual_hash = {
                    use sha2::{Digest, Sha256};
                    Sha256::digest(&wasm_bytes).to_vec()
                };
                let (success, error) = if actual_hash != module_hash {
                    tracing::error!(
                        "Module hash mismatch for '{}': expected {} bytes digest {:02x?}, got {:02x?}",
                        app_id, module_hash.len(), module_hash, actual_hash
                    );
                    (false, Some("module hash mismatch".to_string()))
                } else {
                    match runner.deploy(app_id.clone(), wasm_bytes, config) {
                        Ok(()) => (true, None),
                        Err(e) => {
                            tracing::error!("Deploy failed for '{}': {}", app_id, e);
                            (false, Some(e.to_string()))
                        }
                    }
                };
                let ack = ClientMessage::ApplicationDeployAck { app_id, success, error };
                if tx.send(ack).await.is_err() {
                    break;
                }
            }

            ServerMessage::StopApplication { app_id } => {
                tracing::info!("Stopping application '{}'", app_id);
                let (success, error) = match runner.stop(&app_id) {
                    Ok(()) => (true, None),
                    Err(e) => {
                        tracing::warn!("Stop failed for '{}': {}", app_id, e);
                        (false, Some(e.to_string()))
                    }
                };
                let ack = ClientMessage::ApplicationStopAck { app_id, success, error };
                if tx.send(ack).await.is_err() {
                    break;
                }
            }

            ServerMessage::RequestDeviceInfo => {
                let info = ClientMessage::DeviceInfo {
                    available_memory: available_memory_bytes(),
                    cpu_arch: std::env::consts::ARCH.to_string(),
                    wasm_features: vec!["wasi_preview1".to_string(), "wasmtime".to_string()],
                    max_app_size: 16 * 1024 * 1024, // 16 MiB
                };
                if tx.send(info).await.is_err() {
                    break;
                }
            }

            ServerMessage::RequestApplicationStatus { app_id } => {
                let ids = match app_id {
                    Some(id) => vec![id],
                    None => runner.running_app_ids(),
                };
                for id in ids {
                    let status_msg = ClientMessage::ApplicationStatus {
                        app_id: id.clone(),
                        status: if runner.is_running(&id) {
                            wasmbed_protocol::ApplicationStatus::Running
                        } else {
                            wasmbed_protocol::ApplicationStatus::Stopped
                        },
                        error: None,
                        metrics: None,
                    };
                    if tx.send(status_msg).await.is_err() {
                        return Ok(());
                    }
                }
            }

            // Not expected from gateway in normal operation
            ServerMessage::EnrollmentAccepted
            | ServerMessage::EnrollmentRejected { .. }
            | ServerMessage::DeviceUuid { .. }
            | ServerMessage::EnrollmentCompleted => {
                tracing::warn!("Unexpected enrollment message received after enrollment");
            }
        }
    }

    Ok(())
}

/// Returns available system memory in bytes (best-effort; 0 if unknown).
fn available_memory_bytes() -> u64 {
    // Read from /proc/meminfo MemAvailable line on Linux
    let Ok(contents) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for line in contents.lines() {
        if line.starts_with("MemAvailable:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(kb_str) = parts.get(1) {
                if let Ok(kb) = kb_str.parse::<u64>() {
                    return kb.saturating_mul(1024);
                }
            }
        }
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Connect to the gateway, enroll, and run the event loop indefinitely.
pub async fn run(
    gateway_addr: &str,
    identity: DeviceIdentity,
    ca_cert: Option<&Path>,
) -> Result<()> {
    let connector = match ca_cert {
        Some(path) => {
            tracing::info!("TLS: verifying server with CA cert {}", path.display());
            connector_with_ca(path)?
        }
        None => {
            tracing::warn!("TLS: skipping server certificate verification (development mode)");
            connector_no_verify()?
        }
    };

    let stream = TcpStream::connect(gateway_addr)
        .await
        .with_context(|| format!("TCP connect to {}", gateway_addr))?;

    // SNI server name — use the host part only (no port)
    let host = gateway_addr
        .split(':')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty gateway address"))?;
    let server_name: rustls_pki_types::ServerName<'static> = host
        .to_string()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid SNI server name: {}", host))?;

    let tls_stream = connector
        .connect(server_name, stream)
        .await
        .with_context(|| format!("TLS handshake with {}", gateway_addr))?;

    tracing::info!("TLS connected to {}", gateway_addr);

    let (mut reader, mut writer) = tokio::io::split(tls_stream);

    // Enrollment
    enroll(&mut reader, &mut writer, &identity).await?;
    tracing::info!("Enrolled successfully");

    // Channel: event loop → writer task
    let (tx, mut rx) = mpsc::channel::<ClientMessage>(32);

    // Writer task: forwards queued ClientMessages to gateway
    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = send_message(&mut writer, &msg).await {
                tracing::warn!("Write error, closing: {}", e);
                break;
            }
        }
    });

    // Heartbeat task: sends a Heartbeat every 25 s
    let hb_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(25));
        loop {
            interval.tick().await;
            if hb_tx.send(ClientMessage::Heartbeat).await.is_err() {
                break;
            }
            tracing::debug!("Sent heartbeat");
        }
    });

    // Event loop: reads incoming server messages
    event_loop(reader, tx).await?;

    write_task.await.ok();
    Ok(())
}
