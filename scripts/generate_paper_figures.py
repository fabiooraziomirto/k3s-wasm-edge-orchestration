#!/usr/bin/env python3
"""Regenerate the paper's measured figures from a campaign's raw records.

Covers the five figures whose values changed in the 2026-08-19 re-measurement:
the three latency figures, the control-plane pod resources bar chart, and the
two energy figures. `wire_sizes_bar.png` is regenerated separately, by
generate_wire_sizes_figure.py: the CBOR wire format did change when the
challenge exchange was added, and that figure no longer matched.

Usage:
  generate_paper_figures.py --latency <scalability_metrics.json>
                            --resources <pod_resources.txt>
                            --energy <energy_campaign.json>
                            --out figures/
Any of the three inputs may be omitted; only the figures it feeds are written.
"""
import argparse, json, re, statistics
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

STAGE_COLORS = {"Enrollment": "tab:blue", "Deployment": "tab:red", "End-to-end": "tab:green"}


def by_stage(records):
    out = {}
    for r in records:
        if r.get("duration_ms") is None:
            continue
        out.setdefault(r["experiment"], []).append(r["duration_ms"])
    return out


def end_to_end(records):
    """Per-trial enrollment + deployment, the transactional latency."""
    enr = {r["trial"]: r["duration_ms"] for r in records
           if r["experiment"] == "enrollment" and r.get("duration_ms") is not None}
    dep = {r["trial"]: r["duration_ms"] for r in records
           if r["experiment"] == "deployment" and r.get("duration_ms") is not None}
    return [enr[t] + dep[t] for t in sorted(set(enr) & set(dep))]


def pct(values, q):
    s = sorted(values)
    if not s:
        return None
    k = (len(s) - 1) * q / 100.0
    lo, hi = int(k), min(int(k) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def fig_latency(records, out):
    stages = by_stage(records)
    e2e = end_to_end(records)
    n = len(e2e)

    # Boxplot of the three stages.
    fig, ax = plt.subplots(figsize=(6.4, 4.0))
    data = [stages["enrollment"], stages["heartbeat"], stages["deployment"]]
    ax.boxplot(data, tick_labels=["Enrollment", "Heartbeat", "Deployment"],
               showmeans=True, meanprops=dict(marker="D", markerfacecolor="black",
                                              markeredgecolor="black", markersize=6),
               patch_artist=True, boxprops=dict(facecolor="tab:orange", alpha=0.7))
    ax.set_yscale("log")
    ax.set_ylabel("Latency (ms, log scale)")
    ax.set_title("Trial-level stage latencies ($n = %d$)" % n)
    ax.grid(axis="y", alpha=0.3)
    ax.plot([], [], "D", color="black", label="Mean")
    ax.legend(loc="upper left")
    fig.tight_layout()
    fig.savefig(out / "latency_boxplot.png", dpi=150)
    plt.close(fig)

    # Empirical CDFs with each stage's p95.
    fig, ax = plt.subplots(figsize=(6.4, 4.0))
    for label, vals in (("Enrollment", stages["enrollment"]),
                        ("Deployment", stages["deployment"]),
                        ("End-to-end", e2e)):
        s = sorted(vals)
        ax.plot(s, [(i + 1) / len(s) for i in range(len(s))], label=label,
                color=STAGE_COLORS[label], linewidth=1.8)
        ax.axvline(pct(vals, 95), color=STAGE_COLORS[label], linestyle="--", alpha=0.6, linewidth=1)
    ax.axhline(0.95, color="gray", linestyle="--", linewidth=1)
    ax.axhline(0.99, color="gray", linestyle=":", linewidth=1)
    ax.set_xlabel("Latency (ms)")
    ax.set_ylabel("Empirical CDF")
    ax.set_title("Latency CDF ($n = %d$)" % n)
    ax.grid(alpha=0.3)
    ax.legend(loc="lower right")
    fig.tight_layout()
    fig.savefig(out / "latency_cdf.png", dpi=150)
    plt.close(fig)

    # Per-trial scatter of the transactional latency.
    fig, ax = plt.subplots(figsize=(6.6, 3.8))
    ax.scatter(range(1, n + 1), e2e, s=22, color="tab:blue", alpha=0.75, edgecolors="none")
    ax.axhline(statistics.mean(e2e), color="tab:red", linestyle="--",
               label="Mean = %.1f ms" % statistics.mean(e2e))
    ax.axhline(pct(e2e, 95), color="tab:green", linestyle=":",
               label="p95 = %.1f ms" % pct(e2e, 95))
    ax.set_xlabel("Trial index")
    ax.set_ylabel("End-to-end latency (ms)")
    ax.set_title("Per-trial transactional latency")
    ax.grid(alpha=0.3)
    ax.legend(loc="upper right")
    fig.tight_layout()
    fig.savefig(out / "latency_trial_scatter.png", dpi=150)
    plt.close(fig)
    print("latency figures: n=%d, mean e2e=%.1f ms" % (n, statistics.mean(e2e)))


def fig_resources(text, out):
    """Parse the sampler's output: `kubectl top` rows plus CGROUP lines."""
    pods = {"gateway-1-deployment": "Gateway",
            "wasmbed-api-server": "API server",
            "wasmbed-application-controller": "App controller"}
    cpu, ram = {v: [] for v in pods.values()}, {v: [] for v in pods.values()}
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("CGROUP") or line.startswith("==="):
            continue
        m = re.match(r"(\S+)\s+(\d+)m\s+(\d+)Mi", line)
        if not m:
            continue
        for prefix, label in pods.items():
            if m.group(1).startswith(prefix):
                cpu[label].append(int(m.group(2)))
                ram[label].append(int(m.group(3)))
    labels = list(pods.values())
    n = len(cpu[labels[0]])
    cpu_m = [statistics.mean(cpu[l]) for l in labels]
    ram_m = [statistics.mean(ram[l]) for l in labels]

    fig, ax = plt.subplots(figsize=(6.4, 4.0))
    ax2 = ax.twinx()
    x = range(len(labels))
    ax.bar([i - 0.2 for i in x], cpu_m, width=0.4, color="tab:blue", label="CPU (millicores)")
    ax2.bar([i + 0.2 for i in x], ram_m, width=0.4, color="tab:red", alpha=0.8, label="RAM (MiB)")
    ax.set_xticks(list(x))
    ax.set_xticklabels(labels, rotation=20, ha="right")
    ax.set_ylabel("CPU (millicores)")
    ax2.set_ylabel("RAM (MiB)")
    ax.set_title("Control-plane pod resources (mean of %d samples)" % n)
    h1, l1 = ax.get_legend_handles_labels()
    h2, l2 = ax2.get_legend_handles_labels()
    ax.legend(h1 + h2, l1 + l2, loc="upper left")
    fig.tight_layout()
    fig.savefig(out / "pod_resources_bar.png", dpi=150)
    plt.close(fig)
    print("pod resources: n=%d, CPU=%s, RAM=%s" % (n, [round(c, 1) for c in cpu_m], [round(r, 1) for r in ram_m]))


def fig_energy(camp, out):
    pts = sorted((p for p in camp["points"] if p.get("cpu_s_per_call_mean")), key=lambda p: p["N"])

    # Workload-size curve against an O(N) reference anchored at the smallest point.
    fig, ax = plt.subplots(figsize=(6.4, 4.0))
    ns = [p["N"] for p in pts]
    ys = [p["cpu_s_per_call_mean"] for p in pts]
    ax.loglog(ns, ys, "o-", color="tab:blue", label="Measured CPU-time per invocation")
    ref = [ys[0] * (n / ns[0]) for n in ns]
    ax.loglog(ns, ref, "--", color="gray", label="$O(N)$ reference (slope 1)")
    ax.set_xlabel("Workload size $N$")
    ax.set_ylabel("CPU-s per invocation")
    ax.set_title("CPU-time per WASM invocation vs. workload size")
    ax.grid(alpha=0.3, which="both")
    ax.legend(loc="upper left")
    fig.tight_layout()
    fig.savefig(out / "energy_workload_size_curve.png", dpi=150)
    plt.close(fig)

    # Idle vs active Kepler power at the reference workload size.
    ref_pt = next((p for p in pts if p["N"] == 200000), pts[0])
    idle_w = [w["idle_joules"] / 30.0 for w in ref_pt["windows"] if w.get("idle_joules") is not None]
    load_w = [w["load_joules"] / 30.0 for w in ref_pt["windows"] if w.get("load_joules") is not None]
    if idle_w and load_w:
        fig, ax = plt.subplots(figsize=(5.6, 4.0))
        bp = ax.boxplot([idle_w, load_w], tick_labels=["Idle", "Active"], patch_artist=True,
                        showmeans=True, meanprops=dict(marker="D", markerfacecolor="black",
                                                       markeredgecolor="black", markersize=6))
        for patch, color in zip(bp["boxes"], ["tab:blue", "tab:red"]):
            patch.set_facecolor(color)
            patch.set_alpha(0.7)
        ax.set_ylabel("Kepler-estimated power (W)")
        ax.set_title("Idle vs. active power ($n = %d$ paired cycles, $N = %d$)"
                     % (min(len(idle_w), len(load_w)), ref_pt["N"]))
        ax.grid(axis="y", alpha=0.3)
        fig.tight_layout()
        fig.savefig(out / "energy_idle_active_kepler.png", dpi=150)
        plt.close(fig)
        print("energy idle/active: idle=%.3f W, active=%.3f W" % (statistics.mean(idle_w), statistics.mean(load_w)))
    print("energy curve: " + ", ".join("N=%d %.3e" % (p["N"], p["cpu_s_per_call_mean"]) for p in pts))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--latency")
    ap.add_argument("--resources")
    ap.add_argument("--energy")
    ap.add_argument("--out", default="figures")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    if args.latency:
        fig_latency(json.loads(Path(args.latency).read_text())["records"], out)
    if args.resources:
        fig_resources(Path(args.resources).read_text(), out)
    if args.energy:
        fig_energy(json.loads(Path(args.energy).read_text()), out)


if __name__ == "__main__":
    main()
