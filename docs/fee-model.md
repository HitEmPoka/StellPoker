# Fee Model (Committee Economics)

How per-table MPC costs feed the on-chain rake.

## Sources of cost data

- **Per-session attribution (nodes).** Each MPC node tracks, per proof
  session (`GET /session/:id/cost`):
  - `cpu_seconds` — wall-clock seconds since the session record was created
    (proxy for co-noir witness/proof CPU).
  - `bytes_in` — total decoded share-fragment bytes received.
  - `bytes_out` — proof bytes produced (0 until complete).
  - `fragments`, `circuit_name`, `status`.
- **Structured logs.** Nodes emit `tracing::info!` fields
  (`session_id`, `circuit`, `cpu_seconds`, `bytes_in`, `bytes_out`) on
  completion; the coordinator emits matching fields
  (`session_id`, `circuit`, `proof_bytes`) when it collects a proof.
  Aggregate by `session_id` (3 node records + 1 coordinator record per
  MPC proof; a full hand = deal + reveal(s) + showdown sessions).

## Aggregation

Per table `T` over a window:

```text
cost(T) = Σ_sessions(cpu_seconds(s)) × node_cpu_price
        + Σ_sessions(bytes_in(s) + bytes_out(s)) × byte_price
```

`node_cpu_price` / `byte_price` are operator-configured off-chain
constants (see `infrastructure/README.md` infra costs). The sums come
straight from the structured log/metrics fields above — no estimates.

## On-chain leg

- Rake is taken per pot on-chain: `apply_rake` in
  `contracts/poker-table/src/pot.rs` (`rake_bps`, capped at
  `MAX_RAKE_BPS = 500`), accumulated in `TableState::rake_balance`,
  withdrawable via `withdraw_rake` (emits `rake_withdrawn`).
- **Pricing rule:** set `rake_bps` per table so expected
  `rake_balance` growth covers `cost(T)` with margin. The stats
  indexer already proxies pot size via `rake_withdrawn` amounts
  (`services/coordinator/src/stats.rs`); join that with the
  per-session cost aggregates to check coverage per table.
