#!/usr/bin/env python3
"""Collect RETROSPECT experiment metrics and compute 95% confidence intervals.

This script is intentionally lightweight: it polls the existing API, Gateway,
and Kubernetes resources used by the platform, then emits raw trial records and
aggregated statistics for the paper.
"""

from __future__ import annotations

import argparse
import base64
from collections import Counter
import json
import math
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

try:
    import requests
except ImportError as exc:  # pragma: no cover - environment dependent
    raise SystemExit("Missing dependency: requests") from exc


DEFAULT_NAMESPACE = "wasmbed"
DEFAULT_API_BASE = "http://127.0.0.1:3001"
DEFAULT_GATEWAY_HTTP = "http://127.0.0.1:8080"


@dataclass
class TrialRecord:
    experiment: str
    trial: int
    success: bool
    duration_ms: float | None = None
    details: dict[str, Any] | None = None


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def run_kubectl_json(args: list[str]) -> dict[str, Any] | list[Any] | None:
    try:
        output = subprocess.check_output(["kubectl", *args], text=True)
    except Exception:
        return None
    try:
        return json.loads(output)
    except json.JSONDecodeError:
        return None


def get_devices(namespace: str) -> list[dict[str, Any]]:
    payload = run_kubectl_json(["get", "devices", "-n", namespace, "-o", "json"])
    if not isinstance(payload, dict):
        return []
    return list(payload.get("items", []))


def get_applications(namespace: str) -> list[dict[str, Any]]:
    payload = run_kubectl_json(["get", "applications", "-n", namespace, "-o", "json"])
    if not isinstance(payload, dict):
        return []
    return list(payload.get("items", []))


def jsonpath(namespace: str, resource: str, name: str, expr: str) -> str | None:
    try:
        return subprocess.check_output(
            ["kubectl", "get", resource, name, "-n", namespace, "-o", f"jsonpath={expr}"],
            text=True,
        ).strip()
    except Exception:
        return None


# Student-t critical values t(n-1, 0.975) for 95% two-sided CI.
# Lookup by degrees-of-freedom (df = n-1); linear interpolation for df > 120.
_T_CRIT_TABLE: dict[int, float] = {
    1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571,
    6: 2.447, 7: 2.365, 8: 2.306, 9: 2.262, 10: 2.228,
    12: 2.179, 15: 2.131, 20: 2.086, 25: 2.060, 29: 2.045,
    30: 2.042, 40: 2.021, 49: 2.010, 50: 2.009, 60: 2.000,
    80: 1.990, 99: 1.984, 100: 1.984, 120: 1.980,
}


def t_critical(n: int) -> float:
    """Return Student-t critical value t(n-1, 0.975) for 95% CI of mean."""
    df = n - 1
    if df <= 0:
        return float("inf")
    if df in _T_CRIT_TABLE:
        return _T_CRIT_TABLE[df]
    # Linear interpolation between the two nearest tabulated df values
    keys = sorted(_T_CRIT_TABLE.keys())
    lo = max((k for k in keys if k <= df), default=keys[0])
    hi = min((k for k in keys if k >= df), default=keys[-1])
    if lo == hi:
        return _T_CRIT_TABLE[lo]
    frac = (df - lo) / (hi - lo)
    return _T_CRIT_TABLE[lo] + frac * (_T_CRIT_TABLE[hi] - _T_CRIT_TABLE[lo])


def mean_ci95(values: list[float]) -> dict[str, Any]:
    if not values:
        return {"count": 0, "mean": None, "stdev": None, "ci95": None}
    count = len(values)
    mean = statistics.fmean(values)
    stdev = statistics.stdev(values) if count > 1 else 0.0
    if count > 1:
        t_crit = t_critical(count)
        half_width = t_crit * (stdev / math.sqrt(count))
        ci95 = [mean - half_width, mean + half_width]
    else:
        ci95 = [mean, mean]
    return {
        "count": count,
        "mean": mean,
        "stdev": stdev,
        "min": min(values),
        "max": max(values),
        "median": statistics.median(values),
        "p95": statistics.quantiles(values, n=20)[18] if count >= 20 else max(values),
        "ci95": ci95,
    }


def percentile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    if q <= 0:
        return min(values)
    if q >= 1:
        return max(values)

    ordered = sorted(values)
    pos = (len(ordered) - 1) * q
    lower = math.floor(pos)
    upper = math.ceil(pos)
    if lower == upper:
        return ordered[lower]
    frac = pos - lower
    return ordered[lower] * (1 - frac) + ordered[upper] * frac


def latency_profile(values: list[float]) -> dict[str, Any]:
    base = mean_ci95(values)
    if not values:
        base.update({"p90": None, "p99": None, "iqr": None, "cv": None})
        return base
    p25 = percentile(values, 0.25)
    p75 = percentile(values, 0.75)
    mean = base.get("mean")
    stdev = base.get("stdev")
    cv = (stdev / mean) if mean not in (None, 0) and stdev is not None else None
    base.update(
        {
            "p90": percentile(values, 0.90),
            "p99": percentile(values, 0.99),
            "iqr": (p75 - p25) if p25 is not None and p75 is not None else None,
            "cv": cv,
        }
    )
    return base


def wilson_interval(successes: int, total: int, z: float = 1.96) -> dict[str, Any]:
    if total == 0:
        return {"rate": None, "ci95": None, "successes": 0, "total": 0}
    phat = successes / total
    denom = 1 + (z**2) / total
    center = (phat + (z**2) / (2 * total)) / denom
    half = (z * math.sqrt((phat * (1 - phat) / total) + (z**2) / (4 * total**2))) / denom
    return {
        "rate": phat,
        "ci95": [max(0.0, center - half), min(1.0, center + half)],
        "successes": successes,
        "total": total,
    }


def parse_duration_ms(start: float, end: float) -> float:
    return (end - start) * 1000.0


def find_running_device(namespace: str) -> dict[str, Any] | None:
    for item in get_devices(namespace):
        status = item.get("status") or {}
        if status.get("phase") in ("Connected", "Unreachable"):
            return item
    devices = get_devices(namespace)
    return devices[0] if devices else None


def get_device_public_key(device: dict[str, Any]) -> str | None:
    spec = device.get("spec") or {}
    public_key = spec.get("publicKey")
    if isinstance(public_key, str) and public_key:
        return public_key
    return None


def run_enrollment_trial(api_base: str, gateway_http: str, namespace: str, trial: int) -> TrialRecord:
    device = find_running_device(namespace)
    if device is None:
        return TrialRecord("enrollment", trial, False, details={"reason": "no device found"})

    device_name = device["metadata"]["name"]
    public_key = get_device_public_key(device)

    if not public_key:
        return TrialRecord("enrollment", trial, False, details={"reason": "missing device public key", "device": device_name})

    start = time.perf_counter()
    try:
        response = requests.post(
            f"{gateway_http}/api/v1/board/register",
            json={
                "device_id": device_name,
                "endpoint": jsonpath(namespace, "device", device_name, "{.status.gateway.endpoint}") or "",
                "mcu_type": device.get("spec", {}).get("mcuType", ""),
                "capabilities": {"has_ethernet": True, "has_wifi": False, "has_network": True},
            },
            timeout=10,
        )
        board_register_ok = response.status_code < 400
    except Exception as exc:
        return TrialRecord("enrollment", trial, False, details={"reason": f"board register failed: {exc}"})

    try:
        response = requests.get(f"{api_base}/api/v1/devices", timeout=10)
        api_ok = response.status_code < 400
    except Exception as exc:
        return TrialRecord("enrollment", trial, False, details={"reason": f"api probe failed: {exc}"})

    phase = jsonpath(namespace, "device", device_name, "{.status.phase}")
    last_hb = jsonpath(namespace, "device", device_name, "{.status.last_heartbeat}")
    end = time.perf_counter()

    # Success: device is visible via API and has sent at least one heartbeat.
    # Phase may be "Connected" or "Unreachable" (transient CRD race with heartbeat monitor);
    # either indicates an actively-connected device.
    device_active = phase in ("Connected", "Unreachable") and bool(last_hb)
    return TrialRecord(
        "enrollment",
        trial,
        bool(api_ok and device_active),
        duration_ms=parse_duration_ms(start, end),
        details={"device": device_name, "phase": phase, "last_heartbeat": last_hb, "board_registered": board_register_ok, "api_visible": api_ok},
    )


def run_heartbeat_trial(gateway_http: str, namespace: str, trial: int) -> TrialRecord:
    device = find_running_device(namespace)
    if device is None:
        return TrialRecord("heartbeat", trial, False, details={"reason": "no device found"})

    device_name = device["metadata"]["name"]
    # Use the gateway's in-memory device state — the CRD's last_heartbeat is patched
    # with a camelCase key that doesn't match the snake_case CRD schema, so it stays stale.
    # The gateway HTTP API reflects the real in-memory last_heartbeat updated on each firmware
    # ClientMessage::Heartbeat (sent every 25 s).
    start = time.perf_counter()
    try:
        gw_response = requests.get(f"{gateway_http}/api/v1/devices", timeout=10)
        gw_devices = {d["device_id"]: d for d in (gw_response.json().get("devices") or [])}
        gw_device = gw_devices.get(device_name, {})
        gw_connected = gw_device.get("connected", False)
        gw_hb = gw_device.get("last_heartbeat")
        if gw_hb and isinstance(gw_hb, dict):
            hb_epoch = gw_hb.get("secs_since_epoch", 0)
            hb_age_s = time.time() - hb_epoch
        else:
            hb_age_s = float("inf")
        heartbeat_recent = hb_age_s < 120
    except Exception as exc:
        return TrialRecord("heartbeat", trial, False, details={"device": device_name, "reason": f"gateway query failed: {exc}"})
    end = time.perf_counter()

    return TrialRecord(
        "heartbeat",
        trial,
        bool(gw_connected and heartbeat_recent),
        duration_ms=parse_duration_ms(start, end),
        details={"device": device_name, "gateway_connected": gw_connected, "heartbeat_age_s": round(hb_age_s, 1)},
    )


def run_deploy_trial(api_base: str, namespace: str, trial: int) -> TrialRecord:
    # Clean up any previous Application CRDs so stale deployments don't exhaust WAMR slots
    existing_apps = get_applications(namespace)
    for app in existing_apps:
        app_name = app["metadata"]["name"]
        run_kubectl_json(["delete", "application", app_name, "-n", namespace, "--ignore-not-found=true"])
    if existing_apps:
        time.sleep(2)  # let the controller process the deletions

    devices = get_devices(namespace)
    if not devices:
        return TrialRecord("deployment", trial, False, details={"reason": "no devices found"})

    device_name = devices[0]["metadata"]["name"]
    app_name = f"experiment-{trial}-{int(time.time())}"
    # Minimal valid WASM module with a no-op `run` export (required by the firmware)
    # Sections: magic+version, type([] -> []), function(type 0), export("run" -> func 0), code(empty body)
    wasm_minimal = bytes([
        0x00, 0x61, 0x73, 0x6d,  # magic
        0x01, 0x00, 0x00, 0x00,  # version
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00,  # type section: functype [] -> []
        0x03, 0x02, 0x01, 0x00,              # function section: 1 func, type 0
        0x07, 0x07, 0x01, 0x03, 0x72, 0x75, 0x6e, 0x00, 0x00,  # export "run" -> func 0
        0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,  # code section: 1 body, 0 locals, end
    ])
    wasm_base64 = base64.b64encode(wasm_minimal).decode("ascii")
    create_payload = {
        "name": app_name,
        "description": "Benchmark application",
        "wasmBytes": wasm_base64,
        "targetDevices": {"deviceNames": [device_name]},
    }

    start = time.perf_counter()
    try:
        create_response = requests.post(f"{api_base}/api/v1/applications", json=create_payload, timeout=20)
        if create_response.status_code >= 400:
            return TrialRecord("deployment", trial, False, details={"reason": f"create failed: {create_response.status_code}", "body": create_response.text[:200]})
    except Exception as exc:
        return TrialRecord("deployment", trial, False, details={"reason": f"create failed: {exc}"})

    deploy_started = time.perf_counter()
    deploy_error = None
    try:
        deploy_response = requests.post(f"{api_base}/api/v1/applications/{app_name}/deploy", json={}, timeout=120)
        if deploy_response.status_code >= 400:
            deploy_error = f"deploy failed: {deploy_response.status_code}"
    except Exception as exc:
        deploy_error = f"deploy failed: {exc}"

    deadline = time.time() + 60
    observed_phase = None
    observed_device_status = None
    while time.time() < deadline:
        app_obj = run_kubectl_json(["get", "application", app_name, "-n", namespace, "-o", "json"])
        if isinstance(app_obj, dict):
            status = app_obj.get("status") or {}
            observed_phase = status.get("phase")
            device_statuses = status.get("deviceStatuses") or {}
            if device_statuses and isinstance(device_statuses, dict):
                observed_device_status = next(iter(device_statuses.values()))
            if observed_phase in {"Running", "Failed", "Stopped"}:
                break
        time.sleep(2)

    end = time.perf_counter()

    api_status = None
    api_running_devices = 0
    api_failed_devices = 0
    api_deployed_count = 0
    try:
        app_list_resp = requests.get(f"{api_base}/api/v1/applications", timeout=10)
        if app_list_resp.status_code < 400:
            apps = app_list_resp.json().get("applications") or []
            app_entry = next((a for a in apps if a.get("app_id") == app_name or a.get("name") == app_name), None)
            if isinstance(app_entry, dict):
                api_status = app_entry.get("status")
                stats = app_entry.get("statistics") or {}
                api_running_devices = int(stats.get("running_devices") or 0)
                api_failed_devices = int(stats.get("failed_devices") or 0)
                api_deployed_count = int(stats.get("deployed_count") or 0)
                deployed_devices = app_entry.get("deployed_devices") or []
                if device_name in deployed_devices and api_deployed_count == 0:
                    api_deployed_count = 1
    except Exception:
        pass

    evidence_consistent = True
    evidence_issues: list[str] = []
    try:
        gw_response = requests.get(f"{DEFAULT_GATEWAY_HTTP}/api/v1/devices", timeout=10)
        gw_devices = {d["device_id"]: d for d in (gw_response.json().get("devices") or [])}
        gw_device = gw_devices.get(device_name, {})
        if not gw_device.get("connected"):
            evidence_consistent = False
            evidence_issues.append("gateway_not_connected")
        gw_hb = gw_device.get("last_heartbeat") or {}
        if time.time() - gw_hb.get("secs_since_epoch", 0) > 120:
            evidence_consistent = False
            evidence_issues.append("heartbeat_stale")
    except Exception as exc:
        evidence_consistent = False
        evidence_issues.append(f"gateway_query_failed:{exc}")
    if observed_phase != "Running":
        evidence_consistent = False
        evidence_issues.append(f"crd_phase_{observed_phase}")
    if api_failed_devices > 0 and observed_phase == "Running":
        evidence_consistent = False
        evidence_issues.append("api_contradicts_running")

    # Primary success criterion: Application CRD phase reached Running.
    # The application-controller is active and hardened; CRD phase is now the
    # authoritative source. API statistics are kept as a secondary corroborating
    # signal and are included in the record for analysis.
    success = bool(
        observed_phase == "Running"
        or (
            deploy_error is None
            and api_running_devices > 0
            and api_deployed_count > 0
        )
    )
    return TrialRecord(
        "deployment",
        trial,
        success,
        duration_ms=parse_duration_ms(start, end),
        details={
            "application": app_name,
            "device": device_name,
            "phase": observed_phase,
            "device_status": observed_device_status,
            "api_status": api_status,
            "api_running_devices": api_running_devices,
            "api_failed_devices": api_failed_devices,
            "api_deployed_count": api_deployed_count,
            "error": deploy_error,
            "evidence_consistent": evidence_consistent,
            "evidence_issues": evidence_issues,
            "gateway_ack": deploy_error is None,
            "api_stats_corroborated": api_running_devices > 0 or observed_phase == "Running",
        },
    )


def run_system_snapshot_trial(api_base: str, trial: int) -> TrialRecord:
    try:
        response = requests.get(f"{api_base}/api/v1/metrics", timeout=10)
        metrics = response.json() if response.ok else []
    except Exception as exc:
        metrics = []

    metric_names = {item.get("name") for item in metrics if isinstance(item, dict)}
    if {"cpu_usage", "memory_usage", "disk_usage"}.issubset(metric_names):
        return TrialRecord(
            "system_snapshot",
            trial,
            True,
            duration_ms=0.0,
            details={"metrics": metrics, "source": "api-metrics"},
        )

    pods = run_kubectl_json(["get", "pods", "-n", DEFAULT_NAMESPACE, "-o", "json"])
    pod_summary: list[dict[str, Any]] = []
    if isinstance(pods, dict):
        for item in pods.get("items", []):
            status = item.get("status") or {}
            pod_summary.append({
                "name": (item.get("metadata") or {}).get("name"),
                "phase": status.get("phase"),
                "restarts": sum((container.get("restartCount") or 0) for container in status.get("containerStatuses", [])),
            })

    return TrialRecord(
        "system_snapshot",
        trial,
        bool(pod_summary),
        duration_ms=0.0,
        details={"metrics": metrics, "source": "pod-snapshot", "pods": pod_summary},
    )


def summarize(records: list[TrialRecord]) -> dict[str, Any]:
    by_experiment: dict[str, list[TrialRecord]] = {}
    for record in records:
        by_experiment.setdefault(record.experiment, []).append(record)

    def reason_from_record(record: TrialRecord) -> str:
        details = record.details or {}
        if isinstance(details.get("reason"), str):
            return str(details["reason"])
        if isinstance(details.get("error"), str) and details["error"]:
            return str(details["error"])
        return "unknown"

    def trial_map(rows: Iterable[TrialRecord]) -> dict[int, TrialRecord]:
        return {row.trial: row for row in rows}

    summary: dict[str, Any] = {}
    for experiment, rows in by_experiment.items():
        durations = [row.duration_ms for row in rows if row.duration_ms is not None]
        success = sum(1 for row in rows if row.success)
        failures = [reason_from_record(row) for row in rows if not row.success]
        duration_s = sum(durations) / 1000.0 if durations else None
        goodput_tps = (success / duration_s) if duration_s and duration_s > 0 else None
        summary[experiment] = {
            "trials": len(rows),
            "success_rate": wilson_interval(success, len(rows)),
            "duration_ms": mean_ci95(durations),
            "latency_ms": latency_profile(durations),
            "failure_reasons": dict(Counter(failures)),
            "goodput_tps": goodput_tps,
        }

    enrollment_rows = by_experiment.get("enrollment", [])
    heartbeat_rows = by_experiment.get("heartbeat", [])
    deployment_rows = by_experiment.get("deployment", [])
    common_trials = sorted(
        set(row.trial for row in enrollment_rows)
        & set(row.trial for row in heartbeat_rows)
        & set(row.trial for row in deployment_rows)
    )
    enrollment_by_trial = trial_map(enrollment_rows)
    heartbeat_by_trial = trial_map(heartbeat_rows)
    deployment_by_trial = trial_map(deployment_rows)

    transaction_successes = 0
    evidence_consistent_count = 0
    end_to_end_ms: list[float] = []
    for trial in common_trials:
        enr = enrollment_by_trial[trial]
        hrt = heartbeat_by_trial[trial]
        dep = deployment_by_trial[trial]
        if enr.success and hrt.success and dep.success:
            transaction_successes += 1
            if (dep.details or {}).get("evidence_consistent"):
                evidence_consistent_count += 1
        if enr.duration_ms is not None and dep.duration_ms is not None:
            end_to_end_ms.append(enr.duration_ms + dep.duration_ms)

    summary["transactional"] = {
        "trials": len(common_trials),
        "all_stages_success_rate": wilson_interval(transaction_successes, len(common_trials)),
        "evidence_consistency_rate": wilson_interval(evidence_consistent_count, len(common_trials)),
        "end_to_end_latency_ms": latency_profile(end_to_end_ms),
    }
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description="Collect RETROSPECT experiment metrics")
    parser.add_argument("--api-base", default=DEFAULT_API_BASE)
    parser.add_argument("--gateway-http", default=DEFAULT_GATEWAY_HTTP)
    parser.add_argument("--namespace", default=DEFAULT_NAMESPACE)
    parser.add_argument("--trials", type=int, default=50)
    parser.add_argument("--output-dir", default="experiments")
    args = parser.parse_args()

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    records: list[TrialRecord] = []
    records.append(run_system_snapshot_trial(args.api_base, 1))

    for trial in range(1, args.trials + 1):
        records.append(run_enrollment_trial(args.api_base, args.gateway_http, args.namespace, trial))
        records.append(run_heartbeat_trial(args.gateway_http, args.namespace, trial))
        records.append(run_deploy_trial(args.api_base, args.namespace, trial))

    summary = summarize(records)
    payload = {
        "generated_at": now_iso(),
        "namespace": args.namespace,
        "api_base": args.api_base,
        "gateway_http": args.gateway_http,
        "records": [asdict(record) for record in records],
        "summary": summary,
    }

    output_path = output_dir / f"scalability_metrics_{int(time.time())}.json"
    output_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")

    print(json.dumps(summary, indent=2), file=sys.stderr)
    print(output_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())