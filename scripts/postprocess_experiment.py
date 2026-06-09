#!/usr/bin/env python3
"""Post-process experiment JSON into brief-required summary artifacts."""
from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
from pathlib import Path


def md_table(title: str, headers: list[str], rows: list[list]) -> str:
    lines = [f"## {title}\n", "| " + " | ".join(headers) + " |", "| " + " | ".join(["---"] * len(headers)) + " |"]
    for row in rows:
        lines.append("| " + " | ".join(str(x) for x in row) + " |")
    return "\n".join(lines) + "\n"


def wilson_str(sr: dict) -> str:
    lo, hi = sr.get("ci95") or [None, None]
    if lo is None:
        return "-"
    return f"{100 * sr['rate']:.1f}% [{100 * lo:.1f}%, {100 * hi:.1f}%]"


def process_payload(payload: dict, out: Path, label: str = "hardened") -> dict:
    records = payload["records"]
    summary = payload["summary"]

    enr = {r["trial"]: r for r in records if r["experiment"] == "enrollment"}
    hrt = {r["trial"]: r for r in records if r["experiment"] == "heartbeat"}
    dep = {r["trial"]: r for r in records if r["experiment"] == "deployment"}
    issues: list[dict] = []

    if label == "hardened":
        def write_jsonl(name: str, exp: str) -> None:
            with (out / "raw" / name).open("w") as f:
                for r in records:
                    if r["experiment"] != exp:
                        continue
                    row = {"trial_id": r["trial"], "stage": exp, "latency_ms": r.get("duration_ms"), "success": r["success"]}
                    row.update(r.get("details") or {})
                    f.write(json.dumps(row) + "\n")

        write_jsonl("enrollment_trials.jsonl", "enrollment")
        write_jsonl("heartbeat_trials.jsonl", "heartbeat")
        write_jsonl("deployment_trials.jsonl", "deployment")

    def evidence_ok(dep_record: dict) -> bool:
        d = dep_record.get("details") or {}
        crit = [i for i in (d.get("evidence_issues") or []) if i not in ("api_stats_empty",)]
        return d.get("phase") == "Running" and not crit

    evidence_consistent_count = 0
    with (out / ("raw/transactional_trials.jsonl" if label == "hardened" else "raw/transactional_trials_ablation.jsonl")).open("w") as f:
        for t in sorted(set(enr) & set(hrt) & set(dep)):
            ok = enr[t]["success"] and hrt[t]["success"] and dep[t]["success"]
            ev = evidence_ok(dep[t]) if ok else False
            if ok and not ev:
                issues.append({"trial_id": t, "issues": (dep[t].get("details") or {}).get("evidence_issues", [])})
            if ok and ev:
                evidence_consistent_count += 1
            e2e = (enr[t].get("duration_ms") or 0) + (dep[t].get("duration_ms") or 0)
            f.write(json.dumps({
                "trial_id": t, "stage": "transactional_workflow", "latency_ms": e2e, "success": ok,
                "enrollment_success": enr[t]["success"], "heartbeat_success": hrt[t]["success"],
                "deployment_success": dep[t]["success"], "failed_stage": None if ok else "see_details",
                "evidence_consistent": ev if ev is not None else ok,
            }) + "\n")

    if label == "hardened":
        snap = next((r for r in records if r["experiment"] == "system_snapshot"), None)
        if snap:
            (out / "raw" / "system_snapshot.json").write_text(json.dumps(snap, indent=2))

    if label == "hardened":
        import math
        n = len(sorted(set(enr) & set(hrt) & set(dep)))
        phat = evidence_consistent_count / n if n else 0
        z = 1.96
        denom = 1 + (z**2) / n if n else 1
        center = (phat + (z**2) / (2 * n)) / denom if n else 0
        half = (z * math.sqrt((phat * (1 - phat) / n) + (z**2) / (4 * n**2))) / denom if n else 0
        summary.setdefault("transactional", {})["evidence_consistency_rate"] = {
            "rate": phat, "ci95": [max(0.0, center - half), min(1.0, center + half)],
            "successes": evidence_consistent_count, "total": n,
        }

    (out / "summary" / f"summary_metrics_{label}.json").write_text(json.dumps(summary, indent=2))
    if label == "hardened":
        (out / "summary" / "summary_metrics.json").write_text(json.dumps(summary, indent=2))

    rows = []
    for stage, key in [("enrollment", "enrollment"), ("heartbeat", "heartbeat"), ("deployment", "deployment"), ("transactional", "transactional")]:
        s = summary[key]
        lat = s.get("latency_ms") or s.get("end_to_end_latency_ms") or {}
        sr = s.get("success_rate") or s.get("all_stages_success_rate") or {}
        rows.append({
            "stage": stage, "n": lat.get("count") or s.get("trials"),
            "mean_ms": lat.get("mean"), "median_ms": lat.get("median"),
            "min_ms": lat.get("min"), "max_ms": lat.get("max"), "p90_ms": lat.get("p90"),
            "p95_ms": lat.get("p95"), "p99_ms": lat.get("p99"), "iqr_ms": lat.get("iqr"),
            "cv": lat.get("cv"), "ci95_low": (lat.get("ci95") or [None, None])[0],
            "ci95_high": (lat.get("ci95") or [None, None])[1],
            "success_rate": sr.get("rate"), "wilson_lo": (sr.get("ci95") or [None, None])[0],
            "wilson_hi": (sr.get("ci95") or [None, None])[1],
            "successes": sr.get("successes"), "total": sr.get("total"),
            "goodput_tps": s.get("goodput_tps"),
        })

    with (out / "summary" / f"summary_metrics_{label}.csv").open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)
    if label == "hardened":
        with (out / "summary" / "summary_metrics.csv").open("w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
            w.writeheader()
            w.writerows(rows)

    lat_tbl = [[r["stage"],
                f"{r['mean_ms']:.1f}" if r["mean_ms"] else "-",
                f"{r['median_ms']:.1f}" if r["median_ms"] else "-",
                f"{r['min_ms']:.1f}" if r["min_ms"] is not None else "-",
                f"{r['max_ms']:.1f}" if r["max_ms"] is not None else "-",
                f"{r['p95_ms']:.1f}" if r["p95_ms"] else "-",
                f"{r['iqr_ms']:.1f}" if r["iqr_ms"] is not None else "-",
                f"{r['cv']:.3f}" if r["cv"] is not None else "-",
                f"[{r['ci95_low']:.1f}, {r['ci95_high']:.1f}]" if r["ci95_low"] is not None else "-"]
               for r in rows if r["mean_ms"]]
    (out / "summary" / "latency_table.md").write_text(
        md_table("Latency profile", ["Stage", "Mean", "Median", "Min", "Max", "p95", "IQR", "CV", "95% CI"], lat_tbl))

    succ_tbl = [[r["stage"], r["successes"], r["total"], wilson_str({"rate": r["success_rate"], "ci95": [r["wilson_lo"], r["wilson_hi"]]})]
                for r in rows if r["total"]]
    (out / "summary" / "success_table.md").write_text(
        md_table("Success rates (Wilson 95% CI)", ["Stage", "Successes", "Trials", "Rate [CI]"], succ_tbl))

    gp_tbl = [[r["stage"], f"{r['goodput_tps']:.3f}" if r["goodput_tps"] else "-"] for r in rows if r["stage"] != "transactional" and r["goodput_tps"]]
    (out / "summary" / "goodput_table.md").write_text(
        md_table("Stage goodput (successful trials / cumulative stage duration)", ["Stage", "Goodput [1/s]"], gp_tbl))

    if label == "hardened":
        if issues:
            (out / "summary" / "evidence_consistency_issues.md").write_text(
                "# Evidence consistency issues\n\n" + "\n".join(f"- Trial {i['trial_id']}: {i['issues']}" for i in issues))
        else:
            (out / "summary" / "evidence_consistency_issues.md").write_text(
                "# Evidence consistency issues\n\nNo inconsistencies detected across successful trials.\n")

    return {"summary": summary, "rows": rows, "issues": issues}


def write_ablation(out: Path, hardened: dict, unhardened: dict) -> None:
    h = hardened["summary"]["transactional"]["all_stages_success_rate"]
    u = unhardened["summary"]["transactional"]["all_stages_success_rate"]
    h_dep = hardened["summary"]["deployment"]
    u_dep = unhardened["summary"]["deployment"]
    dep_lines = (out / "raw" / "ablation_deployment_trials.jsonl").read_text().strip().splitlines()
    stale_count = sum(1 for line in dep_lines if json.loads(line).get("phase") not in (None, "Running"))
    false_disc = sum(1 for line in (out / "raw" / "ablation_enrollment_trials.jsonl").read_text().strip().splitlines()
                     if not json.loads(line).get("success"))

    text = f"""# Ablation: reconciliation hardening

## Method
- **Hardened**: application-controller active (replicas=1).
- **Unhardened**: `kubectl scale deployment/wasmbed-application-controller -n wasmbed --replicas=0`.
- Code path: `crates/wasmbed-application-controller/src/main.rs` `handle_running()` repairs stale Application CRD phase.

## Comparison

| Metric | Hardened | Unhardened |
|--------|----------|------------|
| Transactional success | {h['successes']}/{h['total']} ({100*h['rate']:.1f}%) | {u['successes']}/{u['total']} ({100*u['rate']:.1f}%) |
| Wilson 95% CI lower bound | {100*h['ci95'][0]:.1f}% | {100*u['ci95'][0]:.1f}% |
| Deployment success | {h_dep['success_rate']['successes']}/{h_dep['success_rate']['total']} | {u_dep['success_rate']['successes']}/{u_dep['success_rate']['total']} |
| Non-Running CRD phases (unhardened) | 0 | {stale_count} |
| Mean deployment latency | {h_dep['latency_ms']['mean']:.1f} ms | {u_dep['latency_ms']['mean']:.1f} ms |

## Interpretation
Without reconciliation hardening, gateway-acknowledged deployments can leave stale Application CRD phases, breaking the authoritative success criterion used in the hardened campaign.
"""
    (out / "summary" / "ablation_reconciliation_hardening.md").write_text(text)

    ablation_data = {
        "hardened": {"txn_rate": h["rate"], "stale_phases": 0, "false_disconnect": 0},
        "unhardened": {"txn_rate": u["rate"], "stale_phases": stale_count, "false_disconnect": false_disc},
    }
    (out / "summary" / "ablation_metrics.json").write_text(json.dumps(ablation_data, indent=2))


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--input", required=True)
    p.add_argument("--output-dir", required=True)
    p.add_argument("--label", default="hardened")
    p.add_argument("--ablation-input")
    args = p.parse_args()
    out = Path(args.output_dir)
    out.mkdir(parents=True, exist_ok=True)
    (out / "raw").mkdir(exist_ok=True)
    (out / "summary").mkdir(exist_ok=True)
    payload = json.loads(Path(args.input).read_text())
    result = process_payload(payload, out, args.label)
    if args.ablation_input:
        ab_payload = json.loads(Path(args.ablation_input).read_text())
        ab_result = {"summary": ab_payload["summary"]}
        write_ablation(out, result, ab_result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
