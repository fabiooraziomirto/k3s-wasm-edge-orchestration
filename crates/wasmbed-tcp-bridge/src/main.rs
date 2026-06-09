// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::env;
use wasmbed_tcp_bridge::TcpBridge;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 3 {
        eprintln!("Usage: {} <gateway_endpoint> <bridge_port>", args[0]);
        eprintln!("Example: {} localhost:8443 40000", args[0]);
        std::process::exit(1);
    }
    
    let gateway_endpoint = args[1].clone();
    let bridge_port: u16 = args[2].parse().expect("Bridge port must be a valid u16");
    
    println!("Starting TCP Bridge:");
    println!("  Gateway endpoint: {}", gateway_endpoint);
    println!("  Bridge port: {}", bridge_port);
    
    let bridge = TcpBridge::new(gateway_endpoint, bridge_port);
    
    if let Err(e) = bridge.start() {
        eprintln!("Failed to start TCP bridge: {}", e);
        std::process::exit(1);
    }
    
    println!("TCP Bridge started successfully. Listening on 127.0.0.1:{}", bridge_port);
    println!("Press Ctrl+C to stop.");
    
    // Keep the process running
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

