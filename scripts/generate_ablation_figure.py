#!/usr/bin/env python3
import argparse, json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

p = argparse.ArgumentParser()
p.add_argument("--input", required=True)
p.add_argument("--output", required=True)
args = p.parse_args()
data = json.loads(Path(args.input).read_text())
labels = ["Txn success", "Stale CRD", "False disc."]
h = data["hardened"]
u = data["unhardened"]
vals_h = [h["txn_rate"], h["stale_phases"], h["false_disconnect"]]
vals_u = [u["txn_rate"], u["stale_phases"], u["false_disconnect"]]
x = range(len(labels))
fig, ax = plt.subplots(figsize=(5.5, 3.2), constrained_layout=True)
ax.bar([i - 0.18 for i in x], vals_h, 0.36, label="Hardened", color="#2A7F62")
ax.bar([i + 0.18 for i in x], vals_u, 0.36, label="Unhardened", color="#B63A2B")
ax.set_xticks(list(x), labels)
ax.set_ylabel("Rate / count (normalized txn)")
ax.legend(fontsize=8)
Path(args.output).parent.mkdir(parents=True, exist_ok=True)
fig.savefig(args.output, dpi=220, bbox_inches="tight")
