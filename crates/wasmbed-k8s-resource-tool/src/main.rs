// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kube::CustomResourceExt;
use rustls_pki_types::CertificateDer;

use wasmbed_k8s_resource::{Device, DeviceSpec, Application, ApplicationSpec};
use wasmbed_types::PublicKey;

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate the CRD YAML for a given resource.
    #[command(name = "crd", subcommand)]
    GenerateCrd(Resource),
    /// Generate a resource manifest YAML.
    #[command(name = "manifest", subcommand)]
    GenerateManifest(ManifestResource),
}

#[derive(Subcommand)]
enum Resource {
    /// Generate the CRD YAML for the "Device" resource.
    Device,
    /// Generate the CRD YAML for the "Application" resource.
    Application,
}

#[derive(Subcommand)]
enum ManifestResource {
    /// Generate a manifest for the "Device" resource.
    Device {
        /// Metadata.name of the resource.
        #[arg(long)]
        name: String,
        /// Path to the device's certificate in DER format.
        #[arg(long = "cert", value_name = "FILE")]
        certificate: PathBuf,
    },
    /// Generate a manifest for the "Application" resource.
    Application {
        /// Metadata.name of the resource.
        #[arg(long)]
        name: String,
        /// Application name.
        #[arg(long)]
        app_name: String,
        /// Application description.
        #[arg(long)]
        description: Option<String>,
        /// Path to the WASM file.
        #[arg(long = "wasm", value_name = "FILE")]
        wasm_file: PathBuf,
        /// Target device names (comma-separated).
        #[arg(long = "devices")]
        target_devices: String,
    },
}

pub fn main() -> Result<()> {
    use std::io::Write;

    let args = Args::parse();
    match args.command {
        Command::GenerateCrd(resource) => match resource {
            Resource::Device => {
                std::io::stdout().write_all(
                    &serde_yaml::to_string(&Device::crd())?.into_bytes(),
                )?;
            },
            Resource::Application => {
                std::io::stdout().write_all(
                    &serde_yaml::to_string(&Application::crd())?.into_bytes(),
                )?;
            },
        },

        Command::GenerateManifest(resource) => match resource {
            ManifestResource::Device { name, certificate } => {
                let cert_bytes =
                    std::fs::read(&certificate).with_context(|| {
                        format!(
                            "Failed to read certificate from {}",
                            certificate.display()
                        )
                    })?;
                let cert = CertificateDer::from_slice(&cert_bytes);
                let public_key: PublicKey = (&cert).try_into()?;

                let device = Device::new(
                    &name,
                    DeviceSpec {
                        public_key: public_key.to_string(),
                        mcu_type: Some("Mps2An385".to_string()),
                        preferred_gateway: None,
                        device_class: None,
                        runtime_target: None,
                    },
                );

                std::io::stdout()
                    .write_all(&serde_yaml::to_string(&device)?.into_bytes())?;
            },
            ManifestResource::Application { name, app_name, description, wasm_file, target_devices } => {
                let wasm_bytes = std::fs::read(&wasm_file).with_context(|| {
                    format!(
                        "Failed to read WASM file from {}",
                        wasm_file.display()
                    )
                })?;
                
                let wasm_base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &wasm_bytes);
                
                let device_names: Vec<String> = target_devices
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                let application = Application::new(
                    &name,
                    ApplicationSpec {
                        name: app_name,
                        description,
                        wasm_bytes: wasm_base64,
                        target_devices: wasmbed_k8s_resource::TargetDevices {
                            device_names: Some(device_names),
                            selectors: None,
                            all_devices: None,
                        },
                        config: None,
                        metadata: None,
                        target_runtime: None,
                    },
                );

                std::io::stdout()
                    .write_all(&serde_yaml::to_string(&application)?.into_bytes())?;
            },
        },
    };

    Ok(())
}
