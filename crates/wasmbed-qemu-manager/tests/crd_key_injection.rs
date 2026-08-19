// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

//! The gateway resolves a TLS connection to a Device CRD by public key
//! (`Device::find` compares `spec.publicKey` with `PublicKey::to_base64()`).
//! If the emulated device does not receive its own key, every device in the
//! fleet enrolls with the same identity and collapses onto a single session.
//!
//! This test drives `build_resc_script` against a stub `kubectl` that serves a
//! distinct `spec.publicKey` per device, then decodes the Renode memory writes
//! back into bytes and checks each device gets exactly its own key.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use wasmbed_qemu_manager::{McuType, QemuDevice, QemuDeviceStatus, RenodeManager};

/// Install a fake `kubectl` on PATH that echoes a per-device public key.
/// The key is `<device_id>` padded to 32 bytes, url-safe base64 encoded.
fn install_stub_kubectl() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wasmbed-stub-kubectl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = dir.join("kubectl");
    std::fs::write(
        &script,
        r#"#!/bin/sh
# args: get devices.wasmbed.github.io <device_id> -n wasmbed -o jsonpath=...
device_id="$3"
printf '%s' "$device_id" | python3 -c "
import sys, base64
raw = sys.stdin.buffer.read().ljust(32, b'.')[:32]
sys.stdout.write(base64.urlsafe_b64encode(raw).decode().rstrip('='))
"
"#,
    )
    .expect("write stub");
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", dir.display(), path));
    dir
}

fn device(id: &str) -> QemuDevice {
    QemuDevice {
        id: id.to_string(),
        name: id.to_string(),
        architecture: "ARM_CORTEX_M7".to_string(),
        device_type: "MCU".to_string(),
        mcu_type: McuType::Stm32F746gDisco,
        status: QemuDeviceStatus::Stopped,
        process_id: None,
        endpoint: "127.0.0.1:3000".to_string(),
        gateway_endpoint: Some("192.168.1.1:30443".to_string()),
        wasm_runtime: None,
    }
}

/// Decode the length-prefixed blob written at `base` back into bytes.
fn decode_blob(script: &str, base: u32) -> Vec<u8> {
    let mut words = std::collections::BTreeMap::new();
    for line in script.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 4 && parts[0] == "sysbus" && parts[1] == "WriteDoubleWord" {
            let addr = u32::from_str_radix(parts[2].trim_start_matches("0x"), 16).unwrap();
            let value = u32::from_str_radix(parts[3].trim_start_matches("0x"), 16).unwrap();
            words.insert(addr, value);
        }
    }
    let len = *words.get(&base).expect("length word") as usize;
    let mut bytes = Vec::with_capacity(len);
    let mut addr = base + 4;
    while bytes.len() < len {
        let word = *words.get(&addr).expect("data word");
        bytes.extend_from_slice(&word.to_le_bytes());
        addr += 4;
    }
    bytes.truncate(len);
    bytes
}

#[test]
fn each_device_is_given_its_own_crd_public_key() {
    let _stub = install_stub_kubectl();
    let manager = RenodeManager::new("renode".to_string(), 30000);
    let ids = ["fleet-device-1", "fleet-device-2", "fleet-device-3"];

    let mut injected = Vec::new();
    for id in ids {
        let script = manager_build(&manager, id);
        // 0x20002000 = DEVICE_KEY_ADDR in zephyr-app/src/wasmbed_protocol.c
        let key = decode_blob(&script, 0x2000_2000);
        let expected = {
            let mut raw = id.as_bytes().to_vec();
            raw.resize(32, b'.');
            raw
        };
        assert_eq!(key, expected, "device {id} must enroll with its own CRD key");

        // What the gateway will compare against spec.publicKey.
        let b64 = URL_SAFE_NO_PAD.encode(&key);
        assert_eq!(b64, URL_SAFE_NO_PAD.encode(&expected));

        // The endpoint injection must keep working alongside the key.
        let endpoint = decode_blob(&script, 0x2000_1000);
        assert_eq!(String::from_utf8(endpoint).unwrap(), "192.168.1.1:30443");

        // The firmware's shared static fallback key (0xAB repeated) must not appear.
        assert_ne!(key, vec![0xABu8; 32]);
        injected.push(key);
    }

    injected.sort();
    injected.dedup();
    assert_eq!(injected.len(), ids.len(), "keys must be distinct across the fleet");
}

fn manager_build(manager: &RenodeManager, id: &str) -> String {
    manager
        .build_resc_script_for_test(&device(id), id, "192.168.1.1:30443", "/firmware/zephyr.elf")
        .expect("resc script")
}
