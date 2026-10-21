# Privacy-preserving time-bank proof

## Statement

The circuit proves that a committed time-bank state can transition after an
action while its remaining balance stays non-negative. Exact balance, usage,
elapsed-ledger history, replenishment rate, and commitment nonces remain in
the private witness.

| Value | Visibility | Purpose |
| --- | --- | --- |
| `player_hash` | Public | Binds the proof to one player identity |
| `action_id` | Public | Prevents replay for another turn/action |
| `previous_commitment` | Public | Binds the proof to the accepted prior state |
| `next_commitment` | Public | Commits the new private remaining balance |
| Elapsed ledgers, replenishment rate | Public | Anchors replenishment to observable time and table policy |
| Initial balance, time used | Private | Hides the player's timing strategy |
| Previous/next nonce | Private | Blinds equal balances across transitions |

The state commitment is Poseidon2 over `(player_hash, balance, nonce)`. The
next nonce is additionally bound to the public action ID. A verifier accepts
the next commitment as state only after proof verification and rejects reuse
of the prior action ID at the application layer.

All accounting uses constrained `u32` arithmetic. The circuit asserts
`time_used <= initial_balance + replenished` before subtraction, so a witness
with negative remaining time cannot satisfy the constraints.
