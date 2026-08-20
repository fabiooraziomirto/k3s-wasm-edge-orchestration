// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use anyhow::{Context, Result};

use wasmbed_cert::{DistinguishedName, DnType, ClientAuthority, ServerAuthority};

#[derive(Parser)]
#[command(disable_help_subcommand = true)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    GenerateCa {
        #[arg(value_enum)]
        kind: CertKind,
        #[arg(long)]
        common_name: String,
        #[arg(long)]
        organization: Option<String>,
        #[arg(long)]
        organizational_unit: Option<String>,
        #[arg(long)]
        country: Option<String>,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        locality: Option<String>,
        #[arg(long, help = "Output path for the private key (e.g., ca.key)")]
        out_key: PathBuf,
        #[arg(long, help = "Output path for the certificate (e.g., ca.der)")]
        out_cert: PathBuf,
    },

    IssueCert {
        #[arg(value_enum)]
        kind: CertKind,
        #[arg(long)]
        ca_key: PathBuf,
        #[arg(long)]
        ca_cert: PathBuf,
        #[arg(long)]
        common_name: String,
        #[arg(long)]
        organization: Option<String>,
        #[arg(long)]
        organizational_unit: Option<String>,
        #[arg(long)]
        country: Option<String>,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        locality: Option<String>,
        #[arg(
            long,
            help = "Output path for the private key (e.g., identity.key)"
        )]
        out_key: PathBuf,
        #[arg(
            long,
            help = "Output path for the certificate (e.g., identity.der)"
        )]
        out_cert: PathBuf,
    },

    /// Issue the ECDSA P-256 identity a device authenticates with.
    ///
    /// Produces the three artefacts the fleet needs: the PKCS#8 private key and
    /// client certificate injected into the device, and the SubjectPublicKeyInfo
    /// that goes into `Device.spec.publicKey`.
    IssueDevice {
        #[arg(long, help = "Fleet CA private key (PEM)")]
        ca_key: PathBuf,
        #[arg(long, help = "Fleet CA certificate (PEM)")]
        ca_cert: PathBuf,
        #[arg(long, help = "Device name, used as the certificate common name")]
        device_id: String,
        #[arg(long, help = "Directory to write <device-id>.{key,crt,spki} into")]
        out_dir: PathBuf,
    },
}

#[derive(ValueEnum, Clone)]
enum CertKind {
    Server,
    Client,
}

fn build_distinguished_name(args: &Command) -> DistinguishedName {
    let mut dn = DistinguishedName::new();
    match args {
        Command::GenerateCa {
            common_name,
            organization,
            organizational_unit,
            country,
            state,
            locality,
            ..
        }
        | Command::IssueCert {
            common_name,
            organization,
            organizational_unit,
            country,
            state,
            locality,
            ..
        } => {
            dn.push(DnType::CommonName, common_name);
            if let Some(v) = organization {
                dn.push(DnType::OrganizationName, v);
            }
            if let Some(v) = organizational_unit {
                dn.push(DnType::OrganizationalUnitName, v);
            }
            if let Some(v) = country {
                dn.push(DnType::CountryName, v);
            }
            if let Some(v) = state {
                dn.push(DnType::StateOrProvinceName, v);
            }
            if let Some(v) = locality {
                dn.push(DnType::LocalityName, v);
            }
        },
        Command::IssueDevice { device_id, .. } => {
            dn.push(DnType::CommonName, device_id);
        },
    }
    dn
}

fn main() -> Result<()> {
    let cli = Args::parse();

    match &cli.command {
        Command::GenerateCa {
            kind,
            out_key,
            out_cert,
            ..
        } => {
            let dn = build_distinguished_name(&cli.command);
            match kind {
                CertKind::Server => {
                    let cred = ServerAuthority::new(dn)?;
                    std::fs::write(
                        out_key,
                        cred.private_key().secret_pkcs8_der(),
                    )
                    .with_context(|| format!("failed to write {out_key:?}"))?;
                    std::fs::write(out_cert, cred.certificate()).with_context(
                        || format!("failed to write {out_cert:?}"),
                    )?;
                },
                CertKind::Client => {
                    let cred = ClientAuthority::new(dn)?;
                    std::fs::write(
                        out_key,
                        cred.private_key().secret_pkcs8_der(),
                    )
                    .with_context(|| format!("failed to write {out_key:?}"))?;
                    std::fs::write(out_cert, cred.certificate()).with_context(
                        || format!("failed to write {out_cert:?}"),
                    )?;
                },
            }
        },

        Command::IssueCert {
            kind,
            ca_key,
            ca_cert,
            out_key,
            out_cert,
            ..
        } => {
            let ca_der = std::fs::read(ca_cert).with_context(|| {
                format!("failed to read CA cert from {ca_cert:?}")
            })?;
            let key_der = std::fs::read(ca_key).with_context(|| {
                format!("failed to read CA key from {ca_key:?}")
            })?;
            let dn = build_distinguished_name(&cli.command);

            match kind {
                CertKind::Server => {
                    let ca = ServerAuthority::from_parts(
                        key_der.into(),
                        ca_der.into(),
                    );
                    let issued = ca.issue_certificate(dn)?;
                    std::fs::write(
                        out_key,
                        issued.private_key().secret_pkcs8_der(),
                    )
                    .with_context(|| format!("failed to write {out_key:?}"))?;
                    std::fs::write(out_cert, issued.certificate())
                        .with_context(|| {
                            format!("failed to write {out_cert:?}")
                        })?;
                },
                CertKind::Client => {
                    let ca = ClientAuthority::from_parts(
                        key_der.into(),
                        ca_der.into(),
                    );
                    let issued = ca.issue_certificate(dn)?;
                    std::fs::write(
                        out_key,
                        issued.private_key().secret_pkcs8_der(),
                    )
                    .with_context(|| format!("failed to write {out_key:?}"))?;
                    std::fs::write(out_cert, issued.certificate())
                        .with_context(|| {
                            format!("failed to write {out_cert:?}")
                        })?;
                },
            }
        },

        Command::IssueDevice {
            ca_key,
            ca_cert,
            device_id,
            out_dir,
        } => {
            issue_device_identity(ca_key, ca_cert, device_id, out_dir)?;
        },
    }

    Ok(())
}

/// Issue one device identity signed by the fleet CA.
///
/// The device key is always ECDSA P-256: it is the only signature algorithm the
/// firmware's mbedTLS build can both present in the TLS handshake and use to
/// sign the gateway's proof-of-possession challenge. The CA key is loaded from
/// PEM so whatever algorithm the fleet CA already uses keeps working.
fn issue_device_identity(
    ca_key: &PathBuf,
    ca_cert: &PathBuf,
    device_id: &str,
    out_dir: &PathBuf,
) -> Result<()> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use rcgen::{
        CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
        KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
    };

    let ca_key_pem = std::fs::read_to_string(ca_key)
        .with_context(|| format!("failed to read CA key from {ca_key:?}"))?;
    let ca_cert_pem = std::fs::read_to_string(ca_cert)
        .with_context(|| format!("failed to read CA cert from {ca_cert:?}"))?;

    let ca_key_pair = KeyPair::from_pem(&ca_key_pem)
        .with_context(|| format!("failed to parse the CA key in {ca_key:?}"))?;
    let ca_params = CertificateParams::from_ca_cert_pem(&ca_cert_pem)
        .with_context(|| format!("failed to parse the CA certificate in {ca_cert:?}"))?;
    // rcgen signs against a Certificate, so rebuild one from the CA's own
    // parameters and key; the reconstructed certificate carries the same
    // subject and key, which is all the signature depends on.
    let ca_certificate = ca_params
        .self_signed(&ca_key_pair)
        .with_context(|| "failed to reconstruct the CA certificate")?;

    let device_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .with_context(|| "failed to generate a P-256 key pair")?;

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, device_id);

    let mut params = CertificateParams::default();
    params.distinguished_name = dn;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];

    let certificate = params
        .signed_by(&device_key, &ca_certificate, &ca_key_pair)
        .with_context(|| "failed to sign the device certificate")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {out_dir:?}"))?;

    let key_path = out_dir.join(format!("{device_id}.key"));
    let cert_path = out_dir.join(format!("{device_id}.crt"));
    let spki_path = out_dir.join(format!("{device_id}.spki"));

    let spki = device_key.public_key_der();

    std::fs::write(&key_path, device_key.serialize_der())
        .with_context(|| format!("failed to write {key_path:?}"))?;
    std::fs::write(&cert_path, certificate.der())
        .with_context(|| format!("failed to write {cert_path:?}"))?;
    std::fs::write(&spki_path, &spki)
        .with_context(|| format!("failed to write {spki_path:?}"))?;

    // The encoding PublicKey::to_base64 produces, i.e. what Device::find looks up.
    println!("{}", URL_SAFE_NO_PAD.encode(&spki));

    Ok(())
}
