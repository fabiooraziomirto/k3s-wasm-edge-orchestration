// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

mod protocol;
mod wasm_runner;

#[derive(Parser)]
#[command(name = "wasmbed-edge-client", about = "Wasmbed Linux edge daemon (Modello A1 CBOR uniforme)")]
pub struct Args {
    /// Gateway TLS endpoint (host:port)
    #[arg(long, env = "WASMBED_GATEWAY_ENDPOINT", default_value = "127.0.0.1:8081")]
    pub gateway: String,

    /// 32-byte Ed25519 public key in hex (device identity sent during enrollment)
    #[arg(long, env = "WASMBED_DEVICE_PUBLIC_KEY")]
    pub public_key: String,

    /// CA certificate (PEM) for server verification; omit to skip TLS server verification (dev only)
    #[arg(long, env = "WASMBED_CA_CERT")]
    pub ca_cert: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("wasmbed_edge_client=info")),
        )
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Failed to install rustls crypto provider"))?;

    let args = Args::parse();

    let public_key_bytes = hex::decode(&args.public_key)
        .context("Public key must be a hex string")?;
    if public_key_bytes.len() != 32 {
        anyhow::bail!(
            "Public key must be 32 bytes (Ed25519), got {}",
            public_key_bytes.len()
        );
    }

    tracing::info!("Connecting to gateway at {}", args.gateway);
    protocol::run(&args.gateway, public_key_bytes, args.ca_cert.as_deref()).await
}
