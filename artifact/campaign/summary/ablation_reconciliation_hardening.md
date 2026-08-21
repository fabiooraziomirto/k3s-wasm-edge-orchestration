# Ablation: reconciliation hardening

## Method
- **Hardened**: application-controller active (replicas=1).
- **Unhardened**: `kubectl scale deployment/wasmbed-application-controller -n wasmbed --replicas=0`.
- Code path: `crates/wasmbed-application-controller/src/main.rs` `handle_running()` repairs stale Application CRD phase.

## Comparison

| Metric | Hardened | Unhardened |
|--------|----------|------------|
| Transactional success | 100/100 (100.0%) | 30/30 (100.0%) |
| Wilson 95% CI lower bound | 96.3% | 88.6% |
| Deployment success | 100/100 | 30/30 |
| Non-Running CRD phases (unhardened) | 0 | 0 |
| Mean deployment latency | 936.8 ms | 944.2 ms |

## Interpretation
Without reconciliation hardening, gateway-acknowledged deployments can leave stale Application CRD phases, breaking the authoritative success criterion used in the hardened campaign.
