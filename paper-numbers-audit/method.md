# Come sono stati prodotti i numeri "dopo"

## Ambiente

Host di laboratorio (lo stesso della campagna originale): 32 vCPU, 62 GiB RAM,
K3s v1.35.4+k3s1, Renode `antmicro/renode:nightly`, board `Stm32F746gDisco`,
Zephyr 3.5.0 + WAMR.

## Configurazione verificata prima di misurare

La campagna del paper è la variante "hardened" (con application controller
attivo); l'ablation scala quel controller a zero. Prima di ogni misura vanno
quindi verificati:

```bash
kubectl get deploy -n wasmbed          # gateway, api-server, application-controller a 1/1
./scripts/run-e2e-fleet.sh --check     # prerequisiti dell'emulazione
```

Al primo tentativo l'application controller era in `ImagePullBackOff`: la
campagna avrebbe misurato la configurazione dell'ablation credendo di misurare
quella completa. L'immagine va ricostruita con il tag che il deployment
referenzia e importata in containerd (k3s non usa il daemon Docker):

```bash
IMG=$(kubectl -n wasmbed get deploy wasmbed-application-controller \
        -o jsonpath='{.spec.template.spec.containers[0].image}')
docker build -t "$IMG" -f Dockerfile.application-controller .
docker save "$IMG" | k3s ctr images import -
kubectl -n wasmbed rollout restart deploy/wasmbed-application-controller
```

## Misure

**Latenze e tassi di successo** (n=100), con i device emulati connessi:

```bash
./scripts/run-e2e-fleet.sh --skip-build -n 3
python3 scripts/collect_experiment_metrics.py --trials 100 --output-dir /tmp/after
python3 scripts/postprocess_experiment.py --input <json> --output-dir /tmp/after --label after
```

**Footprint firmware**: dal report del linker a fine build e dalla dimensione
dell'immagine.

```bash
ls -l build/stm32f746g_disco/zephyr/zephyr.bin
sed -n '/Memory region/,/IDT_LIST/p' /tmp/west-build.log
```

Attenzione al metodo: il paper riporta la RAM statica come data+bss stimati,
il linker riporta l'occupazione della regione RAM. Sono due grandezze diverse
e non vanno confrontate direttamente.

**Risorse control-plane**: `kubectl top pod -n wasmbed`, 5 campioni.
`metrics-server` arrotonda a 1 mc / 1 Mi, quindi i valori bassi hanno una
granularità grossolana; i campioni vanno presi durante il carico, non a riposo.

## Cosa non è stato rimisurato

- Tabella energia per workload: misura il costo di esecuzione WASM, indipendente
  dalle modifiche fatte.
- Esercizio del registro a 50 board: resta un test sullo stato del registro.
- Ablation (n=30): richiede di scalare a zero l'application controller e
  ripetere la campagna.
