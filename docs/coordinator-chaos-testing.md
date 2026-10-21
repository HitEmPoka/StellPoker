# Coordinator Restart Chaos Test

The staging-only restart test validates that the coordinator recovers while
the service is receiving continuous requests. It starts a k6 workload, waits
for warm-up, restarts the Kubernetes coordinator deployment, waits for both
rollout readiness and the public health endpoint, and records recovery time in
`load-testing/results/coordinator-restart-chaos-summary.json`.

## Running in GitHub Actions

Run **Coordinator Restart Chaos (Staging)** manually. The protected `staging`
environment must provide:

- `STAGING_COORDINATOR_URL`: environment variable containing the coordinator
  base URL.
- `STAGING_KUBE_NAMESPACE`: optional variable, defaulting to
  `stellpoker-staging`.
- `STAGING_COORDINATOR_DEPLOYMENT`: optional variable, defaulting to
  `coordinator`.
- `STAGING_KUBE_CONFIG_B64`: environment secret containing a base64-encoded
  kubeconfig with permission to restart and observe only the staging
  coordinator deployment.

The workflow is manually dispatched and serialized with a staging concurrency
group so two destructive experiments cannot run simultaneously. The load
test uses explicit thresholds for error rate, health-check success, and p99
latency. A run fails when the rollout does not become ready, the health check
does not recover within the timeout, or k6 breaches a threshold.

The test is intentionally bounded to staging. It does not run on pushes or
pull requests and requires the protected environment approval configured by
the repository maintainers.
