# Experiment reproduction

The repository ships artifacts from a 100-trial hardened campaign plus a 30-trial ablation (application-controller scaled to zero).

## Artifacts

```
experiments/20260609-070246/
├── raw/           # JSONL trial records
├── summary/       # Tables, Wilson intervals, ablation report
├── figures/       # Boxplot, CDF, CI profile
└── environment/   # Host and toolchain snapshot (when captured)
```

See also `experiments/experimental_agent_brief.md` for the full measurement specification.

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
