# Load Testing Environment

This folder contains a local `k6` load testing environment with Grafana dashboards and Prometheus scraping for the coordinator service.

## What is included

- `docker-compose.yml` starts:
  - InfluxDB for k6 result storage
  - Prometheus for scraping coordinator metrics from `/metrics`
  - Grafana for real-time dashboards
  - k6 container for running benchmark scripts
- `k6/benchmark.js` simulates concurrent table/player activity with configurable
  session status, action submission, proof submission, and state queries
- `k6/lobby-5000.js` loads a 5000-table lobby and enforces the recorded p95/p99
  SLOs (see [Large lobby scenario](#large-lobby-scenario-5000-tables))
- `lobby-fixture/server.mjs` is a dependency-free deterministic server that
  generates the large lobby payload for the scenario
- Grafana dashboards are provisioned automatically from `grafana/dashboards`

## Start the environment

From the repository root:

```bash
cd load-testing
docker compose up -d
```

Then open:

- Grafana: `http://localhost:3000` (`admin` / `admin`)
- Prometheus: `http://localhost:9090`
- InfluxDB: `http://localhost:8086`

## Run a load test

Use the `k6` container to execute the benchmark against your locally-running coordinator service.

Example: 100 concurrent tables

```bash
cd load-testing
docker compose run --rm --entrypoint k6 -e BASE_URL=http://host.docker.internal:8080 -e TABLE_COUNT=100 -e VUS=100 -e DURATION=2m k6 run --out influxdb=http://influxdb:8086/k6 /scripts/benchmark.js
```

Example: 500 concurrent tables

```bash
cd load-testing
docker compose run --rm --entrypoint k6 -e BASE_URL=http://host.docker.internal:8080 -e TABLE_COUNT=500 -e VUS=500 -e DURATION=3m k6 run --out influxdb=http://influxdb:8086/k6 /scripts/benchmark.js
```

Example: 1000 concurrent tables

```bash
cd load-testing
docker compose run --rm --entrypoint k6 -e BASE_URL=http://host.docker.internal:8080 -e TABLE_COUNT=1000 -e VUS=1000 -e DURATION=4m k6 run --out influxdb=http://influxdb:8086/k6 /scripts/benchmark.js
```

To exercise a real session and write endpoints, provide a session identifier,
the table-relative action/proof paths, and JSON request bodies. The paths and
bodies are deliberately environment variables because valid payloads depend
on the deployed table and player accounts:

```bash
docker compose run --rm --entrypoint k6 \
  -e BASE_URL=http://host.docker.internal:8080 \
  -e SESSION_ID="$SESSION_ID" \
  -e ACTION_PATH="/api/table/$TABLE_ID/player-action" \
  -e ACTION_BODY='{"player":"...","action":"check"}' \
  -e PROOF_PATH="/api/table/$TABLE_ID/request-showdown" \
  -e PROOF_BODY='{}' \
  -e VUS=100 -e DURATION=3m \
  k6 run --out influxdb=http://influxdb:8086/k6 /scripts/benchmark.js
```

## Finding the throughput limit

Run the same workload at increasing `VUS` values (for example 100, 250, 500,
and 1000), keeping the table count and duration fixed. Record requests per
second, p95/p99 latency, error rate, coordinator CPU/memory, and MPC session
queue depth from Grafana. Treat the sustainable limit as the highest level
that remains below the 5% error threshold and the p99 latency threshold for
the full run; report the first level that violates either threshold as the
observed saturation point. These are environment-specific measurements, not
hard protocol limits, so include CPU, memory, database, MPC-node count, and
commit SHA with every result.

## Large lobby scenario (5000 tables)

`k6/lobby-5000.js` fetches the full open-tables payload at a constant arrival
rate and validates that every response is a 200 with the expected number of
tables. The fixture in `lobby-fixture/server.mjs` regenerates the JSON on every
request, so serialization, transfer, and client-side parsing are all measured.

Recorded SLO for this scenario:

| Metric | SLO |
|---|---|
| p95 latency | **< 750 ms** |
| p99 latency | **< 1500 ms** |
| Error rate | **< 1%** |
| Payload size | non-empty and < 10 MB |

Run it against the deterministic fixture:

```bash
cd load-testing
./run-lobby-loadtest.sh
```

Or against a coordinator that has at least 5000 open tables
(`OPEN_TABLE_SCAN_MAX=5000`):

```bash
cd load-testing
BASE_URL=http://host.docker.internal:8080 RATE=50 DURATION=2m \
  ./run-lobby-loadtest.sh
```

Override `TABLE_COUNT`, `RATE`, `DURATION`, `VUS`, and `MAX_VUS` with
environment variables. k6 fails the run when a threshold is breached and writes
the observed p50/p95/p99, error rate, and payload size to
`k6/results/lobby-5000-summary.json` (printed to stdout as well). Attach that
file to the load-test report and include the coordinator commit SHA, CPU/memory
limits, database, and MPC-node count when comparing runs.

The fixture can also be started on its own:

```bash
docker compose up -d lobby-fixture
curl -s http://localhost:8090/api/tables/open?tables=5000 | jq '.tables | length'
```

## Metrics reported

- P95 and P99 latency from k6 (including the 5000-table lobby scenario)
- Error rate from k6
- Throughput from k6
- Lobby payload size and table count from the lobby scenario
- Coordinator CPU usage
- Coordinator memory usage

## Notes

- The coordinator must be running locally and reachable on `http://localhost:8080`
- `host.docker.internal` is used for container-to-host networking on Docker Desktop
- If Grafana does not automatically load the dashboard, refresh the dashboards list in Grafana
