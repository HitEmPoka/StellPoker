# Table Ownership Transfer

A table NFT (`contracts/table-nft`) carries the right to run a table. Changing
its owner while a hand is being played would change who controls the table under
the players' feet, so a transfer is only safe when the table is idle. This
document describes the two safe paths.

## The hand gate

The gate is opt-in. The `table-nft` admin registers a **hand reporter**, normally
the poker-table contract that runs the hands:

```
set_hand_reporter(admin, reporter)
```

The reporter then brackets every hand for the token it belongs to:

| Call | Effect |
|------|--------|
| `report_hand_started(reporter, token_id)` | The table is busy. Fails with `HandAlreadyInProgress` if a hand is already reported. |
| `report_hand_completed(reporter, token_id)` | The table is idle again, and any queued transfer runs. Fails with `NoHandInProgress` if no hand is reported. |
| `is_hand_active(token_id)` | Read the flag. |

A deployment that never sets a reporter never has a hand in flight, so it keeps
the original transfer behavior.

## Path 1: transfer while idle

`transfer(caller, from, to, token_id)` works exactly as before when no hand is in
flight and the table is not leased. While a hand is in flight it fails with
`HandInProgress`, for the owner and for approved operators alike.

## Path 2: transfer gated on hand completion

While a hand is in flight, the owner (or an approved address) can queue the
transfer instead:

```
queue_transfer(caller, from, to, token_id)
```

The transfer is stored and runs automatically inside `report_hand_completed`. It
clears approvals and rental listings and emits `table_transferred`, the same as a
direct transfer.

| Rule | Error |
|------|-------|
| Only while a hand is in flight; use `transfer` when idle | `NoHandInProgress` |
| One queued transfer per table | `TransferAlreadyPending` |
| Not for a leased table (a lease outlives the hand) | `CannotTransferRentedTable` |
| New leases are refused while a transfer is queued | `TransferPending` |

`cancel_queued_transfer(caller, token_id)` drops it. The requester, the owner or an
approved address can cancel. `get_pending_transfer(token_id)` reads it.

### Stuck hands

If a hand is never reported complete (for example the reporter was retired
mid-hand) the table would stay locked. The admin can release it:

```
force_clear_hand(admin, token_id)
```

This clears the flag and, like a normal completion, runs any queued transfer. It
is an admin action and emits `hand_force_cleared`, so it is visible on chain.

## Events

| Event | Data | When |
|-------|------|------|
| `hand_reporter_set` | `(admin, reporter)` | Reporter configured |
| `hand_started` | `(reporter, token_id)` | Hand reported started |
| `hand_completed` | `(reporter, token_id)` | Hand reported complete |
| `transfer_queued` | `(from, to, token_id, caller)` | Transfer queued behind a hand |
| `transfer_cancelled` | `(from, to, token_id, caller)` | Queued transfer cancelled |
| `queued_transfer_executed` | `(from, to, token_id)` | Queued transfer ran at hand completion |
| `table_transferred` | `(from, to, token_id)` | Any ownership change (unchanged) |
| `hand_force_cleared` | `(admin, token_id)` | Admin released a stuck hand |

## Tests

`contracts/table-nft/src/transfer_gate_test.rs` covers both paths: idle transfer,
rejection mid-hand (owner and approved operator), queued transfer executing at
completion, cancellation, lease interaction, reporter authority, hand bookkeeping
and the admin release.
