// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2026 Wasmbed contributors
//
// Energy-tracking probe: wasmbed-wasm-runtime (Wasmtime, fuel metering) is
// implemented and unit-tested but not wired into any running service in
// this workspace (see doc/energy-tracking-assessment.md, section 1). This
// binary exists purely to give the energy/sustainability validation work a
// REAL, running, Kepler-attributable pod that repeatedly executes a real
// WASM module with fuel metering enabled, so fuel consumed can be
// correlated against Kepler-observed wattage under an actual (not
// synthetic) computational load -- the same idle-vs-load methodology used
// to validate Kepler itself in doc/energy-tracking-validation.md section 4,
// but with a real Wasmtime invocation as the load instead of `openssl
// speed`.
//
// Cycles idle_secs / load_secs forever, printing one JSON line per phase
// transition to stdout so `kubectl logs` timestamps line up with
// Prometheus queries against the Kepler recording rules.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wasmbed_wasm_runtime::{DeviceArchitecture, RuntimeConfig, WasmRuntime};

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_millis()
}

// Real arithmetic work, not a no-op loop: sums 0..200_000 in a WASM local.
// At Wasmtime's ~1-fuel-per-simple-instruction accounting this consumes a
// few million fuel units per call -- a meaningful fraction of the MCU
// profile's 5_000_000 fuel budget (config.rs) without exhausting it.
const BURN_WAT: &str = r#"
(module
  (func (export "burn") (result i32)
    (local $i i32)
    (local $sum i32)
    (local.set $i (i32.const 0))
    (local.set $sum (i32.const 0))
    (block $done
      (loop $loop
        (br_if $done (i32.ge_u (local.get $i) (i32.const 200000)))
        (local.set $sum (i32.add (local.get $sum) (local.get $i)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
    (local.get $sum)
  )
)
"#;

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let idle_secs = env_u64("IDLE_SECS", 30);
    let load_secs = env_u64("LOAD_SECS", 30);

    let wasm_bytes = wat::parse_str(BURN_WAT)?;

    let config = RuntimeConfig::for_architecture(DeviceArchitecture::Mcu, "energy-probe".to_string());
    let mut runtime = WasmRuntime::new(config)?;
    runtime.load_module("burn-module", &wasm_bytes).await?;

    println!(
        "{{\"event\":\"probe_start\",\"ts_ms\":{},\"idle_secs\":{},\"load_secs\":{}}}",
        now_epoch_ms(),
        idle_secs,
        load_secs
    );

    loop {
        println!("{{\"event\":\"idle_start\",\"ts_ms\":{}}}", now_epoch_ms());
        tokio::time::sleep(Duration::from_secs(idle_secs)).await;
        println!("{{\"event\":\"idle_end\",\"ts_ms\":{}}}", now_epoch_ms());

        println!("{{\"event\":\"load_start\",\"ts_ms\":{}}}", now_epoch_ms());
        let load_deadline = Instant::now() + Duration::from_secs(load_secs);
        let mut total_fuel: u64 = 0;
        let mut total_calls: u64 = 0;
        let mut errors: u64 = 0;

        while Instant::now() < load_deadline {
            match runtime.create_instance("burn-module", None).await {
                Ok(instance_id) => {
                    if runtime.call_function(&instance_id, "burn", &[]).await.is_ok() {
                        if let Ok(info) = runtime.get_instance_info(&instance_id) {
                            total_fuel += info.fuel_consumed;
                            total_calls += 1;
                        }
                    } else {
                        errors += 1;
                    }
                    let _ = runtime.remove_instance(&instance_id);
                }
                Err(_) => errors += 1,
            }
        }

        println!(
            "{{\"event\":\"load_end\",\"ts_ms\":{},\"total_calls\":{},\"total_fuel_consumed\":{},\"errors\":{},\"fuel_per_call_avg\":{:.1}}}",
            now_epoch_ms(),
            total_calls,
            total_fuel,
            errors,
            total_fuel as f64 / total_calls.max(1) as f64
        );
    }
}
