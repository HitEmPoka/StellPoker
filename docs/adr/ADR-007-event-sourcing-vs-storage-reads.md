# ADR-007: Event Sourcing vs Storage Reads in the Indexer Path

- Status: Accepted
- Date: 2026-10-16
- Context: `GET /api/stats` needs global/leaderboard aggregates;
  game flows need authoritative per-table state.

## Decision

Hybrid:

- **Derived aggregates via event sourcing.** The stats indexer
  (`services/coordinator/src/stats.rs`) polls Horizon `contract_events`
  (`spawn_indexer`/`poll_once`, paging cursor, 30 s served cache at
  `GET /api/stats`) and folds `hand_started`, `player_joined`, and
  `rake_withdrawn` events into global + per-player stats.
- **Authoritative state via direct storage reads.** Per-table flows
  (`get_table_state`, `fetch_onchain_table_view`, create/join/lobby,
  session building in `services/coordinator/src/soroban/`,
  `src/api/session.rs`) invoke the contract and read current
  `TableState` instead of replaying events.

## Tradeoffs

| | Event sourcing (Horizon) | Direct storage reads (RPC invoke) |
|---|---|---|
| Freshness | Eventual (poll interval + TTL) | Current-ledger |
| History | Natural (cursor over events) | None (latest only) |
| Dependencies | Horizon availability/format | Soroban RPC availability |
| Correctness risk | Decoder drift (XDR topics), missed pages | None beyond RPC |
| Cost | One poller, cheap fan-out | One call per read |

## Consequences

- Stats may lag; never use them for settlement — pot/rake truth stays
  on-chain (`pot.rs`, `rake_balance`).
- Keep event decoders (`decode_symbol`, `decode_address`,
  `decode_i128_as_i64`) covered by tests; a Horizon shape change must
  not silently zero the leaderboard.
- If Horizon is down, `/api/stats` serves the last cached snapshot
  while table flows keep working via storage reads, and vice versa.
