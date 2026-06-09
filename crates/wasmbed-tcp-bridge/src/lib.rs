// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

//! TCP Bridge for Renode firmware communication
//! This bridge exposes a TCP connection to the firmware via shared memory or syscalls

use std::net::{TcpStream, TcpListener};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// TCP Bridge that connects firmware to gateway
#[derive(Debug)]
pub struct TcpBridge {
    gateway_endpoint: String,
    bridge_port: u16,
    connection: Arc<Mutex<Option<TcpStream>>>,
}

impl TcpBridge {
    /// Create a new TCP bridge
    pub fn new(gateway_endpoint: String, bridge_port: u16) -> Self {
        Self {
            gateway_endpoint,
            bridge_port,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the bridge server
    /// This creates a local TCP server that the firmware can connect to
    /// The bridge then forwards data to/from the gateway
    pub fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let gateway_endpoint = self.gateway_endpoint.clone();
        let connection = self.connection.clone();
        let bridge_port = self.bridge_port;

        // Start bridge server in background thread
        thread::spawn(move || {
            let listener = match TcpListener::bind(format!("127.0.0.1:{}", bridge_port)) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Failed to bind bridge server on port {}: {}", bridge_port, e);
                    return;
                }
            };

            println!("TCP Bridge listening on 127.0.0.1:{}", bridge_port);

            for stream in listener.incoming() {
                match stream {
                    Ok(firmware_stream) => {
                        println!("Firmware connected to bridge");
                        
                        // Connect to gateway
                        match TcpStream::connect(&gateway_endpoint) {
                            Ok(gateway_stream) => {
                                println!("Bridge connected to gateway: {}", gateway_endpoint);
                                
                                // Store connection
                                *connection.lock().unwrap() = Some(gateway_stream.try_clone().unwrap());
                                
                                // Forward data bidirectionally
                                let gateway_clone = gateway_stream.try_clone().unwrap();
                                let firmware_clone = firmware_stream.try_clone().unwrap();
                                
                                // Simple forwarding (can be improved with proper async handling)
                                thread::spawn(move || {
                                    let mut buf = [0u8; 4096];
                                    let mut gateway = gateway_clone;
                                    let mut firmware = firmware_clone;
                                    loop {
                                        match firmware.read(&mut buf) {
                                            Ok(0) => break,
                                            Ok(n) => {
                                                if let Err(e) = gateway.write_all(&buf[..n]) {
                                                    eprintln!("Error forwarding to gateway: {}", e);
                                                    break;
                                                }
                                                gateway.flush().ok();
                                            }
                                            Err(e) => {
                                                eprintln!("Error reading from firmware: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                });
                                
                                thread::spawn(move || {
                                    let mut buf = [0u8; 4096];
                                    let mut gateway = gateway_stream;
                                    let mut firmware = firmware_stream;
                                    loop {
                                        match gateway.read(&mut buf) {
                                            Ok(0) => break,
                                            Ok(n) => {
                                                if let Err(e) = firmware.write_all(&buf[..n]) {
                                                    eprintln!("Error forwarding to firmware: {}", e);
                                                    break;
                                                }
                                                firmware.flush().ok();
                                            }
                                            Err(e) => {
                                                eprintln!("Error reading from gateway: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("Failed to connect to gateway {}: {}", gateway_endpoint, e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error accepting firmware connection: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Get the bridge endpoint that firmware should connect to
    pub fn bridge_endpoint(&self) -> String {
        format!("127.0.0.1:{}", self.bridge_port)
    }
}

