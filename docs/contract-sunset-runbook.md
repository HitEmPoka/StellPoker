# Runbook: Sunsetting a Table or Contract Version

Soroban contracts cannot be deleted, so there is no self-destruct. "Sunset" here
means: return every chip the contract holds for a table, then freeze the table so
nothing can enter it again. Follow this in order; each step leaves the table in a
state the next one accepts.

> **Before you start.** `execute_table_closure` in contract versions that predate
> this runbook refunded stale `committed` chips after a hand had settled, paying
> the last pot out a second time. Do not run it against such a version on a table
> whose last hand has settled: upgrade first, or let players exit with
> `leave_table`. The fixed version refunds `stack` after settlement and
> `stack + committed` only while a hand is live.

## Where a table's chips sit

| Balance | Held as | Returned by |
|---------|---------|-------------|
| Player stacks (and live bets mid-hand) | `PlayerState.stack` / `committed` | `leave_table`, or `execute_table_closure` |
| Waiting-list buy-ins | `Queue(table_id)`, escrowed in the contract | `leave_queue`, or `finalize_sunset` |
| House rake | `TableState.rake_balance` | `withdraw_rake`, or `finalize_sunset` |
| Jackpot pool | `TableState.jackpot_balance` | `finalize_sunset` (no other admin path) |
| Swept dead chips | Treasury contract | `reclaim_dead_chips` within the reclaim period |

Not covered: tokens sent to the contract outside these paths, and currency tokens
taken by `buy_in_with_currency` before conversion. Reconcile the contract's token
balance in step 6 to find them.

## Steps

### 1. Announce and stop new hands

Tell players the table is closing and when. Stop the coordinator from calling
`start_hand` for the table. Do **not** call `pause` first: a paused table rejects
`leave_table`, which would trap the players you are trying to let out. Closure and
finalize are not blocked by a pause, but exits are.

### 2. Propose the closure

```
propose_table_closure(table_id, admin)
```

Starts a one-day notice period and emits `table_closure_proposed`. Players can keep
using `leave_table` between hands, and queued players can `leave_queue`.

### 3. Let the current hand end, or use emergency withdrawal

Let any hand in progress settle. If the MPC committee cannot finish it, use the
emergency path after its timelock (`admin_emergency_withdrawal`, or
`approve_emergency_withdrawal` by a majority of seated players), which refunds every
stack and committed chip and moves the table to `Settlement`.

### 4. Execute the closure

Once the notice has passed, anyone can call:

```
execute_table_closure(table_id)
```

It refunds every remaining seat, zeroes the stacks and emits `table_closed` with the
total refunded. The table is now in `Settlement` with every stack at zero. Seats are
still assigned, so `rebuy` would still be accepted and, with enough rebuys,
`start_hand` after it. Step 5 shuts that.

### 5. Finalize the sunset

```
finalize_sunset(table_id)     // table admin
```

Requires the table to be between hands with no chips in any seat (otherwise
`SunsetNotReady`). In one call it:

1. refunds every waiting-list buy-in to its owner,
2. pays the house rake and the jackpot pool to the table admin,
3. clears the seats and the wallet-to-table index,
4. cancels any closure notice that is still pending, and
5. writes the `SunsetRecord` and emits `table_sunset`.

From then on `join_table`, `buy_in_with_currency`, `rebuy` and `start_hand` fail with
`TableSunset`, and a second `finalize_sunset` fails the same way. Reads
(`get_table`, hand history, metrics, `get_sunset_record`) keep working.

### 6. Verify

- `get_sunset_record(table_id)` shows what was swept and refunded.
- `get_rake_balance` and `get_jackpot_balance` are zero.
- The contract's token balance has dropped by exactly the sum of the refunds and
  payouts. If other tables remain, it should equal their liabilities (stacks + pots +
  rake + jackpot + queue escrow) and nothing more. If it does not, investigate before
  moving on.

### 7. Retire a contract version

When every table on a contract version is sunset, its liabilities are zero and it can
be left inert. Point the coordinator and app configuration at the new version, stop
extending the old contract's TTLs, and let it archive. Do not deploy a replacement
over live tables: a table that still holds chips has to go through steps 1 to 6 first.

## What a sunset table can still do

| | After `finalize_sunset` |
|---|---|
| Take chips (`join_table`, `rebuy`, `buy_in_with_currency`) | Rejected with `TableSunset` |
| Start a hand | Rejected with `TableSunset` |
| Pay out (`withdraw_rake`) | Returns 0 |
| Read state, history, metrics | Works |
| Repeat `finalize_sunset` | Rejected with `TableSunset` |

## Failure modes

| Symptom | Cause | Action |
|---------|-------|--------|
| `finalize_sunset` returns `SunsetNotReady` | A seat still holds chips, or a hand is live | Run step 3 or 4 first |
| `execute_table_closure` returns `TableClosureNotReady` | Notice period has not elapsed | Wait; it is one day |
| `execute_table_closure` fails on a token transfer | Running a pre-fix version after a settled hand (see the note at the top) | Upgrade, or exit players with `leave_table` |
| A queued player is still owed chips | Sunset not finalized | `leave_queue`, or `finalize_sunset` |

## Tests

`contracts/poker-table/src/sunset_test.rs` follows this runbook with real token
balances: a played showdown hand and a waiting player leave rake, a jackpot pool and
queue escrow; closure returns stacks only; finalize pays out every residual so the
contract ends at zero and every minted chip is accounted for; the freeze rejects
deposits; and finalize refuses while chips are seated or a hand is live.
