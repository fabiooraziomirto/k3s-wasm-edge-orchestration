#!/usr/bin/env python3
"""Per-phase, per-pod CPU-time: brackets each stage with a cgroup cpu.stat snapshot.

Re-measures Table `cpu-time-per-phase` of the paper. Reuses the trial harness in
collect_experiment_metrics.py so the stages are exactly the ones behind the
latency table.
"""
import json, random, statistics, subprocess, sys, time, math
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import collect_experiment_metrics as cem

NS = "wasmbed"
API = "http://127.0.0.1:3001"
GW = "http://127.0.0.1:8080"
POD_PREFIXES = {
    "gateway": "gateway-1-deployment",
    "api-server": "wasmbed-api-server",
    "application-controller": "wasmbed-application-controller",
}


def pod_cgroups():
    out = subprocess.run(["kubectl", "get", "pod", "-n", NS, "-o",
                          "jsonpath={range .items[*]}{.metadata.name} {.metadata.uid}\n{end}"],
                         capture_output=True, text=True).stdout
    mapping = {}
    for line in out.splitlines():
        if not line.strip():
            continue
        name, uid = line.split()
        for label, prefix in POD_PREFIXES.items():
            if name.startswith(prefix):
                pat = f"*{uid.replace('-', '_')}*"
                found = subprocess.run(["find", "/sys/fs/cgroup", "-maxdepth", "4", "-type", "d", "-name", pat],
                                       capture_output=True, text=True).stdout.split()
                if found:
                    mapping[label] = found[0]
    return mapping


def usage_usec(cg):
    for line in Path(cg, "cpu.stat").read_text().splitlines():
        if line.startswith("usage_usec"):
            return int(line.split()[1])
    return None


def snapshot(cgs):
    return {k: usage_usec(v) for k, v in cgs.items()}


def ci95(vals):
    n = len(vals)
    if n < 2:
        return [None, None]
    m = statistics.mean(vals)
    h = cem.t_critical(n) * statistics.stdev(vals) / math.sqrt(n)
    return [m - h, m + h]


def bootstrap_ci95(vals, resamples=10000, seed=0):
    """Percentile bootstrap of the mean.

    CPU time per stage is non-negative and skewed at n=5, where a t interval
    puts its lower bound below zero. Resampling keeps the interval inside the
    range the measurement can take.
    """
    n = len(vals)
    if n < 2:
        return [None, None]
    rng = random.Random(seed)
    means = sorted(
        statistics.mean(rng.choice(vals) for _ in range(n))
        for _ in range(resamples)
    )
    lo = means[int(0.025 * resamples)]
    hi = means[int(0.975 * resamples) - 1]
    return [lo, hi]


def main():
    trials = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    cgs = pod_cgroups()
    missing = set(POD_PREFIXES) - set(cgs)
    if missing:
        raise SystemExit(f"cgroup not found for: {missing}")
    print(f"cgroups: {json.dumps(cgs, indent=2)}", file=sys.stderr)

    stages = {
        "enrollment": lambda t: cem.run_enrollment_trial(API, GW, NS, t),
        "heartbeat": lambda t: cem.run_heartbeat_trial(GW, NS, t),
        "deployment": lambda t: cem.run_deploy_trial(API, NS, t),
    }

    records = []
    for stage, fn in stages.items():
        for t in range(1, trials + 1):
            before = snapshot(cgs)
            wall0 = time.perf_counter()
            rec = fn(t)
            wall1 = time.perf_counter()
            after = snapshot(cgs)
            delta = {k: (after[k] - before[k]) / 1e6 for k in cgs}
            records.append({
                "stage": stage, "trial": t, "success": rec.success,
                "wall_ms": (wall1 - wall0) * 1000.0,
                "cpu_s": delta, "details": rec.details,
            })
            print(f"{stage} #{t} success={rec.success} wall={((wall1-wall0)*1000):.1f}ms "
                  f"cpu={ {k: round(v, 6) for k, v in delta.items()} }", file=sys.stderr)
            time.sleep(1)

    summary = {}
    for stage in stages:
        rows = [r for r in records if r["stage"] == stage]
        summary[stage] = {
            "n": len(rows),
            "successes": sum(1 for r in rows if r["success"]),
            "wall_ms_mean": statistics.mean(r["wall_ms"] for r in rows),
            "pods": {
                pod: {
                    "mean_cpu_s": statistics.mean(r["cpu_s"][pod] for r in rows),
                    "median_cpu_s": statistics.median(r["cpu_s"][pod] for r in rows),
                    "p95_cpu_s": cem.percentile(sorted(r["cpu_s"][pod] for r in rows), 95),
                    "max_cpu_s": max(r["cpu_s"][pod] for r in rows),
                    "min_cpu_s": min(r["cpu_s"][pod] for r in rows),
                    "ci95": ci95([r["cpu_s"][pod] for r in rows]),
                    "ci95_bootstrap": bootstrap_ci95([r["cpu_s"][pod] for r in rows]),
                } for pod in cgs
            },
        }
    out = {"records": records, "summary": summary}
    Path("/tmp/verify-1dev/cpu_time_per_phase.json").write_text(json.dumps(out, indent=2, default=str))
    print(json.dumps(summary, indent=2, default=str))


if __name__ == "__main__":
    main()
