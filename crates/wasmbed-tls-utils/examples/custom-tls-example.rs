use wasmbed_tls_utils::{TlsUtils, TlsServer, TlsClient};
use std::net::SocketAddr;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    println!("🔐 Custom TLS Library Example");
    println!("=============================");
    
    // Parse certificates
    let server_key_bytes = std::fs::read("certs/server-key-pkcs8.pem")?;
    let server_cert_bytes = std::fs::read("certs/server-cert.pem")?;
    let ca_cert_bytes = std::fs::read("certs/ca-cert.pem")?;
    
    let server_key = match TlsUtils::parse_private_key(&server_key_bytes)? {
        rustls_pki_types::PrivateKeyDer::Pkcs8(pkcs8) => pkcs8,
        _ => return Err("Only PKCS8 private keys are supported".into()),
    };
    
    let server_cert = TlsUtils::parse_certificate(&server_cert_bytes)?;
    let ca_cert = TlsUtils::parse_certificates(&ca_cert_bytes)?
        .into_iter()
        .next()
        .ok_or("No CA certificate found")?;
    
    println!("✅ Parsed all certificates successfully");
    
    // Start TLS server in background
    let bind_addr: SocketAddr = "127.0.0.1:8443".parse()?;
    let server = TlsServer::new(bind_addr, server_cert.clone(), server_key.clone_key(), ca_cert.clone());
    
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });
    
    // Wait for server to start
    sleep(Duration::from_millis(100)).await;
    
    // Create TLS client
    let client = TlsClient::new(
        bind_addr,
        Some(server_cert),
        Some(server_key.clone_key()),
        ca_cert,
    );
    
    println!("🔗 Connecting to TLS server...");
    let mut connection = client.connect().await?;
    println!("✅ Connected to TLS server");
    
    // Send some data
    let test_data = b"Hello, Custom TLS!";
    connection.write(test_data).await?;
    println!("📤 Sent: {}", String::from_utf8_lossy(test_data));
    
    // Read response
    let mut buffer = [0; 1024];
    let n = connection.read(&mut buffer).await?;
    let response = String::from_utf8_lossy(&buffer[..n]);
    println!("📥 Received: {}", response);
    
    // Close connection
    connection.close().await?;
    println!("🔌 Connection closed");
    
    println!("\n🎉 Custom TLS Example Complete!");
    Ok(())
}
