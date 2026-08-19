#!/usr/bin/env python3
"""Energy campaign: CPU-time per WASM invocation vs workload size, plus the
paired idle/active pass with Kepler power.

Re-measures Section "Computational energy instrumentation" of the paper:
Table `workload-cpu-time`, the n=39 idle/active comparison, and the
Kepler wattage that corroborates it.

The probe pod (k8s/test-resources/energy-probe-deployment.yaml) cycles
idle_secs/load_secs forever and prints one JSON line per phase transition.
This driver samples the pod's cgroup cpu.stat continuously and interpolates
it onto those phase boundaries, so a window's CPU-seconds do not depend on
how fast the log line is read.

Usage: energy_campaign.py <out.json> [N:cycles[:fuel_budget] ...]
       defaults to 50000:10 200000:39 1000000:10 5000000:10
"""
import json, subprocess, sys, threading, time, urllib.request, urllib.parse, statistics, math
from pathlib import Path

NS = "wasmbed"
PROM = "http://127.0.0.1:9090"
SAMPLE_INTERVAL_S = 0.2


def sh(args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw).stdout


def probe_pod(expect_n=None):
    """The Running probe pod, waiting out a rollout so we never latch onto the
    outgoing pod: its BURN_N must match the workload we just asked for, and it
    must not be terminating."""
    for _ in range(180):
        raw = sh(["kubectl", "-n", NS, "get", "pod", "-l", "app=energy-probe", "-o", "json"])
        try:
            items = json.loads(raw).get("items", [])
        except json.JSONDecodeError:
            items = []
        for pod in items:
            if pod["status"].get("phase") != "Running" or pod["metadata"].get("deletionTimestamp"):
                continue
            if not all(c.get("ready") for c in pod["status"].get("containerStatuses", [])):
                continue
            if expect_n is not None:
                env = {e["name"]: e.get("value") for e in pod["spec"]["containers"][0].get("env", [])}
                if env.get("BURN_N") != str(expect_n):
                    continue
            return pod["metadata"]["name"], pod["metadata"]["uid"]
        time.sleep(2)
    raise SystemExit("energy-probe pod for N=%s did not become ready" % expect_n)


def cgroup_for(uid):
    hits = sh(["find", "/sys/fs/cgroup", "-maxdepth", "4", "-type", "d",
               "-name", "*%s*" % uid.replace("-", "_")]).split()
    if not hits:
        raise SystemExit("cgroup not found for probe pod")
    return hits[0]


def usage_usec(path):
    with open(path) as fh:
        for line in fh:
            if line.startswith("usage_usec"):
                return int(line.split()[1])
    return None


class Sampler(threading.Thread):
    """Continuously records (wall_clock_ms, cpu_usec) for the probe cgroup."""

    def __init__(self, cg):
        super().__init__(daemon=True)
        self.path = cg + "/cpu.stat"
        self.samples = []
        self.stop = threading.Event()

    def run(self):
        while not self.stop.is_set():
            try:
                self.samples.append((time.time() * 1000.0, usage_usec(self.path)))
            except OSError:
                pass
            time.sleep(SAMPLE_INTERVAL_S)

    def cpu_between_inner(self, t0_ms, t1_ms):
        """CPU-seconds between two instants using only samples strictly inside
        the window. Interpolating across the boundary would smear load CPU into
        the neighbouring idle window -- at a 200 ms sampling period and ~1 CPU/s
        under load that is tens of milliseconds of CPU wrongly attributed to an
        idle window that really consumes about a millisecond. Costs up to one
        sampling period of coverage at each end, which is what we want for the
        idle window and not what we want for the load window."""
        inner = [s for s in self.samples if t0_ms <= s[0] <= t1_ms]
        if len(inner) < 2:
            return None
        return (inner[-1][1] - inner[0][1]) / 1e6, (inner[-1][0] - inner[0][0]) / 1000.0

    def cpu_at(self, ts_ms):
        """CPU-usec at a wall-clock instant, linearly interpolated between samples."""
        s = self.samples
        if not s or ts_ms < s[0][0] or ts_ms > s[-1][0]:
            return None
        lo, hi = 0, len(s) - 1
        while lo < hi - 1:
            mid = (lo + hi) // 2
            if s[mid][0] <= ts_ms:
                lo = mid
            else:
                hi = mid
        (t0, c0), (t1, c1) = s[lo], s[hi]
        if t1 == t0:
            return c0
        return c0 + (c1 - c0) * (ts_ms - t0) / (t1 - t0)


def prom_range(query, start_s, end_s, step=5):
    url = PROM + "/api/v1/query_range?" + urllib.parse.urlencode(
        {"query": query, "start": "%.3f" % start_s, "end": "%.3f" % end_s, "step": str(step)})
    try:
        with urllib.request.urlopen(url, timeout=10) as r:
            return json.load(r)
    except Exception as exc:
        return {"error": str(exc)}


def joules_in_window(pod, start_ms, end_ms):
    """Kepler dynamic joules attributed to the probe pod over a window."""
    q = 'sum(increase(kepler_container_joules_total{mode="dynamic",pod_name="%s"}[%ds]))' % (
        pod, max(int((end_ms - start_ms) / 1000), 1))
    res = prom_range(q, end_ms / 1000.0, end_ms / 1000.0, 5)
    try:
        return float(res["data"]["result"][0]["values"][-1][1])
    except Exception:
        return None


def set_workload(n, idle_s, load_s, fuel=None):
    env = ["BURN_N=%d" % n, "IDLE_SECS=%d" % idle_s, "LOAD_SECS=%d" % load_s]
    # The MCU profile's default budget traps from N ~1e6 up; raise it there.
    env.append("FUEL_BUDGET=%s" % (fuel if fuel is not None else "5000000"))
    sh(["kubectl", "-n", NS, "set", "env", "deploy/energy-probe"] + env)
    subprocess.run(["kubectl", "-n", NS, "rollout", "status", "deploy/energy-probe", "--timeout=180s"],
                   capture_output=True, text=True)


def log_lines(proc, pod, cycles, done_count):
    """Lines from `kubectl logs -f`, reconnecting if the stream ends early."""
    while True:
        for line in proc.stdout:
            yield line
        if done_count() >= cycles:
            return
        print("[%s] log stream ended at %d/%d cycles, reattaching" % (pod, done_count(), cycles),
              file=sys.stderr, flush=True)
        proc.terminate()
        time.sleep(2)
        proc = subprocess.Popen(["kubectl", "-n", NS, "logs", "-f", pod, "--tail=0"],
                                stdout=subprocess.PIPE, text=True)


def collect(n, cycles, idle_s=30, load_s=30, fuel=None):
    set_workload(n, idle_s, load_s, fuel)
    pod, uid = probe_pod(expect_n=n)
    cg = cgroup_for(uid)
    sampler = Sampler(cg)
    sampler.start()
    print("[N=%d] pod=%s cgroup=%s, collecting %d cycles" % (n, pod, cg, cycles), file=sys.stderr, flush=True)

    proc = subprocess.Popen(["kubectl", "-n", NS, "logs", "-f", pod, "--tail=0"],
                            stdout=subprocess.PIPE, text=True)
    windows, pending = [], {}
    try:
        for line in log_lines(proc, pod, cycles, lambda: len(windows)):
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            kind, ts = ev.get("event"), ev.get("ts_ms")
            if kind in ("idle_start", "load_start"):
                pending[kind] = ts
            elif kind == "idle_end" and "idle_start" in pending:
                pending["idle_window"] = (pending.pop("idle_start"), ts)
            elif kind == "load_end" and "load_start" in pending:
                t0 = pending.pop("load_start")
                time.sleep(SAMPLE_INTERVAL_S * 3)  # let the sampler pass the boundary
                cpu0, cpu1 = sampler.cpu_at(t0), sampler.cpu_at(ts)
                idle_w = pending.pop("idle_window", None)
                icpu0 = sampler.cpu_at(idle_w[0]) if idle_w else None
                icpu1 = sampler.cpu_at(idle_w[1]) if idle_w else None
                if cpu0 is None or cpu1 is None:
                    continue
                w = {
                    "n": n,
                    "calls": ev.get("total_calls"),
                    "fuel_per_call": ev.get("fuel_per_call_avg"),
                    "errors": ev.get("errors"),
                    "load_ms": ts - t0,
                    "load_cpu_s": (cpu1 - cpu0) / 1e6,
                    "idle_ms": (idle_w[1] - idle_w[0]) if idle_w else None,
                    "idle_cpu_s": ((icpu1 - icpu0) / 1e6) if (icpu0 is not None and icpu1 is not None) else None,
                    "idle_inner": sampler.cpu_between_inner(*idle_w) if idle_w else None,
                    "load_inner": sampler.cpu_between_inner(t0, ts),
                    "load_joules": joules_in_window(pod, t0, ts),
                    "idle_joules": joules_in_window(pod, idle_w[0], idle_w[1]) if idle_w else None,
                    "load_start_ms": t0, "load_end_ms": ts,
                }
                w["cpu_s_per_call"] = w["load_cpu_s"] / w["calls"] if w["calls"] else None
                windows.append(w)
                print("[N=%d] cycle %d/%d calls=%s load_cpu_s=%.4f idle_cpu_s=%s per_call=%.3e J_load=%s" % (
                    n, len(windows), cycles, w["calls"], w["load_cpu_s"],
                    ("%.4f" % w["idle_cpu_s"]) if w["idle_cpu_s"] is not None else "?",
                    w["cpu_s_per_call"] or 0, w["load_joules"]), file=sys.stderr, flush=True)
                if len(windows) >= cycles:
                    break
    finally:
        proc.terminate()
        sampler.stop.set()
    return windows


def ci95(vals):
    n = len(vals)
    if n < 2:
        return [None, None]
    t = {2: 12.706, 3: 4.303, 4: 3.182, 5: 2.776, 6: 2.571, 7: 2.447, 8: 2.365, 9: 2.306, 10: 2.262}.get(
        n - 1, 1.96 + 2.4 / (n - 1))
    m, h = statistics.mean(vals), t * statistics.stdev(vals) / math.sqrt(n)
    return [m - h, m + h]


def main():
    out_path = sys.argv[1]
    specs = sys.argv[2:] or ["50000:10", "200000:39", "1000000:10", "5000000:10"]
    result = {"points": [], "generated_at": time.time()}
    for spec in specs:
        parts = spec.split(":")
        n, cycles = int(parts[0]), int(parts[1])
        fuel = int(parts[2]) if len(parts) > 2 else None
        w = collect(n, cycles, fuel=fuel)
        per_call = [x["cpu_s_per_call"] for x in w if x["cpu_s_per_call"]]
        load_cpu = [x["load_cpu_s"] for x in w]
        idle_cpu = [x["idle_cpu_s"] for x in w if x["idle_cpu_s"] is not None]
        idle_inner = [x["idle_inner"][0] for x in w if x.get("idle_inner")]
        # Paired difference per cycle, the quantity the paper reports a CI for.
        paired = [x["load_cpu_s"] - x["idle_inner"][0] for x in w if x.get("idle_inner")]
        lj = [x["load_joules"] for x in w if x["load_joules"] is not None]
        ij = [x["idle_joules"] for x in w if x["idle_joules"] is not None]
        point = {
            "N": n, "cycles": len(w), "fuel_budget": fuel,
            "cpu_s_per_call_mean": statistics.mean(per_call) if per_call else None,
            "cpu_s_per_call_ci95": ci95(per_call),
            "load_cpu_s_mean": statistics.mean(load_cpu) if load_cpu else None,
            "load_cpu_s_stdev": statistics.stdev(load_cpu) if len(load_cpu) > 1 else None,
            "idle_cpu_s_mean": statistics.mean(idle_cpu) if idle_cpu else None,
            "idle_cpu_s_stdev": statistics.stdev(idle_cpu) if len(idle_cpu) > 1 else None,
            "idle_cpu_s_inner_mean": statistics.mean(idle_inner) if idle_inner else None,
            "idle_cpu_s_inner_stdev": statistics.stdev(idle_inner) if len(idle_inner) > 1 else None,
            "paired_diff_mean": statistics.mean(paired) if paired else None,
            "paired_diff_ci95": ci95(paired),
            "calls_mean": statistics.mean([x["calls"] for x in w if x["calls"]]) if w else None,
            "load_watts_mean": statistics.mean([j / 30.0 for j in lj]) if lj else None,
            "idle_watts_mean": statistics.mean([j / 30.0 for j in ij]) if ij else None,
            "windows": w,
        }
        result["points"].append(point)
        Path(out_path).write_text(json.dumps(result, indent=2))
        print(json.dumps({k: v for k, v in point.items() if k != "windows"}, indent=1), flush=True)
    Path(out_path).write_text(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
