# MPC Coordinator Service

This service orchestrates the MPC committee for game proving and card shuffling.

## Hot Reload

Install `cargo-watch` once:

```bash
cargo install cargo-watch
```

Run the coordinator with Rust hot reload from the repository root:

```bash
./scripts/dev-coordinator-watch.sh
```

The watcher recompiles and restarts `cargo run -p coordinator` when coordinator
Rust sources, migrations, or Cargo manifests change. During hot reload the
coordinator writes table sessions and lobby assignments to
`.tmp/coordinator-hot-reload.json` every few seconds and restores that snapshot
on restart. Persistent chain state is still rehydrated from Soroban when a
session is missing from memory.

## Safety checks

| Variable | Default | Effect |
|---|---|---|
| `SOROBAN_PRESIMULATE` | `true` | Simulate each state-changing `stellar contract invoke` with `--send no`, and submit only if the simulation succeeds. Failed simulations are logged and never submitted. Read-only calls (`get_*`, `is_*`, `has_*`) skip this step. See `src/soroban/simulation.rs`. |
| `SOROBAN_SIMULATION_CACHE_TTL_SECS` | `10` | How long a simulation result is reused for an identical call. `0` disables the cache. Transient failures are never cached. |
| `MPC_RECONSTRUCTION_VALIDATION` | `enforce` | Check reconstructed hole cards against the deal proof's hand commitment (Poseidon2). `enforce` retries up to 3 times, then rejects the request with `502`. `warn` only logs a mismatch. `off` skips the check. After 3 failed requests in a row for one table, it logs an error with `alert = "mpc_reconstruction_failure"`. See `src/mpc_validation.rs`. |

Table chat (`/api/table/:table_id/chat/ws`) accepts only frames with a known schema. Text is reduced to plain text: tags, angle brackets, and control and bidi characters are removed, and it's truncated to 128 characters and aliases to 24. Emotes must be in the allowlist. Each connection may send 5 frames per 10 seconds. See `src/chat.rs`; the web client applies the same rules with DOMPurify in `app/src/lib/chat-sanitize.ts`.

## API Endpoints

### GET `/api/health`

Returns the operational metrics and connectivity status of the coordinator, MPC nodes, and Soroban RPC network.

#### Sample Response

```json
{
  "uptime_seconds": 1284,
  "mpc_nodes": [
    {
      "endpoint": "http://localhost:8101",
      "connected": true,
      "last_heartbeat": "2026-06-23T16:32:00.123Z"
    },
    {
      "endpoint": "http://localhost:8102",
      "connected": true,
      "last_heartbeat": "2026-06-23T16:32:00.456Z"
    },
    {
      "endpoint": "http://localhost:8103",
      "connected": true,
      "last_heartbeat": "2026-06-23T16:32:00.789Z"
    }
  ],
  "soroban_rpc": {
    "endpoint": "http://localhost:8000/soroban/rpc",
    "status": "connected"
  },
  "active_mpc_sessions": 0,
  "request_metrics": {
    "POST /api/tables/create": {
      "count": 3,
      "errors": 0,
      "latency_histogram": {
        "under_50ms": 0,
        "under_250ms": 2,
        "under_1000ms": 1,
        "under_5000ms": 0,
        "over_5000ms": 0
      }
    }
  }
}
```
