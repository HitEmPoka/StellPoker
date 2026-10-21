# Configuration Guide

This document lists all environment variables, CLI flags, and configuration options across all StellPoker services.

## Coordinator Service Environment Variables

### MPC Configuration
- `MPC_NODE_0`, `MPC_NODE_1`, `MPC_NODE_2`: (string) MPC node endpoints. 3 nodes are required for the REP3 protocol. Default: `http://localhost:8101` etc.
- `CIRCUIT_DIR`: (string) Path to compiled Noir circuit artifacts (ACIR). Default: `./circuits`
- `CRS_DIR`: (string) Path to CRS (common reference string) for proof generation. Default: `./crs`

### Stellar & Soroban
- `COMMITTEE_SECRET`: (string) Committee signing key (Stellar secret key, starts with `S`).
- `COMMITTEE_IDENTITY`: (string) Optional Stellar CLI identity label used by staging integration checks.
- `SOROBAN_RPC`: (string) Soroban RPC endpoint URL. Default: `http://localhost:8000/soroban/rpc`
- `POKER_TABLE_CONTRACT`: (string) Deployed poker-table contract ID.
- `NETWORK_PASSPHRASE`: (string) Stellar network passphrase. Default: `Test SDF Network ; September 2015`
- `ONCHAIN_TABLE_ID`: (number) Reference table ID for cloning config on table creation. Default: `0`

### Server Configuration
- `BIND_ADDR`: (string) Coordinator bind address. Default: `0.0.0.0:8080`
- `DATABASE_URL`: (string) PostgreSQL connection URL for the coordinator database. When set, the coordinator persists data. E.g., `postgres://coordinator:password@localhost:5432/coordinator`

### Player Identities (Local/Solo mode)
- `PLAYER1_ADDRESS`, `PLAYER2_ADDRESS`: (string) On-chain addresses for players.
- `PLAYER1_IDENTITY`, `PLAYER2_IDENTITY`: (string) Local signing identities for players.

### Admin RBAC
- `ADMIN_KEYS`: (JSON array) Admin public keys with roles (`super-admin`, `operator`, `read-only`). E.g. `[{"key":"GABCD...","role":"super-admin"}]`
- `ALLOW_INSECURE_DEV_AUTH`: (boolean) Skip auth signature verification. Only for development!

### Optional
- `LOBBY_BUY_IN`: (number) Default buy-in for solo tables in stroops. Default: `1000000000`
- `OPEN_TABLE_SCAN_MAX`: (number) Max table IDs to scan for open tables. Default: `32`
- `FRIENDBOT_URL`: (string) Friendbot URL for testnet top-ups.

## Frontend (Next.js) Variables
- `NEXT_PUBLIC_COORDINATOR_URL`: (string) URL of the coordinator service. Default: `http://localhost:8080`
- `CSP_REPORT_URI`: (string) CSP report endpoint URI. Default: `/api/csp-report`

## Feature Flags
Feature flags can be enabled by setting their value to `1`, `true`, `yes`, or `on`. Disable with `0`, `false`, `no`, or `off`.

- `FEATURE_FLAG_NEW_CIRCUITS`: Enable experimental circuit versions.
- `FEATURE_FLAG_CONTRACT_UPGRADE`: Gate new Soroban contract function calls.
- `FEATURE_FLAG_EXPERIMENTAL_UI`: Signal the UI to render next-gen components.
- `FEATURE_FLAG_CHAT_ENABLED`: Enable in-table WebSocket chat.
- `FEATURE_FLAG_SOLO_MODE`: Allow solo / bot-opponent table creation.

**Overrides:**
You can override feature flags per-table or per-player:
- Per-table suffix: `_TABLE_<id>` (e.g., `FEATURE_FLAG_CHAT_ENABLED_TABLE_3=1`)
- Per-player suffix: `_PLAYER_<stellar-address>` (e.g., `FEATURE_FLAG_EXPERIMENTAL_UI_PLAYER_GABC...=1`)
