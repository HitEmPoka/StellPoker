# Unified Logging Schema & Redaction Policy

**Status**: Accepted (drives Issue #509)

This document is the single source of truth for what the coordinator and MPC
node services may and may **not** put into logs. It sits alongside the
grep-based CI check (`scripts/check_log_redaction.py`) and the runtime
redaction layer (`services/coordinator/src/redact.rs`,
`services/node/src/redact.rs`).

---

## 1. Structured logging

Both services emit structured logs through `tracing` / `tracing-subscriber`:

- `REQUEST_LOG_FORMAT=json` → `tracing_subscriber::fmt().json()`
- default → pretty `tracing_subscriber::fmt()`

Every HTTP request is logged by the request-logging middleware
(`services/coordinator/src/middleware.rs`) with a fixed, low-cardinality field
set:

| Field           | Meaning                                   | Sensitive |
|-----------------|-------------------------------------------|-----------|
| `request_id`    | UUID v4 correlation id                    | No        |
| `method`        | HTTP method                               | No        |
| `path`          | URL path                                  | No        |
| `status`        | HTTP status code                          | No        |
| `duration_ms`   | Handler wall-clock time                  | No        |
| `session_id`    | Table/session id extracted from the path  | No        |

Request bodies and query strings are **never** logged. Card *values*, MPC
*secret shares*, and commitment *salts* are never logged under any condition
(see Section 2), and admin/audit logs never contain private game state.

## 2. Forbidden log fields

The following field names must never appear as structured `tracing` fields
(i.e. within `tracing::{info,debug,warn,error}!`):

- **Card values** – `hole_cards`, `hole_card`, `hole_card1`, `hole_card2`,
  `card1`, `card2`, `cards`, `card`, `player_card_positions`
- **Commitment salts** – `salts`, `salt1`, `salt2`, `salt`
- **MPC secret shares** – `shares`, `share`, `share_bundle`, `share_set_id`,
  `share_ids`
- **Credentials / session secrets** – `secret`, `secret_key`, `private_key`,
  `committee_secret`, `ciphertext`, `api_key`, `authorization`, `password`

Public, on-chain data such as `deck_root`, `hand_commitments`,
`board_indices`, and `dealt_indices` are **not** forbidden – they are posted to
the ledger and encoded inside the public inputs of the ZK proofs.

The list lives in two places and must be kept in sync:
`SENSITIVE_FIELD_KEYS` in `redact.rs` (runtime) and `FORBIDDEN` in
`scripts/check_log_redaction.py` (lint).

## 3. Runtime redaction

Both services install a redacting `MakeWriter` around the tracing sink. Every
line passes through `redact::redact_line` before it is written:

- **JSON lines** are parsed and walked recursively; any object key that
  `is_sensitive_key` matches is replaced with `[REDACTED]` at any nesting
  depth.
- **Pretty lines** are rewritten by a regex that matches
  `key="..."`, `key=[...]`, or `key=value` tokens and replaces the value with
  `[REDACTED]`.
- Malformed/truncated lines fall back to the pretty path so a corrupted line
  can never leak a value.

Matching is *boundary aware* (`is_sensitive_key`): `deck_root` is not flagged
by `secret`, `dealt_indices` is not flagged by `card`, while
`player_card_positions` is caught by `card`.

## 4. CI enforcement

`.github/workflows/ci.yml` runs `python3 scripts/check_log_redaction.py` on
every PR and push to `main`. The script:

1. Finds every `tracing::{level}!` invocation (balanced-paren aware).
2. Extracts structured field keys (`key = value`).
3. Fails the build if any key matches the forbidden list (same boundary rules
   as the runtime matcher).

This makes a regression (someone adding a traced `hole_cards` or `salts`
field) a build failure before it can reach production logs.

## 5. If you must record sensitive material

- **Never** write it to the application log stream.
- Store proof-of-facts (hashes) instead of plaintexts. The coordinator already
  stores only *commitments* (`deck_root`, `hand_commitments`) in memory and on
  disk; `session_cache.rs` persists the same public fields plus `session_id`
  scoping.
- When an audit trail of a player action is required, use `audit_log.rs`
  (tamper-evident hash chain) with request/action metadata – never card
  values, shares, or salts.
# Unified logging schema

StellPoker services should emit one JSON object per line so coordinator, MPC node, contract tooling, and dev scripts can be queried together in Loki/Grafana.

## Required fields

| Field | Type | Description |
| --- | --- | --- |
| `timestamp` | string | RFC3339/ISO timestamp generated at log emission time. |
| `level` | string | Lowercase or uppercase log level: `trace`, `debug`, `info`, `warn`, `error`. |
| `service` | string | Stable service name, for example `coordinator`, `mpc-node-0`, `mpc-node-1`, `contract-deployer`. |
| `session_id` | string or null | Poker/MPC session identifier when the log belongs to one game session. |
| `request_id` | string or null | HTTP, websocket, or job request correlation ID. |
| `duration_ms` | number or null | Duration for completed operations. Use `null` when the event is not timed. |

## Recommended fields

| Field | Type | Description |
| --- | --- | --- |
| `message` | string | Human-readable summary. |
| `target` | string | Rust tracing target/module path. |
| `table_id` | number or string | Poker table ID when available. |
| `player` | string | Stellar address or player handle when safe to log. |
| `contract_id` | string | Soroban contract ID for deploy/invoke events. |
| `network` | string | `local`, `testnet`, `mainnet`, or staging network label. |
| `error` | string | Sanitized error summary. Do not log secrets or private cards. |

## Example

```json
{
  "timestamp": "2026-09-24T12:00:00.000Z",
  "level": "info",
  "service": "coordinator",
  "session_id": "table-7-hand-42",
  "request_id": "req_01K5...",
  "duration_ms": 18,
  "message": "deal proof accepted",
  "table_id": 7,
  "contract_id": "CA3R...CHAV",
  "network": "testnet"
}
```

## Rust tracing guidance

Use structured fields instead of embedding key/value pairs inside the message:

```rust
tracing::info!(
    service = "coordinator",
    session_id = %session_id,
    request_id = %request_id,
    duration_ms = elapsed.as_millis() as u64,
    table_id,
    "deal proof accepted"
);
```

For MPC node logs, set `service` to the node identity (`mpc-node-0`, `mpc-node-1`, `mpc-node-2`) so dashboards can split quorum-level failures from a single-node failure.

## Safety rules

- Never log private cards, card shares, CRS material, secret keys, session encryption keys, or bearer/API tokens.
- Hash or redact wallet addresses when a log is intended for public support bundles.
- Include `request_id` on every inbound request and propagate it to background tasks where practical.
- Keep field names stable; add new fields rather than renaming required fields.
