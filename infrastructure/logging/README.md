# StellPoker Log Aggregation

Loki + Promtail + Grafana stack for collecting and browsing structured JSON logs from all StellPoker services.

## Stack

| Component | Role | Port |
|-----------|------|------|
| Loki | Log storage and query engine | 3100 |
| Promtail | Log collector / shipper | — |
| Grafana | Dashboard UI | 3000 |

Loki was chosen over ELK for its lower resource footprint and native Grafana integration. Promtail is Loki's purpose-built log shipper.

## Structured JSON Logs

All Rust services emit structured JSON logs when `REQUEST_LOG_FORMAT=json` is set (already configured in `docker-compose.yml`). Each line follows the `tracing-subscriber` JSON format:

```json
{"timestamp":"2024-01-01T12:00:00.000Z","level":"INFO","fields":{"message":"Coordinator listening on 0.0.0.0:8080"},"target":"coordinator"}
```

## Local Development

Start the main stack plus the logging overlay:

```bash
docker-compose \
  -f docker-compose.yml \
  -f infrastructure/logging/docker-compose.logging.yml \
  up
```

Open Grafana at http://localhost:3000 (admin / stellpoker).

The **StellPoker - Service Logs** dashboard is pre-provisioned. Use the `service` and `level` dropdowns to filter logs per container.

## Kubernetes

Apply in order:

```bash
kubectl apply -f infrastructure/logging/k8s/namespace.yaml
kubectl apply -f infrastructure/logging/k8s/loki-deployment.yaml
kubectl apply -f infrastructure/logging/k8s/promtail-daemonset.yaml
kubectl apply -f infrastructure/logging/k8s/grafana-deployment.yaml
```

Promtail runs as a DaemonSet (one pod per node) and reads pod logs from `/var/log/pods/`. It has a `ClusterRole` granting read access to pod metadata for label discovery.

To set a custom Grafana admin password before applying:

```bash
kubectl create secret generic grafana-secret \
  --namespace stellpoker-logging \
  --from-literal=admin-password=<your-password>
```

If the secret is absent, Grafana falls back to its default (`admin`).

## Log Querying (LogQL)

Query from the Grafana Explore tab or the Loki API directly:

```logql
# All logs from the coordinator
{service="stellpoker-coordinator"}

# Errors across all MPC nodes
{service=~"stellpoker-mpc-node-.*", level="ERROR"}

# Request rate per service (last 5 minutes)
sum by (service) (rate({service=~".+"}[5m]))
```
