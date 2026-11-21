# Hand History Archive (off-chain tier)

The Soroban `poker-table` contract only keeps a rolling buffer of the
**last 16 settled hands per table** (a circular buffer — see
[poker-table-hand-history.md](poker-table-hand-history.md)). Anything older
is evicted on-chain. This document defines the **archive tier**: an export
pipeline that copies every hand off Soroban storage into queryable
long-term storage, plus the archive's read API and retention policy (#684).

```
Soroban (hot, 16/table) ──get_hand_history_chunk──▶ exporter ──▶ archive DB/CDN ──▶ query API
        circular buffer                              (script,          (Postgres +         (read path
                                                     scheduled)        object storage)     below)
```

## What gets archived

Every settled hand record exactly as the contract returns it (hand number,
board, pot, action summary, settlement tx, seat/player metadata). The
exporter is **append-only and idempotent**: a hand is keyed by
`(table_id, hand_number)` and re-exporting a range never duplicates or
overwrites rows — safe to re-run after crashes or outages.

## Export pipeline

`scripts/export-hand-history-archive.py` paginates the contract's chunked
read API and writes the archive:

```bash
./scripts/export-hand-history-archive.py \
  --contract-id "$POKER_TABLE" \
  --table-id 0 \
  --chunk 16 \
  --out archive/hands-table-0.jsonl
```

- Reads `get_hand_history_meta(table_id)` first (`total_archived` tells you
  how many hands exist), then walks `get_hand_history_chunk(table_id,
  offset, limit)` from the newest hand backwards to `offset = total_archived`.
- Stops on hands already present in the output file (resume support).
- Emits JSONL, one hand per line, sorted by `hand_number` ascending.

Scheduling (either is fine):

- **CI cron**: a nightly workflow step that runs the exporter and commits or
  uploads the artifact (example step below).
- **Host cron**: `0 4 * * * cd /srv/stellpoker && ./scripts/export-hand-history-archive.py ...`

Example CI step:

```yaml
- name: Export hand history archive
  run: |
    ./scripts/export-hand-history-archive.py \
      --contract-id "${{ vars.POKER_TABLE_ID }}" \
      --table-id 0 --chunk 16 --out /tmp/hands.jsonl
  env:
    STELLAR_NETWORK_PASSPHRASE: "Test SDF Network ; September 2015"
    RPC_URL: https://soroban-testnet.stellar.org
```

### Archive storage

| Tier | Store | Format | Purpose |
|---|---|---|---|
| Hot | Postgres (coordinator DB or dedicated) | table `hand_history_archive(table_id, hand_number, recorded_at, hand jsonb)` with PK `(table_id, hand_number)` | Serves the query API, dispute lookups |
| Cold | Object storage / CDN (`hands/<table_id>/<hand_number>.json`) | one JSON object per hand | Cheap long-term copies, static CDN serving |

Load path: JSONL → `COPY hand_history_archive ...` (hot), and same JSONL
split into per-hand objects (cold).

## Queryable archive API

Reads go to the archive, never back to the chain. Contract of the read path
(coordinator HTTP API — spec lives in `docs/openapi.yaml`):

| Endpoint | Returns | Notes |
|---|---|---|
| `GET /api/archive/hands?table_id=<id>&before=<hand_number>&limit=<n>` | Up to `n` hands, **newest first**, strictly below `before` | Cursor pagination; `before` omitted = newest |
| `GET /api/archive/hands/<table_id>/<hand_number>` | One hand, or `404` if not yet archived | Dispute/single-hand lookup |
| `GET /api/archive/hands/<table_id>/meta` | `{total_archived, newest, oldest, exported_at}` | Mirrors `get_hand_history_meta` plus export freshness |

Guarantees:

- The archive is a **superset** of the on-chain buffer: anything readable
  via `get_hand_history` is also in the archive once the pipeline has run.
- `total_archived` in the on-chain meta and the archive count converge to
  the same value (`total_archived` is cumulative and never shrinks).
- On-chain TTL bumps (`get_hand_history_chunk` bump-on-read) are irrelevant
  to archive reads — the archive is off-chain storage.

## Retention policy

Hand history contains gameplay records tied to wallet addresses; retention
follows the same principles as
[data-retention-policy.md](data-retention-policy.md).

| Data | Location | Retention | Deletion method |
|---|---|---|---|
| Rolling buffer (16 hands/table) | Soroban contract storage | Only while the table exists; evicted round-robin | Contract eviction on settle; table sunset → see [contract-sunset-runbook.md](contract-sunset-runbook.md) |
| Archive — hot tier (Postgres) | Coordinator/infrastructure | **90 days** | Row delete + vacuum (`shred`-equivalent for exports) |
| Archive — cold tier (objects/CDN) | Object storage | **13 months** | Object deletion + CDN purge |
| Hands under active dispute / audit hold | Both tiers | Held until the case closes, then reverts to the schedule above | Manual flag, then normal schedule |
| Legal/regulatory copy (if required) | Cold storage, write-once | Up to **7 years** | Kept separate from queryable tiers; no player-identifying index |

Operational notes:

- The exporter never deletes; purge is a separate scheduled job that applies
  the table above (deletes hot rows > 90 days, cold objects > 13 months).
- Deleting from the archive does not touch on-chain data (the chain only
  ever holds the last 16 hands anyway).
- Wallet-address deletion requests follow `data-retention-policy.md`:
  addresses are pseudonymous identifiers; hands are retained for the audit
  window with address associations anonymised at the hot tier after 90 days.
