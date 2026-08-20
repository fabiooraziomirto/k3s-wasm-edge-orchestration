// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

//! Device identity: the ECDSA P-256 key pair a device is provisioned with, used
//! both for its TLS client certificate and to prove possession of the public key
//! it announces during enrollment.

use anyhow::{Context, Result};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

/// Domain separation tag; must match the gateway's `POP_CONTEXT`.
const POP_CONTEXT: &[u8] = b"wasmbed-pop-v1";

/// The fixed SubjectPublicKeyInfo header for an uncompressed prime256v1 point:
/// SEQUENCE { SEQUENCE { id-ecPublicKey, prime256v1 }, BIT STRING (66 bytes) }.
/// ring hands back the raw point, while the gateway and `Device.spec.publicKey`
/// both speak SPKI, so wrap one into the other.
const P256_SPKI_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
    0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

pub struct DeviceIdentity {
    key_pair: EcdsaKeyPair,
    spki: Vec<u8>,
    rng: SystemRandom,
}

impl DeviceIdentity {
    /// Load a PKCS#8 DER private key as provisioned by `wasmbed-cert-tool`.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self> {
        let rng = SystemRandom::new();
        let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8, &rng)
            .map_err(|_| anyhow::anyhow!("not a PKCS#8 ECDSA P-256 private key"))?;

        let point = {
            use ring::signature::KeyPair;
            key_pair.public_key().as_ref().to_vec()
        };
        anyhow::ensure!(
            point.len() == 65 && point[0] == 0x04,
            "expected an uncompressed P-256 point, got {} bytes",
            point.len()
        );

        let mut spki = P256_SPKI_PREFIX.to_vec();
        spki.extend_from_slice(&point);

        Ok(Self { key_pair, spki, rng })
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        let pkcs8 = std::fs::read(path)
            .with_context(|| format!("reading device private key from {}", path.display()))?;
        Self::from_pkcs8(&pkcs8)
    }

    /// The SubjectPublicKeyInfo announced during enrollment; matches
    /// `Device.spec.publicKey`.
    pub fn spki(&self) -> &[u8] {
        &self.spki
    }

    /// Sign the gateway's challenge nonce.
    pub fn sign_challenge(&self, nonce: &[u8]) -> Result<Vec<u8>> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(POP_CONTEXT);
        hasher.update(nonce);
        hasher.update(&self.spki);
        let transcript = hasher.finalize();

        self.key_pair
            .sign(&self.rng, &transcript)
            .map(|sig| sig.as_ref().to_vec())
            .map_err(|_| anyhow::anyhow!("signing the challenge failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-written SPKI prefix must produce exactly what a real generator
    /// emits, otherwise the gateway would look up a key the device never has.
    #[test]
    fn derived_spki_matches_a_generated_one() {
        let generated = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let identity = DeviceIdentity::from_pkcs8(&generated.serialize_der()).unwrap();

        assert_eq!(identity.spki(), generated.public_key_der().as_slice());
    }

    #[test]
    fn rejects_a_key_that_is_not_pkcs8_p256() {
        assert!(DeviceIdentity::from_pkcs8(&[0u8; 40]).is_err());
    }

    #[test]
    fn signatures_differ_per_nonce() {
        let generated = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let identity = DeviceIdentity::from_pkcs8(&generated.serialize_der()).unwrap();

        let a = identity.sign_challenge(&[0x01; 32]).unwrap();
        let b = identity.sign_challenge(&[0x02; 32]).unwrap();
        assert_ne!(a, b);
    }
}
