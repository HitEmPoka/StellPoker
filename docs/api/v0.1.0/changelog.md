# Changelog — v0.1.0 (2026-05-16)

Initial release of the StellPoker Coordinator API.

## Added

- **Health & Monitoring**
  - `GET /api/health` — service health check with MPC node status, Soroban RPC connectivity
  - `GET /api/stats` — global statistics with 30s TTL cache
  - `GET /metrics` — Prometheus metrics (request counts, latency histograms, CPU/memory)

- **Chain Configuration**
  - `GET /api/chain-config` — public Stellar network parameters

- **Feature Flags**
  - `GET /api/flags` — list all runtime feature flags
  - `POST /api/flags/{key}` — set/override a flag value

- **Poker Table Lifecycle**
  - `POST /api/tables/create` — create on-chain table (supports solo mode)
  - `GET /api/tables/open` — list tables in "Waiting" phase
  - `POST /api/table/{id}/join` — register wallet-to-seat mapping
  - `GET /api/table/{id}/lobby` — lobby seat information
  - `GET /api/table/{id}/state` — full on-chain table state

- **MPC Game Operations**
  - `POST /api/table/{id}/request-deal` — trigger MPC shuffle & deal
  - `POST /api/table/{id}/request-reveal/{phase}` — reveal community cards (flop/turn/river)
  - `POST /api/table/{id}/request-showdown` — evaluate hands, distribute pot
  - `POST /api/table/{id}/player-action` — submit betting actions
  - `GET /api/table/{id}/player/{address}/cards` — get authenticated player's hole cards

- **MPC Committee**
  - `GET /api/committee/status` — MPC node health status

- **Real-time Chat**
  - `GET /api/table/{id}/chat/ws` — WebSocket per-table chat

- **MPC Session Management**
  - `POST /api/session/{id}/cancel` — (deprecated) manual session cancellation
  - `GET /api/session/{id}/status` — session status with timeout detection

- **Wallet Authentication**
  - `POST /api/wallet/challenge` — obtain signing challenge
  - `POST /api/wallet/verify` — verify wallet signature (SEP-53 compatible)

- **Admin API (RBAC: read-only / operator / super-admin)**
  - `GET /api/admin/health` — detailed health info
  - `GET /api/admin/sessions` — list all MPC sessions
  - `POST /api/admin/sessions/{id}/cancel` — cancel a session
  - `POST /api/admin/sessions/cleanup` — force-cleanup stale sessions
  - `GET /api/admin/stats` — per-route metrics and system health
  - `POST /api/admin/config/reload` — reload admin keys from env
  - `GET /api/admin/rate-limits` — list rate limit configs
  - `POST /api/admin/rate-limits` — create/update rate limit
  - `DELETE /api/admin/rate-limits/{id}` — delete rate limit
  - `GET /api/admin/cors` — list CORS origin configs
  - `POST /api/admin/cors` — create/update CORS origin
  - `DELETE /api/admin/cors/{id}` — delete CORS origin
  - `GET /api/admin/audit-logs` — query tamper-evident audit logs
  - `POST /api/admin/audit-logs/verify` — verify audit chain integrity
  - `GET /api/admin/migrations` — list pending session migrations
  - `POST /api/admin/migrations/initiate` — initiate session migration
  - `POST /api/admin/migrations/{id}/complete` — complete migration
  - `POST /api/admin/migrations/{id}/cancel` — cancel migration

## Authentication

- Wallet-based auth using Ed25519 signatures (supports raw and SEP-53)
- Admin RBAC with three tiers: read-only, operator, super-admin
- Replay protection via strictly increasing nonces
- 300-second timestamp skew window
- Dev mode: `ALLOW_INSECURE_DEV_AUTH=1` skips signature verification

## Breaking Changes

None — this is the initial release.
