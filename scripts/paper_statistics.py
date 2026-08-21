#!/usr/bin/env python3
"""Recompute every derived statistic the evaluation section quotes in prose.

The latency table comes out of the campaign summary, but the surrounding text
also cites half-campaign means, a lag-1 autocorrelation, a Spearman rank
correlation against trial index, per-stage p95/p99 and the dispersion of the
deployment stage with and without its retried trials. Those are recomputed here
from the raw per-trial records so that the prose can be checked against the same
files the table comes from.

Usage: ./scripts/paper_statistics.py <campaign-dir>
"""

import json
import math
import statistics
import sys
from pathlib import Path

STAGES = ("enrollment", "heartbeat", "deployment", "transactional")


def load(campaign, stage):
    path = Path(campaign) / "raw" / f"{stage}_trials.jsonl"
    if not path.exists():
        return []
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    return sorted(rows, key=lambda r: r.get("trial_id", 0))


def pct(values, q):
    s = sorted(values)
    if not s:
        return float("nan")
    k = (len(s) - 1) * q / 100.0
    lo, hi = int(k), min(int(k) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def ci95(values):
    n = len(values)
    if n < 2:
        return (float("nan"), float("nan"))
    mean = statistics.mean(values)
    half = 1.984 * statistics.stdev(values) / math.sqrt(n)  # t(0.975, ~100)
    return (mean - half, mean + half)


def spearman(xs, ys):
    def ranks(v):
        order = sorted(range(len(v)), key=lambda i: v[i])
        r = [0.0] * len(v)
        for pos, i in enumerate(order):
            r[i] = pos + 1
        return r
    rx, ry = ranks(xs), ranks(ys)
    n = len(xs)
    mx, my = statistics.mean(rx), statistics.mean(ry)
    num = sum((a - mx) * (b - my) for a, b in zip(rx, ry))
    den = math.sqrt(sum((a - mx) ** 2 for a in rx) * sum((b - my) ** 2 for b in ry))
    return num / den if den else float("nan")


def lag1(values):
    n = len(values)
    if n < 3:
        return float("nan")
    mean = statistics.mean(values)
    num = sum((values[i] - mean) * (values[i + 1] - mean) for i in range(n - 1))
    den = sum((v - mean) ** 2 for v in values)
    return num / den if den else float("nan")


def describe(label, values):
    if not values:
        print(f"{label}: no records")
        return
    lo, hi = ci95(values)
    print(f"{label}")
    print(f"  n={len(values)}  mean={statistics.mean(values):.1f}  median={statistics.median(values):.1f}")
    print(f"  p95={pct(values,95):.1f}  p99={pct(values,99):.1f}  min={min(values):.1f}  max={max(values):.1f}")
    iqr = pct(values, 75) - pct(values, 25)
    cv = statistics.stdev(values) / statistics.mean(values) if len(values) > 1 else 0.0
    print(f"  IQR={iqr:.1f}  CV={cv:.3f}  95% CI=[{lo:.1f}, {hi:.1f}]")


def main():
    campaign = sys.argv[1] if len(sys.argv) > 1 else "."

    per_stage = {}
    for stage in STAGES:
        rows = load(campaign, stage)
        ok = [r for r in rows if r.get("success")]
        per_stage[stage] = ok
        values = [r["latency_ms"] for r in ok]
        describe(f"\n== {stage} (successful trials only) ==", values)
        failed = len(rows) - len(ok)
        if failed:
            print(f"  {failed} failed trial(s) excluded: "
                  + ", ".join(f"#{r.get('trial_id')} {r.get('error') or r.get('evidence_issues')}"
                              for r in rows if not r.get("success")))

    e2e = [r["latency_ms"] for r in per_stage["transactional"]]
    if len(e2e) >= 4:
        half = len(e2e) // 2
        first, second = e2e[:half], e2e[half:]
        print("\n== end-to-end series (stationarity) ==")
        print(f"  first half  mean={statistics.mean(first):.1f}  sd={statistics.stdev(first):.1f}")
        print(f"  second half mean={statistics.mean(second):.1f}  sd={statistics.stdev(second):.1f}")
        rho = spearman(list(range(len(e2e))), e2e)
        n = len(e2e)
        t = rho * math.sqrt((n - 2) / (1 - rho ** 2)) if abs(rho) < 1 else float("nan")
        print(f"  lag-1 autocorrelation={lag1(e2e):+.2f}")
        print(f"  Spearman rho vs trial index={rho:+.2f}  (t={t:+.2f}, n={n})")


if __name__ == "__main__":
    main()
