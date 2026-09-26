# Rake configuration history

Rake is taken from every settled pot, so a change to a table's `rake_bps` changes what players pay. Each `poker-table` table keeps an append-only, on-chain log of its rake so the rake in force at any moment can be audited.

## What is recorded

Entry `0` is written by `create_table` with the rake the table starts with. Every `set_rake_bps` call that actually changes the value appends one more entry. Setting the current value again records nothing, and a rejected call (above `MAX_RAKE_BPS`, 500 bps = 5%) records nothing.

| Field | Meaning |
|-------|---------|
| `index` | Position in the log, starting at 0 |
| `rake_bps` | Rake in basis points from this entry on |
| `previous_bps` | Rake in force just before this entry (same as `rake_bps` for entry 0) |
| `effective_at_timestamp` | Ledger timestamp in seconds at which it took effect |
| `effective_at_ledger` | Ledger sequence at which it took effect |
| `changed_by` | The table admin who made the change |

A change takes effect immediately. A hand that settles after the change is raked at the new rate, including one already in progress.

Tables created before this log existed have no entry `0`. The first `set_rake_bps` on such a table writes one for the rake it was charging until then, with `effective_at_timestamp = 0` (meaning "since creation, time unknown"), followed by the new change.

## Reading it

| Function | Returns |
|----------|---------|
| `get_rake_history(table_id, start, limit)` | Entries oldest first, `limit` capped at 50. Page by advancing `start`; past the end is an empty list |
| `get_rake_history_len(table_id)` | Number of entries |
| `get_rake_bps_at(table_id, timestamp)` | The rake in force at `timestamp`, or `None` if it is before the table's history. A change is in force from its own `effective_at_timestamp` |

All three fail with `TableNotFound` for an unknown table. `get_rake_bps_at` does a binary search, so its cost grows with the logarithm of the log length.

## Events

`set_rake_bps` emits `rake_config_changed` for indexers, in addition to the existing `rake_bps_updated`, which is unchanged.

| Event | Topics | Data |
|-------|--------|------|
| `rake_config_changed` | name, table id | (index, previous bps, new bps, effective timestamp) |

It is also recorded in the table's config version log (`get_config_history`, summary `rake_bps`).

## Storage

One persistent entry per change, `DataKey::RakeHistory(table_id, index)`, plus a `DataKey::RakeHistoryLen(table_id)` counter. Both use the table TTL policy (`ttl::TABLE`). Appending never rewrites earlier entries, so the write cost of a change does not grow with the log.
