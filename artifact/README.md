# Code and data availability package

Every number reported in the evaluation section of the paper is produced by one
of the records collected here. This file maps each published figure, table and
in-text value to the file it comes from, and to the script that turns raw
records into it, so that the claim can be checked rather than taken on trust.

The campaign is `20260821-083420`, run against the release images built from
commit `abcc416` with one emulated device attached and no other emulator on the
host. `campaign/environment/git.txt` records that commit; the released tree
adds only the analysis scripts and the paper text on top of it, and no change
to the gateway, the firmware or the protocol.

`PROVENANCE.txt` records the repository revision this package was assembled
from, and any uncommitted change present at that moment.

The campaign directories under `experiments/` are earlier runs of the same
harness and are not the source of any published number.

## Layout

    campaign/environment/   host, toolchain, cluster and commit capture
    campaign/raw/           per-trial records, one JSON object per line
    campaign/summary/       the tables derived from raw/
    campaign/figures/       the figures derived from raw/
    firmware/               linker output for the two firmware builds compared
    security/               output of the authentication checks
    enrollment-timings.txt  device-side enrollment timings from the UART logs

## Where each published number comes from

| Published | Source | Produced by |
|---|---|---|
| Latency profile table | `campaign/summary/latency_table.md` | `scripts/postprocess_experiment.py` |
| Success-rate table | `campaign/summary/success_table.md` | `scripts/postprocess_experiment.py` |
| Latency box plot, CDF, trial scatter | `campaign/figures/` | `scripts/generate_paper_figures.py` |
| Half-campaign means, lag-1 autocorrelation, Spearman correlation | `campaign/raw/transactional_trials.jsonl` | `scripts/paper_statistics.py` |
| Per-stage p95, p99, IQR, CV | `campaign/raw/*_trials.jsonl` | `scripts/paper_statistics.py` |
| Application-layer wire sizes | pinned by `test_wire_sizes` in `crates/wasmbed-protocol/src/cbor.rs` | `cargo test -p wasmbed-protocol` |
| Wire-size figure | `figures/wire_sizes_bar.png` | `scripts/generate_wire_sizes_figure.py` |
| Firmware flash and RAM footprint | `firmware/footprint-authenticated.txt` | Zephyr linker report |
| Cost of authentication in flash and RAM | difference against `firmware/footprint-baseline.txt` | Zephyr linker report |
| Control-plane pod CPU and memory | `pod-resources.txt` | `kubectl top`, cgroup v2 reads |
| Per-phase, per-pod CPU time | `cpu-time-per-phase.json` | `scripts/cpu_time_per_phase.py` |
| Enrollment and signature timings on the device | `enrollment-timings.txt` | `scripts/measure_enrollment_timings.py` |
| Refusal of an unproven device (fault-injection row) | `security/proof-of-possession-probe.txt` | `scripts/probe_proof_of_possession.py` |
| Refusal of an untrusted or absent client certificate | `security/tls-negative-tests.txt` | `openssl s_client`, commands in that file |

## Reproducing the campaign

The campaign runs against a live cluster with one emulated device attached:

    ./scripts/provision-device-identity.sh <device>     # issue the device identity
    ./scripts/run-e2e-fleet.sh --skip-build -n 1        # bring the device up
    ./scripts/run_experiment_campaign.sh                # 100 trials plus ablation

Then, from a checkout with matplotlib available:

    ./scripts/paper_statistics.py experiments/<timestamp>
    ./scripts/generate_paper_figures.py --latency experiments/<timestamp>/raw/scalability_metrics.json --out figures/
    ./scripts/build_artifact.sh experiments/<timestamp>

## What the campaign does and does not measure

The trial harness times the control-plane path: a board-registration POST to
the gateway, a device listing from the API server and two resource queries.
It runs against a device that is already attached, so it does not cross the
southbound TLS handshake or the enrollment exchange. The cost of those is
measured separately, on the device, and reported in `enrollment-timings.txt`.

Trials are repetitions against one warm host with no induced faults. They
establish that the evidence-convergence criterion holds whenever the mechanism
is exercised without failure; they are not an availability estimate for a
fleet.
