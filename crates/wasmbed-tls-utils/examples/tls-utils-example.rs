// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

//! Example usage of the wasmbed-tls-utils library
//! 
//! This example demonstrates how to use the TLS utilities for:
//! - Parsing PEM certificates and private keys
//! - Extracting certificate information
//! - Validating certificate chains
//! - Checking certificate expiration

use anyhow::Result;
use wasmbed_tls_utils::{TlsUtils, CertificateInfo};

fn main() -> Result<()> {
    println!("Wasmbed TLS Utils Example");
    println!("=========================");
    
    // Example 1: Parse a private key
    println!("\n1. Parsing Private Key");
    let private_key_pem = b"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCqQtTzrZZlXxzTx
CLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i7hD69nd6IzmTLTfQJFyp1EHGuOW
2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKoZIhvcNAQELBQELBQADgYEAWVTk8aWm
HAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END PRIVATE KEY-----";
    
    match TlsUtils::parse_private_key(private_key_pem) {
        Ok(key) => println!("✓ Private key parsed successfully"),
        Err(e) => println!("✗ Failed to parse private key: {}", e),
    }
    
    // Example 2: Parse a certificate
    println!("\n2. Parsing Certificate");
    let cert_pem = b"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKpC1HNuZliXxzTxCLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i
7hD69nd6IzmTLTfQJFyp1EHGuOW2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKo
ZIhvcNAQELBQADgYEAWVTk8aWmHAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END CERTIFICATE-----";
    
    match TlsUtils::parse_certificate(cert_pem) {
        Ok(cert) => {
            println!("✓ Certificate parsed successfully");
            
            // Example 3: Extract certificate information
            println!("\n3. Certificate Information");
            match TlsUtils::get_certificate_info(&cert) {
                Ok(info) => {
                    println!("  Subject: {}", info.subject);
                    println!("  Issuer: {}", info.issuer);
                    println!("  Serial Number: {}", info.serial_number);
                    println!("  Public Key Length: {} bytes", info.public_key.len());
                    println!("  Valid From: {:?}", info.not_before);
                    println!("  Valid Until: {:?}", info.not_after);
                },
                Err(e) => println!("✗ Failed to extract certificate info: {}", e),
            }
            
            // Example 4: Check if certificate is expired
            println!("\n4. Certificate Expiration Check");
            match TlsUtils::is_certificate_expired(&cert) {
                Ok(expired) => {
                    if expired {
                        println!("⚠ Certificate is expired");
                    } else {
                        println!("✓ Certificate is still valid");
                    }
                },
                Err(e) => println!("✗ Failed to check certificate expiration: {}", e),
            }
            
            // Example 5: Check hostname validity
            println!("\n5. Hostname Validation");
            let hostnames = ["localhost", "example.com", "wasmbed.dev"];
            for hostname in &hostnames {
                match TlsUtils::is_certificate_valid_for_hostname(&cert, hostname) {
                    Ok(valid) => {
                        if valid {
                            println!("✓ Certificate is valid for {}", hostname);
                        } else {
                            println!("✗ Certificate is not valid for {}", hostname);
                        }
                    },
                    Err(e) => println!("✗ Failed to validate hostname {}: {}", hostname, e),
                }
            }
        },
        Err(e) => println!("✗ Failed to parse certificate: {}", e),
    }
    
    // Example 6: Parse multiple certificates
    println!("\n6. Parsing Multiple Certificates");
    let certs_pem = b"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKpC1HNuZliXxzTxCLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i
7hD69nd6IzmTLTfQJFyp1EHGuOW2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKo
ZIhvcNAQELBQADgYEAWVTk8aWmHAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKpC1HNuZliXxzTxCLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i
7hD69nd6IzmTLTfQJFyp1EHGuOW2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKo
ZIhvcNAQELBQADgYEAWVTk8aWmHAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END CERTIFICATE-----";
    
    match TlsUtils::parse_certificates(certs_pem) {
        Ok(certs) => println!("✓ Parsed {} certificates", certs.len()),
        Err(e) => println!("✗ Failed to parse certificates: {}", e),
    }
    
    // Example 7: Generate Ed25519 keypair (not implemented yet)
    println!("\n7. Generate Ed25519 Keypair");
    match TlsUtils::generate_ed25519_keypair() {
        Ok((private_key, public_key)) => {
            println!("✓ Generated Ed25519 keypair");
            println!("  Private key length: {} bytes", 
                match private_key {
                    rustls_pki_types::PrivateKeyDer::Pkcs8(pkcs8) => pkcs8.secret_pkcs8_der().len(),
                    _ => 0,
                }
            );
            println!("  Public key length: {} bytes", public_key.len());
        },
        Err(e) => println!("✗ Failed to generate keypair: {}", e),
    }
    
    // Example 8: Create self-signed certificate (not implemented yet)
    println!("\n8. Create Self-Signed Certificate");
    match TlsUtils::create_self_signed_certificate("example.com", &rustls_pki_types::PrivateKeyDer::Pkcs8(
        rustls_pki_types::PrivatePkcs8KeyDer::from(vec![0u8; 100])
    ), 365) {
        Ok(cert) => {
            println!("✓ Created self-signed certificate");
            println!("  Certificate length: {} bytes", cert.as_ref().len());
        },
        Err(e) => println!("✗ Failed to create certificate: {}", e),
    }
    
    println!("\n=========================");
    println!("Example completed!");
    
    Ok(())
}
