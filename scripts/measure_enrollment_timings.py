#!/usr/bin/env python3
"""Extract device-side enrollment timings from the Renode UART logs.

The campaign harness (collect_experiment_metrics.py) times a control-plane round
trip against an already-connected device, so it never crosses the southbound
handshake. These timings come from the firmware's own log timestamps and are the
ones that move when the southbound protocol changes.

Reports per completed enrollment, in milliseconds:
  enrollment  TLS connected -> "Enrollment completed successfully"
  sign        challenge received -> ChallengeResponse sent (the ECDSA signature)

Usage: ./scripts/measure_enrollment_timings.py [container-name-filter]
"""

import re
import statistics
import subprocess
import sys

# Zephyr log prefix: [HH:MM:SS.mmm,uuu]
STAMP = re.compile(r"\[(\d+):(\d+):(\d+)\.(\d+),")
ANSI = re.compile(r"\x1b\[[0-9;]*m")

MARKERS = {
    "connected": "Connected to gateway with TLS",
    "challenge": "byte challenge from gateway",
    "response": "Sent ChallengeResponse",
    "completed": "Enrollment completed successfully",
}


def timestamp_ms(line):
    match = STAMP.search(line)
    if not match:
        return None
    hours, minutes, seconds, millis = (int(g) for g in match.groups())
    return ((hours * 60 + minutes) * 60 + seconds) * 1000 + millis


def events_from(log):
    found = {name: [] for name in MARKERS}
    for line in ANSI.sub("", log).splitlines():
        stamp = timestamp_ms(line)
        if stamp is None:
            continue
        for name, marker in MARKERS.items():
            if marker in line:
                found[name].append(stamp)
    return found


def containers(name_filter):
    listing = subprocess.run(
        ["docker", "ps", "--format", "{{.Names}}"],
        capture_output=True, text=True, check=True,
    ).stdout
    return [n for n in listing.split() if name_filter in n]


def uart_log(container, device):
    result = subprocess.run(
        ["docker", "exec", container, "sh", "-c", f"cat /tmp/uart-{device}.log"],
        capture_output=True, text=True,
    )
    return result.stdout


def main():
    name_filter = sys.argv[1] if len(sys.argv) > 1 else "wasmbed-renode-"
    enrollments, signatures = [], []

    for container in containers(name_filter):
        device = container.replace("wasmbed-renode-", "")
        events = events_from(uart_log(container, device))

        completed = min(len(events["connected"]), len(events["completed"]))
        if completed == 0:
            print(f"{device}: no completed enrollment in the log")
            continue

        for i in range(completed):
            total = events["completed"][i] - events["connected"][i]
            enrollments.append(total)
            line = f"{device}: enrollment {total:4d} ms"
            if i < len(events["challenge"]) and i < len(events["response"]):
                sign = events["response"][i] - events["challenge"][i]
                signatures.append(sign)
                line += f"   sign {sign:4d} ms"
            print(line)

    for label, samples in (("enrollment", enrollments), ("signature", signatures)):
        if not samples:
            continue
        mean = statistics.mean(samples)
        spread = statistics.stdev(samples) if len(samples) > 1 else 0.0
        print(
            f"{label}: n={len(samples)} mean={mean:.1f} ms "
            f"median={statistics.median(samples):.1f} ms sd={spread:.1f} ms "
            f"min={min(samples)} max={max(samples)}"
        )


if __name__ == "__main__":
    main()
