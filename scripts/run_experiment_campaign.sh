#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRIALS="${TRIALS:-100}"
ABLATION_TRIALS="${ABLATION_TRIALS:-30}"
TS="$(date -u +%Y%m%d-%H%M%S)"
OUT="$ROOT/experiments/$TS"
mkdir -p "$OUT"/{environment,raw,summary,figures,logs}

prep_runtime() {
  "$ROOT/scripts/ensure-experiment-runtime.sh" >/dev/null
  curl -sf -X POST http://127.0.0.1:3001/api/v1/devices/native-sim-1/renode/start -H 'Content-Type: application/json' -d '{}' >/dev/null || true
  sleep 5
  if ip link show tap0 &>/dev/null; then sudo "$ROOT/scripts/setup-renode-net.sh" >/dev/null; fi
}

capture_env() {
  ENV="$OUT/environment"
  {
    echo "# Environment $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "- repo: $(cd "$ROOT" && git remote get-url origin 2>/dev/null || echo local)"
    echo "- commit: $(cd "$ROOT" && git rev-parse HEAD)"
    echo "- trials: $TRIALS hardened + $ABLATION_TRIALS ablation"
  } >"$ENV/environment_summary.md"
  (cd "$ROOT" && git remote -v; git rev-parse HEAD; git status --short; git diff --stat) >"$ENV/git.txt" 2>&1
  (uname -a; lscpu | head -20; free -h; df -h /) >"$ENV/host.txt" 2>&1
  (docker version; kubectl version; rustc --version; python3 --version) >"$ENV/toolchain.txt" 2>&1
  (kubectl get ns,pods,svc,crd -A; kubectl get all,devices,applications -n wasmbed -o wide) >"$ENV/kubernetes.txt" 2>&1
  # sudo -n (non-interactive): environments without passwordless sudo must
  # not abort the whole campaign over a diagnostic snapshot. Under `set -e
  # -o pipefail`, a plain `sudo` here that fails auth makes the pipeline's
  # exit status nonzero and kills the script before a single trial runs --
  # confirmed 2026-07-28 on a host with no passwordless sudo configured (see
  # doc/energy-tracking-validation.md). The `|| true` is required in
  # addition to `-n`: pipefail still propagates sudo's failure through the
  # `| head -10` pipe even though head itself succeeds.
  (ip link; ss -tln | grep -E '3001|8080|30443'; sudo -n iptables -t nat -L PREROUTING -n 2>/dev/null | head -10 || true) >"$ENV/network.txt" 2>&1
}

capture_logs() {
  kubectl logs -n wasmbed deploy/gateway-1-deployment --tail=8000 >"$OUT/logs/gateway.log" 2>&1 || true
  kubectl logs -n wasmbed deploy/wasmbed-application-controller --tail=8000 >"$OUT/logs/controller.log" 2>&1 || true
  kubectl logs -n wasmbed deploy/wasmbed-api-server --tail=8000 >"$OUT/logs/api_server.log" 2>&1 || true
  kubectl get all,devices,applications -n wasmbed -o yaml >"$OUT/logs/kubernetes_resources.yaml" 2>&1 || true
}

run_campaign() {
  local n="$1" dir="$2"
  python3 "$ROOT/scripts/collect_experiment_metrics.py" --trials "$n" --output-dir "$dir" 2>&1 | tail -1
}

prep_runtime
capture_env

echo "Warm-up..."
python3 "$ROOT/scripts/collect_experiment_metrics.py" --trials 1 --output-dir /tmp/retrospect_warmup >/dev/null

echo "Hardened campaign ($TRIALS trials)..."
HARD_JSON=$(run_campaign "$TRIALS" /tmp/retrospect_hardened)
cp "$HARD_JSON" "$OUT/raw/scalability_metrics.json"

echo "Ablation ($ABLATION_TRIALS trials, controller scaled to 0)..."
kubectl scale deployment/wasmbed-application-controller -n wasmbed --replicas=0
sleep 5
ABL_JSON=$(run_campaign "$ABLATION_TRIALS" /tmp/retrospect_ablation)
kubectl scale deployment/wasmbed-application-controller -n wasmbed --replicas=1
kubectl rollout status deployment/wasmbed-application-controller -n wasmbed --timeout=120s

cp "$ABL_JSON" "$OUT/raw/scalability_metrics_ablation.json"
python3 - "$HARD_JSON" "$ABL_JSON" "$OUT" <<'PY'
import json, sys
from pathlib import Path
h, a, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
for label, src, name in [("enrollment","enrollment_trials.jsonl","ablation_enrollment_trials.jsonl"),
                         ("heartbeat","heartbeat_trials.jsonl","ablation_heartbeat_trials.jsonl"),
                         ("deployment","deployment_trials.jsonl","ablation_deployment_trials.jsonl")]:
    rows = [r for r in json.loads(a.read_text())["records"] if r["experiment"]==label]
    with (out/"raw"/name).open("w") as f:
        for r in rows:
            row = {"trial_id": r["trial"], "stage": label, "latency_ms": r.get("duration_ms"), "success": r["success"]}
            row.update(r.get("details") or {})
            f.write(json.dumps(row)+"\n")
PY

python3 "$ROOT/scripts/postprocess_experiment.py" --input "$HARD_JSON" --output-dir "$OUT" --label hardened --ablation-input "$ABL_JSON"
capture_logs

"$ROOT/.venv-paper/bin/python" "$ROOT/RETROSPECT_submission/paper_ieee_inginf05/generate_figures.py" --input "$HARD_JSON" --output-dir "$OUT/figures"
"$ROOT/.venv-paper/bin/python" "$ROOT/scripts/generate_ablation_figure.py" --input "$OUT/summary/ablation_metrics.json" --output "$OUT/figures/ablation_hardening.png" 2>/dev/null || true
cp "$OUT/figures/"*.png "$ROOT/RETROSPECT_submission/paper_ieee_inginf05/figures/" 2>/dev/null || true

COMMIT=$(cd "$ROOT" && git rev-parse HEAD)
cat >"$OUT/README_REPRODUCE.md" <<EOF
# Reproduce experiment $TS
TRIALS=$TRIALS ABLATION_TRIALS=$ABLATION_TRIALS ./scripts/run_experiment_campaign.sh
Commit: $COMMIT
EOF

python3 - "$OUT" "$TRIALS" "$COMMIT" <<'PY'
import json, sys
from pathlib import Path
out, trials, commit = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
s = json.loads((out/"summary/summary_metrics.json").read_text())
(out/"summary/reviewer_experimental_summary.md").write_text(f"""# Reviewer experimental summary

Single-node K3s (wasmbed), Renode-emulated Zephyr STM32F746, TAP+DNAT to gateway TLS 192.168.1.1:30443.

Campaign: 1 warm-up + {trials} hardened trials + 30 ablation trials (controller replicas=0). Per-trial: enrollment, heartbeat supervision, minimal WASM deployment, transactional composition.

{(out/'summary/success_table.md').read_text()}

{(out/'summary/latency_table.md').read_text()}

{(out/'summary/goodput_table.md').read_text()}

{(out/'summary/ablation_reconciliation_hardening.md').read_text()}

Limitations: single-node K3s, emulation only, port-forward path.

Commit: {commit}
Artifacts: {out}
""")
PY

echo "DONE $OUT"
python3 -c "import json; s=json.load(open('$OUT/summary/summary_metrics.json')); print('txn', s['transactional']['all_stages_success_rate'])"
