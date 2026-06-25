# StellPoker Helm Charts

Production Kubernetes Helm charts for all StellPoker services.

| Chart | Kind | Scales |
|-------|------|--------|
| `coordinator` | Deployment | Yes (HPA, CPU + memory) |
| `mpc-node` | StatefulSet | Fixed at 3 (protocol constraint) |
| `frontend` | Deployment | Yes (HPA, CPU + memory) |
| `soroban-rpc-proxy` | Deployment | Single instance |

## Prerequisites

- Kubernetes 1.25+
- Helm 3.10+
- `metrics-server` installed (for HPA)
- A container registry with built images (see below)

## Building images

```bash
# Coordinator
docker build -f services/coordinator/Dockerfile -t ghcr.io/<org>/stellpoker/coordinator:latest .

# MPC node
docker build -f services/node/Dockerfile -t ghcr.io/<org>/stellpoker/mpc-node:latest .

# Frontend
docker build -f app/Dockerfile -t ghcr.io/<org>/stellpoker/frontend:latest app/
```

## Installing the charts

### 1. soroban-rpc-proxy

```bash
helm install soroban-rpc-proxy ./infrastructure/helm/soroban-rpc-proxy \
  --namespace stellpoker \
  --create-namespace \
  --set stellarNetwork=testnet
```

### 2. mpc-node

Party configs contain per-node key material. Store them in a Secret if they contain private keys:

```bash
kubectl create secret generic mpc-node-party-configs \
  --namespace stellpoker \
  --from-file=party_0.toml=services/node/config/party_0.toml \
  --from-file=party_1.toml=services/node/config/party_1.toml \
  --from-file=party_2.toml=services/node/config/party_2.toml
```

```bash
helm install mpc-node ./infrastructure/helm/mpc-node \
  --namespace stellpoker \
  --set partyConfigs.useSecret=true \
  --set volumes.circuits.existingClaim=circuits-pvc \
  --set volumes.crs.existingClaim=crs-pvc
```

The StatefulSet creates pods `mpc-node-0`, `mpc-node-1`, `mpc-node-2`. Each pod derives its `NODE_ID` from the pod ordinal via `${HOSTNAME##*-}`. The headless service `mpc-node-headless` gives each pod a stable DNS name for peer-to-peer MPC TCP communication.

### 3. coordinator

```bash
helm install coordinator ./infrastructure/helm/coordinator \
  --namespace stellpoker \
  --set mpcNodes.node0="http://mpc-node-0.mpc-node-headless.stellpoker.svc.cluster.local:8101" \
  --set mpcNodes.node1="http://mpc-node-1.mpc-node-headless.stellpoker.svc.cluster.local:8101" \
  --set mpcNodes.node2="http://mpc-node-2.mpc-node-headless.stellpoker.svc.cluster.local:8101" \
  --set sorobanRpc="http://soroban-rpc-proxy.stellpoker.svc.cluster.local:8000/soroban/rpc" \
  --set existingSecret=coordinator-secret \
  --set volumes.circuits.existingClaim=circuits-pvc \
  --set volumes.crs.existingClaim=crs-pvc
```

Enable HPA (coordinator scales horizontally; note: in-memory session state means sticky sessions or external session storage is recommended at scale):

```bash
helm upgrade coordinator ./infrastructure/helm/coordinator \
  --namespace stellpoker \
  --set autoscaling.enabled=true \
  --set autoscaling.minReplicas=2 \
  --set autoscaling.maxReplicas=5
```

### 4. frontend

```bash
helm install frontend ./infrastructure/helm/frontend \
  --namespace stellpoker \
  --set coordinatorUrl="http://coordinator.stellpoker.svc.cluster.local:8080" \
  --set autoscaling.enabled=true
```

## Resource summary

| Chart | CPU request | CPU limit | Mem request | Mem limit |
|-------|-------------|-----------|-------------|-----------|
| coordinator | 200m | 1 | 256Mi | 512Mi |
| mpc-node (×3) | 500m | 2 | 512Mi | 2Gi |
| frontend | 50m | 200m | 64Mi | 128Mi |
| soroban-rpc-proxy | 100m | 500m | 256Mi | 512Mi |

## Network policies

All charts deploy `NetworkPolicy` resources (enabled by default). Traffic is restricted to:

- **coordinator** — accepts ingress from frontend pods and ingress controllers on `:8080`; egress to mpc-node pods on `:8101` and soroban-rpc-proxy on `:8000`
- **mpc-node** — accepts ingress from coordinator on `:8101` and from peer mpc-node pods on `:10000`; egress only to peer mpc-node pods on `:10000`
- **frontend** — accepts ingress on `:3000`; egress to coordinator on `:8080` and external HTTPS on `:443`
- **soroban-rpc-proxy** — accepts ingress from coordinator only on `:8000`; egress on Stellar peer ports and `:443`

Disable network policies per chart with `--set networkPolicy.enabled=false`.
