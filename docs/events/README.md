# poker-table event schemas

One JSON Schema per event emitted by the poker-table contract, for indexers and the frontend. Each file describes the event as `{ "topics": [...], "data": ... }` in the canonical JSON form below.

| Event | Topics | Data |
|-------|--------|------|
| `table_created` | name, table id | admin |
| `player_joined` | name, table id | (player, seat) |
| `player_left` | name, table id | (player, amount withdrawn) |
| `hand_started` | name, table id | hand number |
| `deal_committed` | name, table id | (hand number, commitments) |
| `player_action` | name, table id, action type | (player, chips added) |
| `board_revealed` | name, table id | (cards, deck indices) |
| `phase_change` | name, table id | new phase |
| `hand_settled` | name, table id | (winner, total pot, payouts) |
| `fold_win` | name, table id | (winner, winnings) |
| `rake_collected` | name, table id | 5 fields after a showdown, 3 after a fold win or run it twice |

## Canonical JSON

| Soroban type | `x-soroban` tag | JSON |
|--------------|-----------------|------|
| `Symbol` | `symbol` | string |
| `u32` | `u32` | integer |
| `i128` | `i128` | decimal string (safe beyond 2^53) |
| `Address` | `address` | strkey string (`G...` or `C...`) |
| `BytesN<32>` | `bytes32` | 64 lowercase hex characters |
| `Vec<T>` | `vec` | array |
| tuple | `tuple` | fixed length array |
| unit enum variant | `enum` | variant name |

The `x-soroban` tags let a decoder check the raw XDR value types, and the standard JSON Schema keywords validate the decoded JSON.

## Decoders

- Rust: `contracts/poker-table/src/event_schema.rs` (`decode_event`). It depends only on the XDR types and `serde_json`.
- TypeScript: `app/src/lib/event-schemas.ts` (`decodePokerTableEvent`), with a typed union of every event.

## Checks

`contracts/poker-table/src/event_schema_test.rs` runs in the contract test suite. It plays a showdown hand and a fold win hand, decodes every emitted event against these files, and fails if an event does not match its schema or a schema is never emitted. It also checks that the JSON types and `x-soroban` tags agree in every file.

To add an event, add its schema here, list it in `SCHEMAS` in `event_schema.rs` and in `app/src/lib/event-schemas.ts`, and make sure the lifecycle test emits it.
