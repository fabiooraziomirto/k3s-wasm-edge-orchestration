use serde_json;
use wasmbed_k8s_resource::ApplicationPhase;

fn main() {
    // Test deserializing "Deploying" string
    let phase_json = r#""Deploying""#;
    match serde_json::from_str::<ApplicationPhase>(phase_json) {
        Ok(phase) => println!("✅ Phase deserialized successfully: {:?}", phase),
        Err(e) => println!("❌ Error deserializing phase: {}", e),
    }
    
    // Test deserializing full status object
    let status_json = r#"{"phase": "Deploying", "statistics": {}}"#;
    println!("\nTesting full status deserialization...");
    println!("JSON: {}", status_json);
}

