#!/usr/bin/env python3
"""Regenerate figures/wire_sizes_bar.png from the current protocol encoding.

The sizes are the ones pinned by `test_wire_sizes` in
crates/wasmbed-protocol/src/cbor.rs: a 4-byte big-endian length prefix plus the
CBOR body. They are kept here rather than parsed out of the Rust source so the
figure stays readable, and the test is what stops the two drifting.

Usage: ./scripts/generate_wire_sizes_figure.py [--out figures/wire_sizes_bar.png]
"""

import argparse
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

# (label, bytes on the wire, phase)
MESSAGES = [
    ("Heartbeat",          6,  "steady state"),
    ("HeartbeatAck",       6,  "steady state"),
    ("EnrollmentRequest",  6,  "enrollment"),
    ("EnrollmentAccepted", 6,  "enrollment"),
    ("PublicKey",         99,  "enrollment"),
    ("Challenge",         40,  "enrollment"),
    ("ChallengeResponse", 79,  "enrollment"),
    ("DeviceUuid",        23,  "enrollment"),
    ("EnrollmentAck",      6,  "enrollment"),
    ("EnrollmentCompleted", 6, "enrollment"),
]

# Deployment is not shown per message: its size depends on the application id,
# the module name and the module itself. What the digest adds is fixed, and is
# quoted in the text instead: a 32-byte hash costs 34 bytes on the wire
# (a 2-byte CBOR byte-string header plus the hash).
MODULE_DIGEST_OVERHEAD = 34

PHASE_COLOURS = {
    "steady state": "tab:green",
    "enrollment": "tab:blue",
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="figures/wire_sizes_bar.png")
    args = parser.parse_args()

    labels = [m[0] for m in MESSAGES]
    sizes = [m[1] for m in MESSAGES]
    colours = [PHASE_COLOURS[m[2]] for m in MESSAGES]

    fig, ax = plt.subplots(figsize=(6.4, 3.6))
    bars = ax.bar(range(len(labels)), sizes, color=colours)
    for bar, size in zip(bars, sizes):
        ax.text(bar.get_x() + bar.get_width() / 2, size + 1.5, str(size),
                ha="center", va="bottom", fontsize=7)

    ax.set_xticks(range(len(labels)))
    ax.set_xticklabels(labels, rotation=45, ha="right", fontsize=7)
    ax.set_ylabel("Bytes on the wire")
    ax.set_ylim(0, max(sizes) * 1.18)
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)

    handles = [plt.Rectangle((0, 0), 1, 1, color=c) for c in PHASE_COLOURS.values()]
    ax.legend(handles, PHASE_COLOURS.keys(), fontsize=7, frameon=False, loc="upper left")

    enrollment = sum(s for _, s, phase in MESSAGES if phase == "enrollment")
    ax.set_title(f"Application-layer message sizes (enrollment totals {enrollment}~B)"
                 .replace("~", " "), fontsize=9)

    fig.tight_layout()
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out, dpi=200)
    print(f"wrote {out} (enrollment round trip {enrollment} B)")


if __name__ == "__main__":
    main()
