# Experiment reproduction

The repository ships artifacts from a 100-trial hardened campaign plus a 30-trial ablation (application-controller scaled to zero).

## Artifacts

The campaign the paper reports is `experiments/20260821-083420`, collected together
with the rest of the published evidence in `artifact/`:

```
artifact/
├── README.md                 # Maps each published number to the record behind it
├── PROVENANCE.txt            # Revision the package was assembled from
├── campaign/
│   ├── raw/                  # JSONL trial records
│   ├── summary/              # Tables and Wilson intervals
│   ├── figures/              # Boxplot, CDF, trial scatter
│   └── environment/          # Host, toolchain and commit capture
├── firmware/                 # Linker output for the compared builds
├── security/                 # Authentication checks
├── pod-resources.txt         # Control-plane CPU and memory samples
└── enrollment-timings.txt    # Device-side enrollment and signature timings
```

Earlier campaigns are kept under `experiments/` and are not the source of any
published number. See `experiments/experimental_agent_brief.md` for the full
measurement specification.

## Prerequisites

- Running K3s deployment (`./scripts/deploy-k3s.sh`)
- Emulated device connected (`./scripts/ensure-experiment-runtime.sh`, Renode start, `./scripts/setup-renode-net.sh`)

## Run campaign

```bash
export TRIALS=100
export ABLATION_TRIALS=30
./scripts/run_experiment_campaign.sh
```

Outputs land under `experiments/<timestamp>/`.

## Collect metrics only

```bash
./scripts/ensure-experiment-runtime.sh
python3 scripts/collect_experiment_metrics.py --trials 100 --output-dir /tmp/campaign
python3 scripts/postprocess_experiment.py \
  --input /tmp/campaign/scalability_metrics_*.json \
  --output-dir experiments/manual-run
```

## Key metrics

- Per-stage latency (enrollment, heartbeat supervision, deployment, end-to-end)
- Transactional success (all stages in one trial)
- Goodput per stage (successful trials / cumulative stage duration)
- Evidence consistency (gateway state + Application CRD `Running`)
