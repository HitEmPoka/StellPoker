# Staging namespace guardrails

`ResourceQuota` and `LimitRange` for the `stellpoker-staging` namespace
(Issue #674). They cap what staging can consume on a shared cluster and make
sure every pod declares resources.

```bash
# Once per cluster, before installing the Helm charts:
kubectl apply -k infrastructure/k8s/staging

# Then install the charts into that namespace (see ../../helm/README.md),
# replacing `--namespace stellpoker` with `--namespace stellpoker-staging`
# and dropping `--create-namespace`.

# Current usage against the quota:
kubectl describe resourcequota -n stellpoker-staging
```

## What is enforced

| Object | Limit | Why |
|--------|-------|-----|
| `stellpoker-staging-compute` | requests 4 CPU / 6Gi, limits 14 CPU / 12Gi, 20 pods | Charts' worst case plus ~30–40% headroom |
| `stellpoker-staging-storage` | 100Gi, 6 PVCs | soroban ledger (20Gi) + circuits/CRS volumes |
| `stellpoker-staging-objects` | 10 services, 0 LoadBalancers, 0 NodePorts, 30 ConfigMaps, 30 Secrets | Traffic goes through the shared ingress |
| `stellpoker-staging-limits` (Container) | default 100m/128Mi request, 500m/512Mi limit; max 2 CPU / 4Gi; limit:request ≤ 10 (CPU), ≤ 8 (memory) | No BestEffort pods; no single pod can starve the rest |
| `stellpoker-staging-limits` (PVC) | 1Gi – 50Gi | Catch typos like `size: 500Gi` |

## How the quota was sized

Worst case = chart `resources` × HPA `maxReplicas` (or `replicaCount`), plus
one surge pod per Deployment during a rolling update. The mpc-node
StatefulSet is fixed at 3 and updates in place.

| Chart | Pods | Requests (CPU / mem) | Limits (CPU / mem) |
|-------|------|----------------------|--------------------|
| coordinator | 3 + 1 surge | 0.8 / 1Gi | 4 / 2Gi |
| frontend | 5 + 1 surge | 0.3 / 384Mi | 1.2 / 768Mi |
| mpc-node | 3 | 1.5 / 1.5Gi | 6 / 6Gi |
| soroban-rpc-proxy | 1 + 1 surge | 0.2 / 512Mi | 1 / 1Gi |
| **Total** | **15** | **2.8 / 3.38Gi** | **12.2 / 9.75Gi** |

`scripts/check_k8s_quota.py` recomputes this table from the chart
`values.yaml` files and runs in CI (`k8s-staging-quota.yml`). If you raise a
chart's resources or replica count and the check fails, raise the quota here
in the same PR, or lower the chart values.

## When a deploy is rejected

`exceeded quota` or `must be less than or equal to ... limit` errors in
`kubectl get events -n stellpoker-staging` mean a pod was refused at
admission. Check `kubectl describe resourcequota -n stellpoker-staging` for
what is in use. A rollout that gets stuck mid-way is usually the surge pod not
fitting; scale down a Deployment or raise the quota.
