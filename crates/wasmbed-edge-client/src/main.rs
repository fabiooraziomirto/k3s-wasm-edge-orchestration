// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

mod identity;
mod protocol;
mod wasm_runner;

#[derive(Parser)]
#[command(name = "wasmbed-edge-client", about = "Wasmbed Linux edge daemon (Modello A1 CBOR uniforme)")]
pub struct Args {
    /// Gateway TLS endpoint (host:port)
    #[arg(long, env = "WASMBED_GATEWAY_ENDPOINT", default_value = "127.0.0.1:8081")]
    pub gateway: String,

    /// PKCS#8 DER private key provisioned for this device. Required to answer
    /// the gateway's proof-of-possession challenge; the announced public key is
    /// derived from it.
    #[arg(long, env = "WASMBED_DEVICE_KEY")]
    pub identity_key: PathBuf,

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

    let identity = identity::DeviceIdentity::load(&args.identity_key)
        .context("Loading the device identity key")?;

    tracing::info!("Connecting to gateway at {}", args.gateway);
    protocol::run(&args.gateway, identity, args.ca_cert.as_deref()).await
}
