# Self-Hosted Soroban RPC Node Guide

This guide covers a production-oriented setup for running your own Soroban RPC stack for StellPoker. A self-hosted RPC endpoint gives you predictable latency, control over upgrade timing, and direct visibility into Soroban ingestion health.

Stellar RPC does not run in isolation. In production, treat it as part of a small data plane:

```text
Stellar Core -> Horizon ingestion -> Soroban RPC -> Load balancer / TLS proxy -> StellPoker services
```

Use Horizon when you need account, transaction, and historical REST data alongside Soroban RPC. Use Soroban RPC for contract simulation, sending transactions, events, and contract state access.

## Recommended Topology

For mainnet production, start with:

1. One dedicated `stellar-core` node for ingestion.
2. One dedicated Horizon ingestion node backed by PostgreSQL.
3. Two Soroban RPC nodes behind a load balancer.
4. One reverse proxy or cloud load balancer terminating TLS and enforcing rate limits.
5. Centralized metrics, logs, and alerting.

Avoid colocating everything on one VM once you move beyond low-volume testing. Horizon ingestion and RPC traffic compete for disk, CPU, and database capacity.

## Hardware Requirements

The figures below are operational starting points for production. They are conservative recommendations for StellPoker-style contract traffic, not protocol-level minimums from SDF.

### Stellar Core

- `8 vCPU`
- `32 GB RAM`
- `1-2 TB` NVMe SSD
- `1 Gbps` network

### Horizon ingestion

- `8-16 vCPU`
- `32-64 GB RAM`
- PostgreSQL on dedicated NVMe storage
- `500 GB+` database volume to leave room for growth, reindexing, and maintenance

### Soroban RPC API node

- `4-8 vCPU`
- `16-32 GB RAM`
- `200-500 GB` SSD
- `1 Gbps` network

### High-availability notes

- Run at least `2` RPC instances behind a load balancer.
- Put PostgreSQL on managed HA storage or a replicated cluster.
- Keep Core, Horizon, and RPC on the same region and low-latency network.
- Reserve headroom for protocol upgrades, catchup, and replay bursts.

## Stellar Core Configuration

Soroban RPC depends on a healthy Core node underneath the ingestion pipeline. Keep Core dedicated to network participation and ledger storage; do not expose it directly to applications.

### Core configuration checklist

- Set the correct `NETWORK_PASSPHRASE` for the target network.
- Use PostgreSQL instead of SQLite for production.
- Store buckets and ledger state on fast local SSD.
- Keep history archives configured so the node can recover and catch up cleanly.
- Bind admin and peer ports only to the networks that need them.
- Use systemd or container orchestration to restart Core automatically.

### Example `stellar-core.cfg`

This is an example starting point. Adjust quorum, peers, and history settings to your environment and the current Stellar release guidance.

```cfg
NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
NODE_IS_VALIDATOR=false
RUN_STANDALONE=false

HTTP_PORT=11626
PEER_PORT=11625

DATABASE="postgresql://stellar:CHANGE_ME@127.0.0.1:5432/stellar_core?sslmode=disable"
BUCKET_DIR_PATH="/var/lib/stellar-core/buckets"
TMP_DIR_PATH="/var/lib/stellar-core/tmp"

KNOWN_PEERS=[
  "core-live-a.stellar.org:11625",
  "core-live-b.stellar.org:11625",
  "core-live-c.stellar.org:11625"
]

[HISTORY.h1]
get="curl -sf http://history.stellar.org/prd/core-live/core_live_001/{0} -o {1}"
```

### Core operational guidance

- Run Core as a non-validator unless you are intentionally operating validator infrastructure.
- Keep `HTTP_PORT` on a private interface or private subnet.
- Watch bucket growth and disk latency closely; Core performance degrades quickly on slow disks.
- Test catchup and replay procedures before mainnet cutover.

## Horizon Setup

Horizon remains useful for StellPoker operations even though contract calls go through Soroban RPC. It gives you classic account, payments, and transaction history APIs that are still valuable for wallets, ops tooling, and incident response.

### Recommended Horizon layout

Split Horizon into roles:

1. One ingestion node connected to Core and PostgreSQL.
2. One or more API nodes reading from the same PostgreSQL database.

This keeps ingestion isolated from user-facing query traffic.

### Horizon production checklist

- Use PostgreSQL, not SQLite.
- Keep the ingestion process on a dedicated host or workload class.
- Enable captive Core or connect Horizon to a reliable Core process per the current Horizon admin guide.
- Put Horizon API nodes behind the same private network boundary as RPC.
- Back up PostgreSQL and test restore times.
- Monitor database bloat, long-running queries, and ingestion lag.

### Database guidance

- Put PostgreSQL on fast SSD-backed storage.
- Size connections separately for ingestion and API traffic.
- Enable automated vacuuming and regular index maintenance.
- Track WAL growth during heavy ingestion or replay events.

## Soroban RPC Endpoint Tuning

Stellar's RPC admin guide recommends using a TOML configuration file for production rather than long CLI flag lists. Keep the RPC service stateless and scale it horizontally.

### RPC deployment recommendations

- Run `2+` identical RPC instances behind a load balancer.
- Terminate TLS at NGINX, Envoy, HAProxy, or a managed L7 balancer.
- Expose the JSON-RPC endpoint publicly, but keep the admin endpoint private.
- Enable Prometheus scraping from the RPC admin endpoint.
- Pin exact RPC versions and upgrade deliberately after validating against your contracts.

### Request-path tuning

- Set reverse-proxy timeouts high enough for `simulateTransaction` and event queries.
- Use HTTP keep-alives between the proxy and RPC instances.
- Enable gzip or brotli for JSON responses if your proxy supports it.
- Add per-IP and per-key rate limits to protect simulation-heavy methods.
- Cap request body size so malformed or abusive payloads fail early.
- Prefer horizontal scaling over very large single-node RPC instances.

### Example NGINX front door

```nginx
upstream soroban_rpc {
    server 10.0.10.11:8000 max_fails=3 fail_timeout=30s;
    server 10.0.10.12:8000 max_fails=3 fail_timeout=30s;
    keepalive 64;
}

limit_req_zone $binary_remote_addr zone=rpc_per_ip:10m rate=20r/s;

server {
    listen 443 ssl http2;
    server_name rpc.example.com;

    ssl_certificate     /etc/letsencrypt/live/rpc.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/rpc.example.com/privkey.pem;

    client_max_body_size 1m;

    location / {
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $host;
        proxy_read_timeout 120s;
        proxy_connect_timeout 5s;
        proxy_send_timeout 30s;
        proxy_pass http://soroban_rpc;
        limit_req zone=rpc_per_ip burst=40 nodelay;
    }
}
```

Tune the timeout and rate numbers to your workload. For StellPoker, simulation and event polling are typically the hottest paths.

### Scaling guidance

- Keep RPC instances stateless so replacements are cheap.
- Scale out first when p95 latency rises under concurrent simulations.
- If event queries dominate, separate public polling traffic from trusted backend traffic with distinct balancers or hostnames.

## Monitoring

You want alerts on ingestion health before players see stale contract state.

### Minimum metrics to collect

#### Stellar Core

- Ledger close latency
- Catchup status
- Peer count
- Bucket storage usage
- Disk latency and filesystem free space
- Process restarts

#### Horizon

- Ingestion lag
- Ingestion errors
- PostgreSQL CPU, IOPS, locks, and replication lag
- API latency and error rate
- Captive Core health if enabled

#### Soroban RPC

- Request rate by method
- p50/p95/p99 latency
- `5xx` rate
- Upstream saturation
- Ledger freshness
- Admin endpoint health and scrape success

### Alerts worth paging on

- RPC is serving ledgers older than your freshness threshold.
- Horizon ingestion stops advancing.
- Core falls behind the network.
- PostgreSQL disk or WAL usage grows unexpectedly fast.
- RPC error rate spikes for `simulateTransaction` or `sendTransaction`.

### Logging guidance

- Emit structured JSON logs from RPC, Horizon, and proxies.
- Include request IDs so a wallet transaction can be traced across proxy, RPC, and app logs.
- Keep audit logs for version upgrades, config changes, and failovers.

## Upgrade and Change Management

- Stage Core, Horizon, and RPC upgrades in a non-production environment first.
- Read the current network software-version matrix before any mainnet rollout.
- Upgrade one RPC instance at a time behind the load balancer.
- Validate `simulateTransaction`, `sendTransaction`, contract event queries, and ledger freshness before continuing.
- Schedule catchup and replay drills at least once before launch.

## StellPoker Integration Notes

- Point `SOROBAN_RPC` at your load-balanced RPC hostname, not a single node.
- Keep coordinator and any backend workers on the same region as your RPC tier.
- If you ingest contract events for analytics or support tooling, isolate that polling from gameplay-critical traffic.
- Treat RPC latency regressions as gameplay regressions because they directly affect wallet prompts and move submission time.

## Sources

This guide was checked against Stellar primary sources on June 24, 2026:

- Stellar RPC admin guide: <https://developers.stellar.org/docs/data/apis/rpc/admin-guide>
- Horizon admin guide: <https://developers.stellar.org/docs/data/apis/horizon/admin-guide>
- Stellar validator/admin configuration guide: <https://developers.stellar.org/docs/validators/admin-guide/configuring>
- `stellar-rpc` repository: <https://github.com/stellar/stellar-rpc>
- `stellar-core` example configuration: <https://github.com/stellar/stellar-core/blob/master/docs/stellar-core_example.cfg>
