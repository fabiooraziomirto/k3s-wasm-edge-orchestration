// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::error::{WasmResult, WasmRuntimeError};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::Aead;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Security manager for handling encryption, authentication, and key management
pub struct SecurityManager {
    /// Encryption keys
    encryption_keys: HashMap<String, Vec<u8>>,
    /// MAC keys
    mac_keys: HashMap<String, Vec<u8>>,
    /// Session keys
    session_keys: HashMap<String, SessionKey>,
    /// Device certificates
    certificates: HashMap<String, Certificate>,
    /// Nonce counters
    nonce_counters: HashMap<String, u64>,
}

/// Session key for secure communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKey {
    pub key_id: String,
    pub key: Vec<u8>,
    pub created: u64,
    pub expires: Option<u64>,
    pub algorithm: EncryptionAlgorithm,
}

/// Certificate for device authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub cert_id: String,
    pub certificate: Vec<u8>,
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
    pub created: u64,
    pub expires: u64,
    pub issuer: String,
    pub subject: String,
}

/// Encryption algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    Aes128Gcm,
    Aes256Gcm,
    ChaCha20Poly1305,
}

/// Security operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityResult {
    pub success: bool,
    pub data: Option<Vec<u8>>,
    pub error_message: Option<String>,
}

impl SecurityManager {
    /// Create a new security manager
    pub fn new() -> Self {
        Self {
            encryption_keys: HashMap::new(),
            mac_keys: HashMap::new(),
            session_keys: HashMap::new(),
            certificates: HashMap::new(),
            nonce_counters: HashMap::new(),
        }
    }

    /// Generate a new encryption key
    pub fn generate_encryption_key(&mut self, key_id: String, key_size: usize) -> WasmResult<Vec<u8>> {
        let mut key = vec![0u8; key_size];
        // In real implementation, use secure random number generator
        for byte in &mut key {
            *byte = rand::random();
        }

        self.encryption_keys.insert(key_id.clone(), key.clone());
        
        tracing::info!("Generated encryption key: {}", key_id);
        Ok(key)
    }

    /// Generate a new MAC key
    pub fn generate_mac_key(&mut self, key_id: String, key_size: usize) -> WasmResult<Vec<u8>> {
        let mut key = vec![0u8; key_size];
        // In real implementation, use secure random number generator
        for byte in &mut key {
            *byte = rand::random();
        }

        self.mac_keys.insert(key_id.clone(), key.clone());
        
        tracing::info!("Generated MAC key: {}", key_id);
        Ok(key)
    }

    /// Generate a new session key
    pub fn generate_session_key(&mut self, key_id: String, algorithm: EncryptionAlgorithm) -> WasmResult<SessionKey> {
        let key_size = match algorithm {
            EncryptionAlgorithm::Aes128Gcm => 16,
            EncryptionAlgorithm::Aes256Gcm => 32,
            EncryptionAlgorithm::ChaCha20Poly1305 => 32,
        };

        let mut key = vec![0u8; key_size];
        // In real implementation, use secure random number generator
        for byte in &mut key {
            *byte = rand::random();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let session_key = SessionKey {
            key_id: key_id.clone(),
            key,
            created: now,
            expires: Some(now + 3600), // 1 hour expiration
            algorithm,
        };

        self.session_keys.insert(key_id.clone(), session_key.clone());
        
        tracing::info!("Generated session key: {}", key_id);
        Ok(session_key)
    }

    /// Encrypt data using AES-256-GCM
    pub fn encrypt_data(&mut self, key_id: &str, data: &[u8]) -> WasmResult<SecurityResult> {
        let key = self.encryption_keys.get(key_id)
            .ok_or_else(|| WasmRuntimeError::SecurityError(
                format!("Encryption key {} not found", key_id)
            ))?;

        // Create cipher
        let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from_slice(key));

        // Generate nonce
        let nonce = self.generate_nonce(key_id)?;
        let nonce_bytes = Nonce::from_slice(&nonce);

        // Encrypt data
        match cipher.encrypt(nonce_bytes, data) {
            Ok(ciphertext) => {
                // Combine nonce and ciphertext
                let mut result = nonce;
                result.extend_from_slice(ciphertext.as_ref());
                
                Ok(SecurityResult {
                    success: true,
                    data: Some(result),
                    error_message: None,
                })
            }
            Err(e) => {
                Ok(SecurityResult {
                    success: false,
                    data: None,
                    error_message: Some(format!("Encryption failed: {}", e)),
                })
            }
        }
    }

    /// Decrypt data using AES-256-GCM
    pub fn decrypt_data(&mut self, key_id: &str, encrypted_data: &[u8]) -> WasmResult<SecurityResult> {
        if encrypted_data.len() < 12 {
            return Ok(SecurityResult {
                success: false,
                data: None,
                error_message: Some("Encrypted data too short".to_string()),
            });
        }

        let key = self.encryption_keys.get(key_id)
            .ok_or_else(|| WasmRuntimeError::SecurityError(
                format!("Decryption key {} not found", key_id)
            ))?;

        // Create cipher
        let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from_slice(key));

        // Extract nonce and ciphertext
        let nonce = &encrypted_data[0..12];
        let ciphertext = &encrypted_data[12..];
        let nonce_bytes = Nonce::from_slice(nonce);

        // Decrypt data
        match cipher.decrypt(nonce_bytes, ciphertext) {
            Ok(plaintext) => {
                Ok(SecurityResult {
                    success: true,
                    data: Some(plaintext.to_vec()),
                    error_message: None,
                })
            }
            Err(e) => {
                Ok(SecurityResult {
                    success: false,
                    data: None,
                    error_message: Some(format!("Decryption failed: {}", e)),
                })
            }
        }
    }

    /// Generate MAC using HMAC-SHA256
    pub fn generate_mac(&mut self, key_id: &str, data: &[u8]) -> WasmResult<SecurityResult> {
        let key = self.mac_keys.get(key_id)
            .ok_or_else(|| WasmRuntimeError::SecurityError(
                format!("MAC key {} not found", key_id)
            ))?;

        // Create HMAC
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
            .map_err(|e| WasmRuntimeError::SecurityError(
                format!("Failed to create HMAC: {}", e)
            ))?;

        // Update with data
        mac.update(data);

        // Generate MAC
        let mac_bytes = mac.finalize().into_bytes().to_vec();

        Ok(SecurityResult {
            success: true,
            data: Some(mac_bytes),
            error_message: None,
        })
    }

    /// Verify MAC using HMAC-SHA256
    pub fn verify_mac(&mut self, key_id: &str, data: &[u8], mac: &[u8]) -> WasmResult<SecurityResult> {
        let key = self.mac_keys.get(key_id)
            .ok_or_else(|| WasmRuntimeError::SecurityError(
                format!("MAC key {} not found", key_id)
            ))?;

        // Create HMAC
        let mut hmac = <Hmac<Sha256> as hmac::Mac>::new_from_slice(key)
            .map_err(|e| WasmRuntimeError::SecurityError(
                format!("Failed to create HMAC: {}", e)
            ))?;

        // Update with data
        hmac.update(data);

        // Verify MAC
        match hmac.verify_slice(mac) {
            Ok(_) => {
                Ok(SecurityResult {
                    success: true,
                    data: None,
                    error_message: None,
                })
            }
            Err(_) => {
                Ok(SecurityResult {
                    success: false,
                    data: None,
                    error_message: Some("MAC verification failed".to_string()),
                })
            }
        }
    }

    /// Generate nonce for encryption
    fn generate_nonce(&mut self, key_id: &str) -> WasmResult<Vec<u8>> {
        let counter = self.nonce_counters.entry(key_id.to_string()).or_insert(0);
        *counter += 1;

        // Generate nonce from counter (in real implementation, use random nonce)
        let mut nonce = vec![0u8; 12];
        nonce[0..8].copy_from_slice(&counter.to_le_bytes());
        
        Ok(nonce)
    }

    /// Load certificate
    pub fn load_certificate(&mut self, cert_id: String, certificate: Vec<u8>, private_key: Vec<u8>) -> WasmResult<()> {
        // In real implementation, parse X.509 certificate
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let cert = Certificate {
            cert_id: cert_id.clone(),
            certificate: certificate.clone(),
            private_key,
            public_key: certificate, // Simplified
            created: now,
            expires: now + 365 * 24 * 3600, // 1 year
            issuer: "Wasmbed CA".to_string(),
            subject: cert_id.clone(),
        };

        self.certificates.insert(cert_id.clone(), cert);
        
        tracing::info!("Loaded certificate: {}", cert_id);
        Ok(())
    }

    /// Verify certificate
    pub fn verify_certificate(&self, cert_id: &str) -> WasmResult<SecurityResult> {
        let cert = self.certificates.get(cert_id)
            .ok_or_else(|| WasmRuntimeError::SecurityError(
                format!("Certificate {} not found", cert_id)
            ))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now > cert.expires {
            return Ok(SecurityResult {
                success: false,
                data: None,
                error_message: Some("Certificate expired".to_string()),
            });
        }

        // In real implementation, verify certificate signature and chain
        Ok(SecurityResult {
            success: true,
            data: None,
            error_message: None,
        })
    }

    /// Get encryption key
    pub fn get_encryption_key(&self, key_id: &str) -> Option<&Vec<u8>> {
        self.encryption_keys.get(key_id)
    }

    /// Get MAC key
    pub fn get_mac_key(&self, key_id: &str) -> Option<&Vec<u8>> {
        self.mac_keys.get(key_id)
    }

    /// Get session key
    pub fn get_session_key(&self, key_id: &str) -> Option<&SessionKey> {
        self.session_keys.get(key_id)
    }

    /// Get certificate
    pub fn get_certificate(&self, cert_id: &str) -> Option<&Certificate> {
        self.certificates.get(cert_id)
    }

    /// List all keys
    pub fn list_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        keys.extend(self.encryption_keys.keys().cloned());
        keys.extend(self.mac_keys.keys().cloned());
        keys.extend(self.session_keys.keys().cloned());
        keys
    }

    /// List all certificates
    pub fn list_certificates(&self) -> Vec<String> {
        self.certificates.keys().cloned().collect()
    }

    /// Remove key
    pub fn remove_key(&mut self, key_id: &str) -> bool {
        let mut removed = false;
        removed |= self.encryption_keys.remove(key_id).is_some();
        removed |= self.mac_keys.remove(key_id).is_some();
        removed |= self.session_keys.remove(key_id).is_some();
        removed
    }

    /// Remove certificate
    pub fn remove_certificate(&mut self, cert_id: &str) -> bool {
        self.certificates.remove(cert_id).is_some()
    }

    /// Clear all keys and certificates
    pub fn clear_all(&mut self) {
        self.encryption_keys.clear();
        self.mac_keys.clear();
        self.session_keys.clear();
        self.certificates.clear();
        self.nonce_counters.clear();
    }
}
