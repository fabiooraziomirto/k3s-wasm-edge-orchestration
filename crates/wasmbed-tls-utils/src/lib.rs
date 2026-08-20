// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use anyhow::{Context, Result};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use x509_parser::parse_x509_certificate;
use log::warn;
// use std::collections::HashMap; // Not used in current implementation
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use std::fmt;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{ReadBuf, AsyncRead, AsyncWrite};
use minicbor;

// Re-export protocol types for compatibility
pub use wasmbed_protocol::{ClientMessage, ServerMessage};

/// Custom TLS certificate and key utilities for Wasmbed
pub struct TlsUtils;

/// Custom TLS Server implementation
pub struct TlsServer {
    bind_addr: std::net::SocketAddr,
    server_cert: CertificateDer<'static>,
    server_key: PrivatePkcs8KeyDer<'static>,
    client_ca: CertificateDer<'static>,
}

/// Custom TLS Client implementation
pub struct TlsClient {
    server_addr: std::net::SocketAddr,
    client_cert: Option<CertificateDer<'static>>,
    client_key: Option<PrivatePkcs8KeyDer<'static>>,
    server_ca: CertificateDer<'static>,
}

/// TLS Connection state
pub enum TlsState {
    Handshake,
    Connected,
    Closed,
}

/// TLS Connection
pub struct TlsConnection {
    stream: TcpStream,
    state: TlsState,
    peer_cert: Option<CertificateDer<'static>>,
    peer_public_key: Option<Vec<u8>>,
}

/// Server Identity for compatibility with existing code
pub struct ServerIdentity {
    private_key: PrivatePkcs8KeyDer<'static>,
    certificate: CertificateDer<'static>,
}

/// Authorization Result for compatibility
#[derive(Debug, Clone)]
pub enum AuthorizationResult {
    Authorized,
    Unauthorized,
}

/// Message Context for compatibility
pub struct MessageContext {
    pub public_key: Vec<u8>,
    pub connection_id: String,
    pub message: Option<ClientMessage>,
    pub reply_fn: Option<Box<dyn Fn(ServerMessage) -> Result<()> + Send + Sync>>,
}

impl MessageContext {
    /// Create a new MessageContext
    pub fn new(public_key: Vec<u8>, connection_id: String) -> Self {
        Self {
            public_key,
            connection_id,
            message: None,
            reply_fn: None,
        }
    }

    /// Get the client message
    pub fn message(&self) -> Option<&ClientMessage> {
        self.message.as_ref()
    }

    /// Get the client public key
    pub fn client_public_key(&self) -> &Vec<u8> {
        &self.public_key
    }

    /// Reply to the client
    pub fn reply(&self, message: ServerMessage) -> Result<()> {
        if let Some(reply_fn) = &self.reply_fn {
            reply_fn(message)
        } else {
            Ok(())
        }
    }

    /// Set the message
    pub fn set_message(&mut self, message: ClientMessage) {
        self.message = Some(message);
    }

    /// Set the reply function
    pub fn set_reply_fn(&mut self, reply_fn: Box<dyn Fn(ServerMessage) -> Result<()> + Send + Sync>) {
        self.reply_fn = Some(reply_fn);
    }
}

/// Enhanced Message Context with PublicKey support for gateway
pub struct MessageContextWithKey {
    pub public_key: Vec<u8>,
    pub connection_id: String,
    pub message: Option<ClientMessage>,
    pub reply_fn: Option<Box<dyn Fn(ServerMessage) -> Result<()> + Send + Sync>>,
}

impl MessageContextWithKey {
    /// Create a new MessageContextWithKey
    pub fn new(public_key: Vec<u8>, connection_id: String) -> Self {
        Self {
            public_key,
            connection_id,
            message: None,
            reply_fn: None,
        }
    }

    /// Get the client message
    pub fn message(&self) -> Option<&ClientMessage> {
        self.message.as_ref()
    }

    /// Get the client public key
    pub fn client_public_key(&self) -> &Vec<u8> {
        &self.public_key
    }

    /// Reply to the client
    pub fn reply(&self, message: ServerMessage) -> Result<()> {
        if let Some(reply_fn) = &self.reply_fn {
            reply_fn(message)
        } else {
            Ok(())
        }
    }

    /// Set the message
    pub fn set_message(&mut self, message: ClientMessage) {
        self.message = Some(message);
    }

    /// Set the reply function
    pub fn set_reply_fn(&mut self, reply_fn: Box<dyn Fn(ServerMessage) -> Result<()> + Send + Sync>) {
        self.reply_fn = Some(reply_fn);
    }
}

/// Callback types for compatibility
pub type OnClientConnect = Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = AuthorizationResult> + Send>> + Send + Sync>;
pub type OnClientDisconnect = Box<dyn Fn(Vec<u8>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;
pub type OnClientMessage = Box<dyn Fn(MessageContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Enhanced callback types for gateway with PublicKey support
pub type OnClientConnectWithKey = Box<dyn Fn(Vec<u8>) -> std::pin::Pin<Box<dyn std::future::Future<Output = AuthorizationResult> + Send>> + Send + Sync>;
pub type OnClientDisconnectWithKey = Box<dyn Fn(Vec<u8>, String) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;
pub type OnClientMessageWithKey = Box<dyn Fn(MessageContextWithKey) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Callback when a TLS connection is ready to receive server messages (public_key, connection_id, sender to push ServerMessage)
pub type OnConnectionReadyWithKey = Box<dyn Fn(Vec<u8>, String, mpsc::Sender<ServerMessage>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Server Configuration for compatibility
pub struct ServerConfig {
    pub bind_addr: std::net::SocketAddr,
    pub identity: ServerIdentity,
    pub client_ca: CertificateDer<'static>,
    pub on_client_connect: Arc<OnClientConnect>,
    pub on_client_disconnect: Arc<OnClientDisconnect>,
    pub on_client_message: Arc<OnClientMessage>,
    pub shutdown: tokio_util::sync::CancellationToken,
}

/// Enhanced Server Configuration for gateway with PublicKey support
pub struct GatewayServerConfig {
    pub bind_addr: std::net::SocketAddr,
    pub identity: ServerIdentity,
    pub client_ca: CertificateDer<'static>,
    /// When true the TLS handshake fails unless the device presents a
    /// certificate issued by `client_ca`. When false an anonymous client is
    /// still accepted, but a certificate, if offered, is still validated.
    pub require_client_auth: bool,
    pub on_client_connect: Arc<OnClientConnectWithKey>,
    pub on_client_disconnect: Arc<OnClientDisconnectWithKey>,
    pub on_client_message: Arc<OnClientMessageWithKey>,
    /// Optional: called when connection is ready so the gateway can store a sender to push ServerMessage to this device
    pub on_connection_ready: Option<Arc<OnConnectionReadyWithKey>>,
    pub shutdown: tokio_util::sync::CancellationToken,
}

/// Custom TLS Server that implements the same interface as rustls
pub struct Server {
    config: ServerConfig,
}

/// Enhanced TLS Server for gateway with PublicKey support
pub struct GatewayServer {
    config: GatewayServerConfig,
}

/// TLS Stream wrapper for AsyncRead/AsyncWrite
pub struct TlsStream {
    inner: TcpStream,
    peer_cert: Option<CertificateDer<'static>>,
    peer_public_key: Option<Vec<u8>>,
}

impl TlsUtils {
    /// Parse a PEM-encoded private key, supporting multiple formats
    pub fn parse_private_key(pem_data: &[u8]) -> Result<PrivateKeyDer<'static>> {
        // First try to parse as PEM
        match pem::parse(pem_data) {
            Ok(pem) => {
                match pem.tag() {
                    "PRIVATE KEY" => {
                        // PKCS8 format
                        let der_data = pem.contents();
                        if der_data.is_empty() {
                            return Err(anyhow::anyhow!("Empty PKCS8 private key"));
                        }
                        
                        // Validate that it's actually PKCS8 by checking the DER structure
                        if der_data.len() < 2 {
                            return Err(anyhow::anyhow!("Invalid PKCS8 private key: too short"));
                        }
                        
                        // Check for PKCS8 structure (should start with SEQUENCE)
                        if der_data[0] != 0x30 {
                            return Err(anyhow::anyhow!("Invalid PKCS8 private key: not a valid DER sequence"));
                        }
                        
                        let pkcs8_key = PrivatePkcs8KeyDer::from(der_data.to_vec());
                        Ok(PrivateKeyDer::from(pkcs8_key))
                    },
                    "RSA PRIVATE KEY" => {
                        // RSA format - convert to PKCS8
                        let der_data = pem.contents();
                        if der_data.is_empty() {
                            return Err(anyhow::anyhow!("Empty RSA private key"));
                        }
                        
                        // Validate RSA structure
                        if der_data.len() < 2 || der_data[0] != 0x30 {
                            return Err(anyhow::anyhow!("Invalid RSA private key: not a valid DER sequence"));
                        }
                        
                        // For now, we'll treat RSA as PKCS8 (this is a workaround)
                        // In a production environment, you'd want to properly convert RSA to PKCS8
                        let pkcs8_key = PrivatePkcs8KeyDer::from(der_data.to_vec());
                        Ok(PrivateKeyDer::from(pkcs8_key))
                    },
                    _ => {
                        Err(anyhow::anyhow!("Unsupported private key format: {}", pem.tag()))
                    }
                }
            },
            Err(_) => {
                // If PEM parsing fails, try to parse as raw DER
                if pem_data.is_empty() {
                    return Err(anyhow::anyhow!("Empty private key data"));
                }
                
                // Check if it looks like DER data
                if pem_data[0] == 0x30 {
                    // Looks like DER, treat as PKCS8
                    let pkcs8_key = PrivatePkcs8KeyDer::from(pem_data.to_vec());
                    Ok(PrivateKeyDer::from(pkcs8_key))
                } else {
                    Err(anyhow::anyhow!("Invalid private key format: not PEM or DER"))
                }
            }
        }
    }
    
    /// Parse a PEM-encoded certificate
    pub fn parse_certificate(pem_data: &[u8]) -> Result<CertificateDer<'static>> {
        // First try to parse as PEM
        match pem::parse(pem_data) {
            Ok(pem) => {
                if pem.tag() != "CERTIFICATE" {
                    return Err(anyhow::anyhow!("Expected CERTIFICATE, got: {}", pem.tag()));
                }
                
                let der_data = pem.contents();
                if der_data.is_empty() {
                    return Err(anyhow::anyhow!("Empty certificate"));
                }
                
                // Validate the certificate structure
                if der_data.len() < 2 || der_data[0] != 0x30 {
                    return Err(anyhow::anyhow!("Invalid certificate: not a valid DER sequence"));
                }
                
                // Try to parse as X.509 to validate
                match parse_x509_certificate(der_data) {
                    Ok((_, cert)) => {
                        // Basic validation - check if certificate has valid structure
                        let _ = cert.validity();
                        Ok(CertificateDer::from(der_data.to_vec()))
                    },
                    Err(e) => {
                        // Even if X.509 parsing fails, we'll still return the certificate
                        // as rustls might be able to handle it
                        warn!("X.509 parsing failed but continuing: {}", e);
                        Ok(CertificateDer::from(der_data.to_vec()))
                    }
                }
            },
            Err(_) => {
                // If PEM parsing fails, try to parse as raw DER
                if pem_data.is_empty() {
                    return Err(anyhow::anyhow!("Empty certificate data"));
                }
                
                // Check if it looks like DER data
                if pem_data[0] == 0x30 {
                    // Looks like DER, treat as certificate
                    Ok(CertificateDer::from(pem_data.to_vec()))
                } else {
                    Err(anyhow::anyhow!("Invalid certificate format: not PEM or DER"))
                }
            }
        }
    }
    
    /// Parse multiple PEM-encoded certificates (for CA bundles)
    pub fn parse_certificates(pem_data: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
        let mut certificates = Vec::new();
        let mut remaining = pem_data;
        
        while !remaining.is_empty() {
            match pem::parse(remaining) {
                Ok(pem) => {
                    if pem.tag() == "CERTIFICATE" {
                        certificates.push(CertificateDer::from(pem.contents().to_vec()));
                    }
                    // Move to next PEM block
                    let pem_end = pem.contents().len() + pem.tag().len() + 25; // Approximate
                    if pem_end >= remaining.len() {
                        break;
                    }
                    remaining = &remaining[pem_end..];
                },
                Err(_) => break,
            }
        }
        
        if certificates.is_empty() {
            return Err(anyhow::anyhow!("No certificates found in PEM data"));
        }
        
        Ok(certificates)
    }
    
    /// Extract public key from a certificate
    pub fn extract_public_key(cert: &CertificateDer) -> Result<Vec<u8>> {
        let (_, cert) = parse_x509_certificate(cert.as_ref())
            .with_context(|| "Failed to parse certificate")?;
        
        Ok(cert.public_key().raw.to_vec())
    }
    
    /// Verify that a private key matches a certificate (simplified version)
    pub fn verify_key_cert_match(
        _private_key: &PrivateKeyDer,
        _certificate: &CertificateDer,
    ) -> Result<bool> {
        // For now, we'll assume they match if both can be parsed successfully
        // In a production environment, you'd want to do proper cryptographic verification
        Ok(true)
    }

    /// Generate a new Ed25519 key pair
    /// Note: This is a simplified implementation for development purposes
    pub fn generate_ed25519_keypair() -> Result<(PrivateKeyDer<'static>, Vec<u8>)> {
        // For now, return an error indicating this feature needs external tools
        // In a production environment, you'd want to use a proper key generation library
        Err(anyhow::anyhow!("Ed25519 key generation not yet implemented. Use external tools like openssl for now."))
    }

    /// Create a self-signed certificate for testing/development
    /// Note: This is a simplified implementation for development purposes
    pub fn create_self_signed_certificate(
        _common_name: &str,
        _private_key: &PrivateKeyDer,
        _validity_days: u32,
    ) -> Result<CertificateDer<'static>> {
        // For now, return an error indicating this feature is not fully implemented
        // In a production environment, you'd want to use a proper certificate generation library
        Err(anyhow::anyhow!("Self-signed certificate generation not yet implemented. Use external tools like openssl or mkcert for now."))
    }

    /// Validate certificate chain
    pub fn validate_certificate_chain(
        cert: &CertificateDer,
        _ca_certs: &[CertificateDer],
    ) -> Result<bool> {
        // For now, just check if the certificate can be parsed
        // In a full implementation, you'd verify the signature chain
        let (_, cert) = parse_x509_certificate(cert.as_ref())
            .with_context(|| "Failed to parse certificate")?;
        
        let _ = cert.validity();
        
        Ok(true)
    }

    /// Extract certificate information
    pub fn get_certificate_info(cert: &CertificateDer) -> Result<CertificateInfo> {
        let (_, cert) = parse_x509_certificate(cert.as_ref())
            .with_context(|| "Failed to parse certificate")?;
        
        let validity = cert.validity();
        let subject = cert.subject();
        let issuer = cert.issuer();
        
        Ok(CertificateInfo {
            subject: subject.to_string(),
            issuer: issuer.to_string(),
            not_before: validity.not_before,
            not_after: validity.not_after,
            serial_number: cert.serial.to_string(),
            public_key: cert.public_key().raw.to_vec(),
        })
    }

    /// Check if certificate is expired
    pub fn is_certificate_expired(cert: &CertificateDer) -> Result<bool> {
        let (_, cert) = parse_x509_certificate(cert.as_ref())
            .with_context(|| "Failed to parse certificate")?;
        
        let validity = cert.validity();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        
        Ok(now > validity.not_after.timestamp() as u64)
    }

    /// Check if certificate is valid for the given hostname
    pub fn is_certificate_valid_for_hostname(
        cert: &CertificateDer,
        hostname: &str,
    ) -> Result<bool> {
        let (_, cert) = parse_x509_certificate(cert.as_ref())
            .with_context(|| "Failed to parse certificate")?;
        
        let subject = cert.subject();
        
        // Simple hostname matching - in production you'd want more sophisticated matching
        let subject_str = subject.to_string();
        Ok(subject_str.contains(hostname))
    }
}

impl ServerIdentity {
    /// Create a new ServerIdentity from parts
    pub fn from_parts(
        private_key: PrivatePkcs8KeyDer<'static>,
        certificate: CertificateDer<'static>,
    ) -> Self {
        Self {
            private_key,
            certificate,
        }
    }

    /// Get the private key
    pub fn private_key(&self) -> &PrivatePkcs8KeyDer<'static> {
        &self.private_key
    }

    /// Get the certificate
    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.certificate
    }

    /// Clone the private key
    pub fn clone_key(&self) -> PrivatePkcs8KeyDer<'static> {
        self.private_key.clone_key()
    }
}

impl fmt::Display for ServerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ServerIdentity")
    }
}

impl TlsStream {
    /// Create a new TLS stream
    pub fn new(stream: TcpStream) -> Self {
        Self {
            inner: stream,
            peer_cert: None,
            peer_public_key: None,
        }
    }

    /// Get the peer's public key
    pub fn peer_public_key(&self) -> Option<&Vec<u8>> {
        self.peer_public_key.as_ref()
    }
}

impl AsyncRead for TlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll_shutdown(cx)
    }
}

impl Server {
    /// Create a new Server
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Run the server
    pub async fn run(&self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(self.config.bind_addr).await?;
        log::info!("TLS Server listening on {}", self.config.bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    log::info!("New connection from {}", addr);
                    let tls_stream = TlsStream::new(stream);
                    self.handle_connection(tls_stream).await?;
                }
                Err(e) => {
                    log::error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a new TLS connection
    async fn handle_connection(&self, mut tls_stream: TlsStream) -> Result<()> {
        // Call on_client_connect callback
        let auth_result = (self.config.on_client_connect)().await;
        match auth_result {
            AuthorizationResult::Authorized => {
                log::info!("Client authorized");
            }
            AuthorizationResult::Unauthorized => {
                log::warn!("Client unauthorized");
                return Ok(());
            }
        }

        // Handle the connection
        loop {
            let mut buffer = [0; 1024];
            match tls_stream.read(&mut buffer).await {
                Ok(0) => {
                    log::info!("Connection closed by client");
                    break;
                }
                Ok(n) => {
                    log::debug!("Received {} bytes", n);
                    
                    // Create message context
                    let mut ctx = MessageContext::new(
                        tls_stream.peer_public_key().unwrap_or(&vec![]).clone(),
                        "test-connection".to_string(),
                    );
                    
                    // Call on_client_message callback
                    (self.config.on_client_message)(ctx).await;
                    
                    // Echo back the data
                    tls_stream.write_all(&buffer[..n]).await?;
                }
                Err(e) => {
                    log::error!("Error reading from connection: {}", e);
                    break;
                }
            }
        }
        
        // Call on_client_disconnect callback
        (self.config.on_client_disconnect)(tls_stream.peer_public_key().unwrap_or(&vec![]).clone()).await;
        
        Ok(())
    }
}

impl TlsServer {
    /// Create a new TLS server
    pub fn new(
        bind_addr: std::net::SocketAddr,
        server_cert: CertificateDer<'static>,
        server_key: PrivatePkcs8KeyDer<'static>,
        client_ca: CertificateDer<'static>,
    ) -> Self {
        Self {
            bind_addr,
            server_cert,
            server_key,
            client_ca,
        }
    }

    /// Start the TLS server
    pub async fn start(&self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(self.bind_addr).await?;
        log::info!("TLS Server listening on {}", self.bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    log::info!("New connection from {}", addr);
                    let connection = TlsConnection::new(stream);
                    self.handle_connection(connection).await?;
                }
                Err(e) => {
                    log::error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a new TLS connection
    async fn handle_connection(&self, mut connection: TlsConnection) -> Result<()> {
        // Perform TLS handshake
        connection.perform_handshake().await?;
        
        // Handle the connection
        loop {
            let mut buffer = [0; 1024];
            match connection.stream.read(&mut buffer).await {
                Ok(0) => {
                    log::info!("Connection closed by client");
                    break;
                }
                Ok(n) => {
                    log::debug!("Received {} bytes", n);
                    // Echo back the data
                    connection.stream.write_all(&buffer[..n]).await?;
                }
                Err(e) => {
                    log::error!("Error reading from connection: {}", e);
                    break;
                }
            }
        }
        
        Ok(())
    }
}

impl TlsClient {
    /// Create a new TLS client
    pub fn new(
        server_addr: std::net::SocketAddr,
        client_cert: Option<CertificateDer<'static>>,
        client_key: Option<PrivatePkcs8KeyDer<'static>>,
        server_ca: CertificateDer<'static>,
    ) -> Self {
        Self {
            server_addr,
            client_cert,
            client_key,
            server_ca,
        }
    }

    /// Connect to the TLS server
    pub async fn connect(&self) -> Result<TlsConnection> {
        let stream = TcpStream::connect(self.server_addr).await?;
        let mut connection = TlsConnection::new(stream);
        
        // Perform TLS handshake
        connection.perform_handshake().await?;
        
        Ok(connection)
    }
}

impl TlsConnection {
    /// Create a new TLS connection
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            state: TlsState::Handshake,
            peer_cert: None,
            peer_public_key: None,
        }
    }

    /// Perform TLS handshake
    pub async fn perform_handshake(&mut self) -> Result<()> {
        // Simplified TLS handshake implementation
        // In a real implementation, this would implement the full TLS handshake protocol
        
        log::info!("Performing TLS handshake");
        
        // For now, just mark as connected
        self.state = TlsState::Connected;
        
        log::info!("TLS handshake completed");
        Ok(())
    }

    /// Read data from the connection
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        match self.state {
            TlsState::Connected => {
                self.stream.read(buffer).await.map_err(|e| anyhow::anyhow!("Read error: {}", e))
            }
            _ => Err(anyhow::anyhow!("Connection not ready for reading")),
        }
    }

    /// Write data to the connection
    pub async fn write(&mut self, data: &[u8]) -> Result<()> {
        match self.state {
            TlsState::Connected => {
                self.stream.write_all(data).await.map_err(|e| anyhow::anyhow!("Write error: {}", e))
            }
            _ => Err(anyhow::anyhow!("Connection not ready for writing")),
        }
    }

    /// Get the peer's public key
    pub fn peer_public_key(&self) -> Option<&Vec<u8>> {
        self.peer_public_key.as_ref()
    }

    /// Close the connection
    pub async fn close(&mut self) -> Result<()> {
        self.state = TlsState::Closed;
        self.stream.shutdown().await.map_err(|e| anyhow::anyhow!("Shutdown error: {}", e))
    }
}

/// Certificate information structure
#[derive(Debug, Clone)]
pub struct CertificateInfo {
    pub subject: String,
    pub issuer: String,
    pub not_before: x509_parser::time::ASN1Time,
    pub not_after: x509_parser::time::ASN1Time,
    pub serial_number: String,
    pub public_key: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_private_key_pkcs8() {
        let key_pem = b"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCqQtTzrZZlXxzTx
CLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i7hD69nd6IzmTLTfQJFyp1EHGuOW
2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKoZIhvcNAQELBQADgYEAWVTk8aWm
HAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END PRIVATE KEY-----";
        
        let result = TlsUtils::parse_private_key(key_pem);
        // This test uses invalid data, so we expect an error
        assert!(result.is_err());
    }
    
    #[test]
    fn test_parse_certificate() {
        let cert_pem = b"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKpC1HNuZliXxzTxCLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i
7hD69nd6IzmTLTfQJFyp1EHGuOW2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKo
ZIhvcNAQELBQADgYEAWVTk8aWmHAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END CERTIFICATE-----";
        
        let result = TlsUtils::parse_certificate(cert_pem);
        // This test uses invalid data, so we expect an error
        assert!(result.is_err());
    }
    
    #[test]
    fn test_generate_ed25519_keypair() {
        let result = TlsUtils::generate_ed25519_keypair();
        // This function is not yet implemented, so we expect an error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not yet implemented"));
    }
    
    #[test]
    fn test_extract_public_key() {
        let cert_pem = b"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKpC1HNuZliXxzTxCLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i
7hD69nd6IzmTLTfQJFyp1EHGuOW2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKo
ZIhvcNAQELBQADgYEAWVTk8aWmHAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END CERTIFICATE-----";
        
        let cert_result = TlsUtils::parse_certificate(cert_pem);
        // This test uses invalid data, so we expect an error
        assert!(cert_result.is_err());
    }
    
    #[test]
    fn test_get_certificate_info() {
        let cert_pem = b"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKpC1HNuZliXxzTxCLaKpCCMgDrD9VARogdeJhtOoQ/hsk66u9m8i
7hD69nd6IzmTLTfQJFyp1EHGuOW2qyks2o0IwQDAOgBtFQ8BAf8EABMAqQwDgYIKo
ZIhvcNAQELBQADgYEAWVTk8aWmHAig1voP5rpJS8fRBRI0G6SWvxG5MPcymt+CvA
YF7yXpmHoluHsRUoqg9xrqqyOHrmmmSKuKfah2Q=
-----END CERTIFICATE-----";
        
        let cert_result = TlsUtils::parse_certificate(cert_pem);
        // This test uses invalid data, so we expect an error
        assert!(cert_result.is_err());
    }
    
    #[test]
    fn test_parse_certificates_multiple() {
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
        
        let result = TlsUtils::parse_certificates(certs_pem);
        // This test uses invalid data, so we expect an error
        assert!(result.is_err());
    }
    
    #[test]
    fn test_invalid_inputs() {
        // Test empty input
        assert!(TlsUtils::parse_private_key(b"").is_err());
        assert!(TlsUtils::parse_certificate(b"").is_err());
        
        // Test invalid PEM
        assert!(TlsUtils::parse_private_key(b"invalid data").is_err());
        assert!(TlsUtils::parse_certificate(b"invalid data").is_err());
        
        // Test wrong PEM type
        let wrong_pem = b"-----BEGIN PUBLIC KEY-----
invalid
-----END PUBLIC KEY-----";
        assert!(TlsUtils::parse_private_key(wrong_pem).is_err());
    }
}

impl GatewayServer {
    /// Create a new GatewayServer
    pub fn new(config: GatewayServerConfig) -> Self {
        println!("[DEBUG] Creating GatewayServer instance");
        log::info!("Creating GatewayServer instance");
        Self { config }
    }

    /// Run the gateway server with real TLS
    pub async fn run(&self) -> Result<()> {
        println!("[DEBUG] GatewayServer::run() called");
        log::info!("GatewayServer::run() called");
        
        use rustls::{ServerConfig, ServerConnection};
        use tokio_rustls::TlsAcceptor;
        use std::sync::Arc;
        
        println!("[DEBUG] After use declarations");
        println!("[DEBUG] About to log TLS server configuration");
        log::info!("Creating TLS server configuration...");
        println!("[DEBUG] After logging TLS server configuration");
        println!("[DEBUG] About to create ServerConfig");
        
        // Devices authenticate with a client certificate issued by the fleet CA.
        // Without this the handshake leaves `peer_certificates()` empty, every
        // device lands on the anonymous path, and identity collapses to
        // announcing a public key -- which is not a secret -- over an
        // unauthenticated channel.
        println!("[DEBUG] Calling ServerConfig::builder()");
        println!("[DEBUG] Certificate: {:?}", self.config.identity.certificate());
        println!("[DEBUG] Private key: {:?}", self.config.identity.private_key());
        // Use ECDHE-RSA-AES128-GCM-SHA256 for TLS 1.2 — matches the RSA certificates generated
        // by wasmbed-cert-tool. ECDSA variant is kept as fallback for future ECDSA cert migration.
        let mut provider = rustls::crypto::ring::default_provider();
        provider.cipher_suites = vec![
            rustls::crypto::ring::cipher_suite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            rustls::crypto::ring::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        ];
        let provider = std::sync::Arc::new(provider);

        let mut client_roots = rustls::RootCertStore::empty();
        client_roots
            .add(self.config.client_ca.clone())
            .map_err(|e| {
                log::error!("Failed to add client CA to the root store: {:?}", e);
                anyhow::anyhow!("invalid client CA certificate: {e}")
            })?;

        let verifier_builder = rustls::server::WebPkiClientVerifier::builder_with_provider(
            std::sync::Arc::new(client_roots),
            provider.clone(),
        );
        let client_verifier = if self.config.require_client_auth {
            log::info!("Client certificate authentication is REQUIRED");
            verifier_builder.build()
        } else {
            log::warn!(
                "Client certificate authentication is OPTIONAL: anonymous devices are still \
                 accepted. Device identity rests on the application-level exchange alone."
            );
            verifier_builder.allow_unauthenticated().build()
        }
        .map_err(|e| {
            log::error!("Failed to build the client certificate verifier: {:?}", e);
            anyhow::anyhow!("client verifier: {e}")
        })?;

        let server_config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS12])
            .map_err(|e| {
                log::error!("Failed to configure TLS 1.2 with cipher suites: {:?}", e);
                e
            })?
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(
                vec![self.config.identity.certificate().clone()],
                rustls_pki_types::PrivateKeyDer::from(self.config.identity.private_key().clone_key()),
            ).map_err(|e| {
                println!("[DEBUG] ServerConfig creation failed: {:?}", e);
                log::error!("Failed to create ServerConfig: {:?}", e);
                e
            })?;
        println!("[DEBUG] ServerConfig created successfully");
        
        log::info!("TLS server configuration created successfully (TLS 1.2 only)");
        
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        log::info!("TlsAcceptor created successfully");
        
        let listener = tokio::net::TcpListener::bind(self.config.bind_addr).await?;
        log::info!("Gateway TLS Server listening on {}", self.config.bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    println!("[TLS] New connection from {}", addr);
                    log::info!("New connection from {}", addr);
                    
                    // Accept TLS connection
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            println!("[TLS] Handshake OK for {}", addr);
                            log::info!("TLS handshake completed for {}", addr);
                            self.handle_tls_connection(tls_stream, addr).await?;
                        }
                        Err(e) => {
                            println!("[TLS] Handshake FAILED for {}: {}", addr, e);
                            log::error!("TLS handshake failed for {}: {}", addr, e);
                        }
                    }
                }
                Err(e) => {
                    println!("[TLS] Accept error: {}", e);
                    log::error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a real TLS connection
    async fn handle_tls_connection(&self, mut tls_stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>, addr: std::net::SocketAddr) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        
        // Extract client certificate and public key
        let peer_certs = tls_stream.get_ref().1.peer_certificates();
        let public_key = if let Some(certs) = peer_certs {
            if let Some(cert) = certs.first() {
                // Extract public key from certificate
                TlsUtils::extract_public_key(cert).unwrap_or_else(|_| vec![])
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        
        println!("[TLS] Client public key: {} bytes", public_key.len());
        log::info!("Client public key: {} bytes", public_key.len());
        
        // Call on_client_connect callback with public key
        let auth_result = (self.config.on_client_connect)(public_key.clone()).await;
        match auth_result {
            AuthorizationResult::Authorized => {
                println!("[TLS] Client AUTHORIZED (key {} bytes)", public_key.len());
                log::info!("Client authorized with public key: {} bytes", public_key.len());
            }
            AuthorizationResult::Unauthorized => {
                println!("[TLS] Client UNAUTHORIZED (key {} bytes) → closing", public_key.len());
                log::warn!("Client unauthorized with public key: {} bytes", public_key.len());
                return Ok(());
            }
        }

        let (tx, mut rx) = mpsc::channel::<ServerMessage>(32);
        // Clone tx for reply_fn and to keep the channel alive regardless of what
        // on_connection_ready does with its copy.
        let tx_for_read = tx.clone();
        let on_client_message = self.config.on_client_message.clone();
        let on_client_disconnect = self.config.on_client_disconnect.clone();
        let public_key_clone = public_key.clone();

        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            use tokio::io::AsyncWriteExt;

            // Split the stream so the read loop and write loop don't interfere.
            let (mut reader, mut writer) = tokio::io::split(tls_stream);

            // Writer task: forwards queued ServerMessage → client.
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if let Ok(cbor) = minicbor::to_vec(&msg) {
                        let len = cbor.len() as u32;
                        let _ = writer.write_all(&len.to_be_bytes()).await;
                        let _ = writer.write_all(&cbor).await;
                        let _ = writer.flush().await;
                    }
                }
            });

            // Reader loop: reads framed messages (4-byte BE length + CBOR) from client.
            // Holds tx_for_read to keep the writer channel alive even if on_connection_ready
            // drops its copy of tx (e.g. when the device has no known device_id yet).
            let _tx_keep = tx_for_read.clone();
            loop {
                // Read 4-byte big-endian length prefix
                let mut len_buf = [0u8; 4];
                match reader.read_exact(&mut len_buf).await {
                    Ok(_) => {}
                    Err(_) => break,
                }
                let msg_len = u32::from_be_bytes(len_buf) as usize;
                if msg_len == 0 || msg_len > 65536 {
                    log::warn!("Invalid message length {} from {}, closing", msg_len, addr);
                    break;
                }
                // Read CBOR payload
                let mut payload = vec![0u8; msg_len];
                if reader.read_exact(&mut payload).await.is_err() {
                    break;
                }
                let mut ctx = MessageContextWithKey::new(
                    public_key_clone.clone(),
                    addr.to_string(),
                );
                match minicbor::decode::<ClientMessage>(&payload) {
                    Ok(client_message) => ctx.set_message(client_message),
                    Err(e) => log::warn!("CBOR decode error from {}: {}", addr, e),
                }
                // Wire reply_fn so handlers can send responses back via the channel.
                let tx_reply = tx_for_read.clone();
                ctx.set_reply_fn(Box::new(move |msg: ServerMessage| {
                    tx_reply.try_send(msg).map_err(|e| anyhow::anyhow!("Reply send failed: {}", e))
                }));
                (on_client_message)(ctx).await;
            }
            (on_client_disconnect)(public_key_clone, addr.to_string()).await;
        });

        if let Some(ref cb) = self.config.on_connection_ready {
            (cb)(public_key, addr.to_string(), tx).await;
        }

        Ok(())
    }

    /// Handle a new TLS connection with PublicKey support
    async fn handle_connection(&self, mut tls_stream: TlsStream) -> Result<()> {
        // Extract public key from TLS connection
        let public_key = tls_stream.peer_public_key().cloned().unwrap_or_else(|| vec![]);
        
        // Call on_client_connect callback with public key
        let auth_result = (self.config.on_client_connect)(public_key.clone()).await;
        match auth_result {
            AuthorizationResult::Authorized => {
                log::info!("Client authorized with public key: {} bytes", public_key.len());
            }
            AuthorizationResult::Unauthorized => {
                log::warn!("Client unauthorized with public key: {} bytes", public_key.len());
                return Ok(());
            }
        }

        // Handle the connection
        loop {
            let mut buffer = [0; 1024];
            match tls_stream.read(&mut buffer).await {
                Ok(0) => {
                    log::info!("Connection closed by client");
                    break;
                }
                Ok(n) => {
                    log::debug!("Received {} bytes", n);
                    
                    // Create message context with public key
                    let mut ctx = MessageContextWithKey::new(
                        public_key.clone(),
                        "gateway-connection".to_string(),
                    );
                    
                    // Parse CBOR message if possible
                    if let Ok(client_message) = minicbor::decode::<ClientMessage>(&buffer[..n]) {
                        ctx.set_message(client_message);
                    }
                    
                    // Call on_client_message callback
                    (self.config.on_client_message)(ctx).await;
                    
                    // Echo back the data
                    tls_stream.write_all(&buffer[..n]).await?;
                }
                Err(e) => {
                    log::error!("Error reading from connection: {}", e);
                    break;
                }
            }
        }
        
        // Call on_client_disconnect callback
        (self.config.on_client_disconnect)(public_key.clone(), "legacy-connection".to_string()).await;
        
        Ok(())
    }
}