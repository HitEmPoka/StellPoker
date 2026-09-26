# Contract Metrics

`poker-table` exposes cheap aggregate views for dashboards. Each one reads a
running counter; none scans tables, seats or history.

## Views

| View | Returns | Reads |
|------|---------|-------|
| `get_contract_metrics()` | `ContractMetrics { tables_created, hands_played, total_rake, active_seats }` | Instance storage only: the metrics record and the table id allocator |
| `get_table_metrics(table_id)` | `TableMetrics { hands_played, total_rake, active_seats }` | The table entry and one counter entry |

Both are O(1): the cost does not depend on how many tables, seats or hands exist.
`metrics_test::reading_metrics_costs_the_same_however_much_the_contract_has_done`
measures this.

### What each counter means

| Counter | Meaning |
|---------|---------|
| `tables_created` | Tables ever created. Read from the id allocator, so it is exact even for tables that pre-date the counters. |
| `hands_played` | Settlements archived. Equals `get_hand_history_meta(table).total_archived` for a table, and counts one per archived entry (a run-it-twice hand archives once per run). |
| `total_rake` | Lifetime rake, including the jackpot share. It is cumulative: `withdraw_rake` empties the spendable balance (`get_rake_balance`) but never reduces this. |
| `active_seats` | Players holding a seat. Queued players are not counted until they are seated. |

The counters cover activity since the version that introduced them was deployed.
Tables that already existed start at zero for `hands_played`, `total_rake` and
`active_seats` and catch up as they settle hands and reseat players.

## Update costs

A counter is only as cheap as the writes that maintain it, so these are the
storage operations each entrypoint gains. Nothing else changes.

| Entrypoint | Added work |
|------------|------------|
| `join_table` (direct seat) | Read-modify-write of the metrics record (instance) |
| `leave_table` | Same, once. A seat filled from the queue in the same call adds no extra write. |
| Any hand settlement (`player_action` fold win, `submit_showdown`, `claim_timeout`) | One instance write (contract-wide record) and one persistent write plus TTL bump (per-table totals) |
| `finalize_sunset` | One instance write (seats removed) |
| `create_table` | None |

Instance storage is loaded once per invocation whatever it holds, so the metrics
record adds a fixed few dozen bytes and a single ledger-entry write to the calls
above, and never grows with table or hand count.

The CPU and memory ceilings for the affected entrypoints remain enforced by
`budget_ceilings_test.rs` and `gas_regression_test.rs`; they would flag any
regression these writes cause. If a ceiling has to move, follow the procedure in
`docs/soroban-budget-profiling.md`.

## Storage

| Key | Domain | Notes |
|-----|--------|-------|
| `DataKey::ContractMetrics` | instance | Contract-wide counters |
| `DataKey::TableMetrics(table_id)` | persistent | Per-table hands and rake, `ttl::TABLE` policy |

Both are appended to `DataKey` (append-only), recorded in
`storage-layout-snapshot.json`, and listed in `security/storage-access-policy.json`.

## Admin dashboard

`/admin` (`app/src/app/admin/page.tsx`) shows the contract-wide counters in an
"On-Chain Metrics" panel (`ContractMetricsPanel`). It calls `getContractMetrics`
in `app/src/lib/onchain.ts`, which simulates `get_contract_metrics`, and
`getTableMetrics` is available for per-table cards. `app/src/lib/contract-metrics.ts`
parses the native values (bigint for `u64` and `i128`) and formats them.
