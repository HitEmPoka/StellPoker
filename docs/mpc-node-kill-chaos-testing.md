# MPC Node-Kill Chaos Test (Issue #643)

This experiment kills one MPC node with `SIGKILL` while a staging table is
running deal proofs. The hypothesis is that the coordinator either recovers the
session with the remaining committee or aborts it cleanly; in neither case may a
partial settlement be written on-chain.

## Prerequisites

- Staging is running `docker-compose.staging.yml` with all three MPC nodes.
- `COORDINATOR_URL` points at the staging coordinator.
- `TABLE_ID` is an isolated staging table not used by other workloads.
- If signed requests are enabled, set `AUTH_HEADERS_JSON` to a JSON object with
  `x-player-address`, `x-auth-signature`, `x-auth-nonce`, and `x-auth-timestamp`.
- Docker and Python 3 are available on the runner.

## Run locally

```bash
COORDINATOR_URL=http://localhost:8080 TABLE_ID=42 \
  ./scripts/run-mpc-node-kill-chaos.sh
```

Useful overrides:

- `MPC_NODE_SERVICE` — compose service to kill (`mpc-node-1` by default).
- `WARMUP_SECONDS` — delay before the kill (5 by default).
- `RECOVERY_TIMEOUT_SECONDS` — committee recovery budget (90 by default).
- `VUS` / `DURATION` — k6 load shape (3 / `2m` by default).
- `COMPOSE_FILE` — path to the staging compose file.

## Run in CI

Dispatch the `MPC Node Kill Chaos (Staging)` workflow with an isolated
`table_id`. The workflow is manually triggered, staging-only, and serialized via
a concurrency group. The killed node is restarted automatically by the script's
cleanup trap.

## Expected outcomes

Both outcomes pass:

- **Recovery**: `/api/committee/status` reports at least two healthy nodes,
  in-flight deal sessions terminate, and later deal requests succeed.
- **Safe abort**: deal requests are refused or session statuses end as
  `failed`, `cancelled`, or `timed_out`, with no on-chain settlement.

The test fails if k6 reports unexpected HTTP statuses, a session never
terminates, the hand-history count changes, or the table enters `settlement`
after the interrupted deal. Evidence is written to
`load-testing/results/mpc-node-kill-chaos-summary.json`, the baseline/final
state snapshots, and the k6 log.

## Rollback

The cleanup trap restarts the killed compose service. If the coordinator does
not return to a healthy committee, restart the full staging stack with:

```bash
docker compose -f docker-compose.staging.yml up -d
```
