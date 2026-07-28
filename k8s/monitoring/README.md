# Energy/monitoring stack (Phase 1)

Adds a **Kepler** energy exporter and a minimal **Prometheus** to the cluster.
This is the first concrete piece of the plan in
[doc/energy-tracking-assessment.md](../../doc/energy-tracking-assessment.md);
read that document for the full gap analysis before using any number this
stack produces in a paper.

No Helm, no kustomize — plain manifests, applied like the rest of `k8s/`:

```bash
kubectl apply -f k8s/monitoring/namespace.yaml
kubectl apply -f k8s/monitoring/kepler-rbac.yaml
kubectl apply -f k8s/monitoring/kepler-daemonset.yaml
kubectl apply -f k8s/monitoring/prometheus-rbac.yaml
kubectl apply -f k8s/monitoring/prometheus-config.yaml
kubectl apply -f k8s/monitoring/prometheus-rules.yaml
kubectl apply -f k8s/monitoring/prometheus-deployment.yaml
```

## What's in scope

- **Kepler** (DaemonSet, one pod per node): reads RAPL (`/sys/class/powercap/intel-rapl`)
  if the host exposes it, otherwise falls back to a model-based power
  estimate. Exposes `/metrics` on `:9102` (`hostNetwork: true`).
- **Prometheus** (single Deployment): scrapes Kepler and any `wasmbed`
  namespace pod that opts in via `prometheus.io/scrape: "true"` annotations.
  `emptyDir` storage, 6h retention — this is a scrape buffer for
  experiment campaigns, not a long-term store. **No Grafana yet** (deferred;
  ask before adding it — see "Open decision" below).

Verify what Kepler is actually reporting after deploy, since its metric
names have changed across releases and RAPL availability is host-dependent:

```bash
kubectl -n wasmbed-monitoring exec -it ds/kepler-exporter -- curl -s localhost:9102/metrics | grep ^kepler_ | head -30
kubectl -n wasmbed-monitoring logs ds/kepler-exporter | grep -i rapl
```

If RAPL isn't found, Kepler logs a warning and switches to its estimator —
the recording rules in `prometheus-rules.yaml` still work but the numbers
are model-based estimates, not direct hardware measurement. **This must be
stated as a limitation in the paper if it's the case on the test host.**

## The one limitation that matters most here

**Kepler measures the physical host the k3s node runs on. That host also
runs the `wasmbed-renode` Docker container** (all emulated edge devices —
STM32F7, nRF52840, Arduino Nano BLE, RISC-V — as Renode "machines" inside
one process; see `doc/ARCHITECTURE.md`). Kepler's node-level totals
(`kepler_node_platform_joules_total`) are **whole-host power, Renode
included** — they do not, and architecturally cannot, isolate "the cluster"
from "the emulated edge devices," because both run on the same silicon.

On top of that, the Renode container is started with a raw `docker run`
from inside the api-server pod
(`crates/wasmbed-qemu-manager/src/lib.rs`), **not** by kubelet/CRI. Kepler's
per-pod attribution (`kepler_container_joules_total{container_namespace=...}`)
is built from the kubelet's pod list, so:

- Every `wasmbed`-namespace k8s pod (gateway, controllers, api-server,
  dashboard) **does** get a `container_namespace="wasmbed"` label — real,
  attributable per-pod energy.
- The Renode container **does not** — it has no Pod object for Kepler to
  attribute it to.

### How to approximate the Renode share anyway

`prometheus-rules.yaml` defines:

| Recording rule | Meaning |
|---|---|
| `wasmbed:kepler_namespace_watts:sum` | Watts attributed to `wasmbed`-namespace pods only |
| `wasmbed:kepler_all_pods_watts:sum` | Watts attributed to any k8s pod on the node |
| `wasmbed:kepler_host_watts:sum` | Whole-host watts (RAPL or estimate) |
| `wasmbed:kepler_unattributed_watts:approx` | `host - all_pods`, i.e. everything Kepler couldn't tie to a kubelet pod |

`wasmbed:kepler_unattributed_watts:approx` is dominated by the Renode
container in this deployment, but it is **not** an exact Renode-only figure
— it also includes containerd/kubelet/OS overhead not attributed to any
pod. Treat it as an **upper bound** on the Renode emulation cost, state that
explicitly wherever it's quoted, and don't subtract a "kubelet overhead"
estimate you haven't actually measured just to make the number look
cleaner.

Example query (via port-forward, see below):

```
wasmbed:kepler_namespace_watts:sum
wasmbed:kepler_unattributed_watts:approx
```

## Querying Prometheus

```bash
kubectl -n wasmbed-monitoring port-forward svc/prometheus 9090:9090
curl -s 'http://127.0.0.1:9090/api/v1/query?query=wasmbed:kepler_namespace_watts:sum' | jq
```

Or open `http://127.0.0.1:9090` for the built-in expression browser (no
Grafana needed for spot-checks).

## Open decision for you

I left Grafana out for now, per your instruction to keep the stack minimal
("valuta anche un setup minimale senza Grafana per ora"). If you want
dashboards rather than raw PromQL for the paper's figures, say so and I'll
add a Grafana Deployment (also plain YAML, no Helm) reading from this same
Prometheus — low complexity, but it's another pod to keep running, so I
didn't add it unasked.
