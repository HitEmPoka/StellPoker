# Contract Access Control Matrix (Issue #551)

This document specifies which parties can call which functions in the Stellar Poker contracts.

## Poker Table Contract (`poker-table`)

### Public Functions (Any Caller)

| Function | Purpose | Auth Required |
|----------|---------|----------------|
| `get_table(table_id)` | Read table state | None |
| `get_player_tables(player)` | List tables a player sits at | None |
| `get_player_table_count(player)` | Count of active tables per player | None |
| `get_player_buy_in(table_id, player)` | Query player's current buy-in | None |
| `get_queue(table_id)` | View waiting-list queue | None |
| `get_player_count(table_id)` | Get number of seated players | None |
| `get_players_paginated(table_id, offset, limit)` | Paginated player list | None |
| `get_hand_history(table_id, limit)` | Get archived hands (newest first) | None |
| `get_hand_history_chunk(table_id, offset, limit)` | Paginated hand history | None |
| `get_hand(table_id, hand_number)` | Fetch single hand record | None |
| `get_hand_history_meta(table_id)` | Buffer bookkeeping (capacity/count) | None |
| `hand_history_capacity()` | History buffer size constant | None |
| `get_straddle_config(table_id)` | Straddle configuration | None |
| `get_mississippi_pending(table_id)` | Pending Mississippi straddle | None |
| `get_active_straddle(table_id)` | Active straddle state | None |
| `is_paused(table_id)` | Check pause flag | None |
| `get_admin(table_id)` | Admin address | None |
| `get_config_version(table_id)` | Current configuration version | None |
| `get_config_history(table_id)` | Configuration change audit log | None |
| `get_config_change(table_id, version)` | Fetch specific config version | None |
| `has_sufficient_budget()` | Budget guard status | None |
| `get_contract_metrics()` | Tables created, hands played, total rake, active seats (O(1) counters) | None |
| `get_table_metrics(table_id)` | Hands played, total rake and seated players for one table | None |
| `is_table_sunset(table_id)` | Whether the table has been frozen by `finalize_sunset` | None |
| `get_sunset_record(table_id)` | What `finalize_sunset` swept and refunded | None |

### Player Functions (Requires Auth)

| Function | Purpose | Caller | Notes |
|----------|---------|--------|-------|
| `join_table(table_id, buy_in)` | Seat at table | Player | Must have sufficient token balance |
| `leave_table(table_id, player)` | Unseat and refund | Player calling for self | Refunds stack + committed chips |
| `rebuy(table_id, amount)` | Top up stack mid-session | Player | Within `max_rebuys` limit |
| `leave_queue(table_id, player)` | Leave waiting list | Player | Refunds buy-in escrow |
| `player_action(table_id, player, action, amount)` | Bet/check/fold/etc | Player | Only when it's their turn |
| `claim_timeout(table_id, claimer)` | Claim opponent timeout | Player | Called by any active player |
| `rit_opt_in(table_id, player)` | Opt into Run-It-Twice | Player | After hand is complete |
| `post_mississippi_straddle(table_id, player, amount)` | Post Mississippi straddle | Player | Config must allow it |
| `cancel_mississippi_straddle(table_id, player)` | Cancel pending straddle | Player | Before cards dealt |

### Coordinator/Committee Functions

| Function | Purpose | Caller | Notes |
|----------|---------|--------|-------|
| `start_hand(table_id)` | Begin new hand | Coordinator | Checks min players, blinds |
| `commit_deal(table_id, ...)` | Register shuffled deck commitment | MPC Committee | Via coordinator |
| `reveal_board(table_id, ...)` | Reveal community cards | MPC Committee | Must match commitment |
| `submit_showdown(table_id, ...)` | Showdown (winner determination) | MPC Committee | With ZK proof |
| `force_fold(table_id, seat)` | Forfeit inactive player | Coordinator | After timeout |
| `commit_action(table_id, player, action_hash, nonce_hash)` | Commit-reveal scheme | Player | For dispute resolution |
| `reveal_action(table_id, player, seq, action, amount, nonce)` | Reveal committed action | Player | Verifies against commitment |

### Admin Functions (Table Owner or Game Hub)

| Function | Purpose | Caller | Notes |
|----------|---------|--------|-------|
| `create_table(config)` | Provision new table | Game Hub | Stores config, initializes state |
| `set_max_rebuys(table_id, max_rebuys)` | Adjust rebuy limit | Table Admin | Can be changed mid-session |
| `configure_straddle(table_id, multiplier, position)` | Enable straddle rules | Table Admin | Before next hand |
| `configure_straddle_extended(table_id, config)` | Advanced straddle setup | Table Admin | Supports live/capped/reraise flags |
| `pause(table_id)` | Block all actions | Table Admin | Emergency only |
| `unpause(table_id)` | Resume table | Table Admin | Re-enables play |
| `propose_table_closure(table_id, caller)` | Initiate closure with notice | Table Admin | 1-day notice period |
| `execute_table_closure(table_id)` | Finalize closure & refund | Anyone (after notice) | Auto-refunds all players |
| `finalize_sunset(table_id)` | Sweep rake, jackpot pool and queue escrow, then freeze the table | Table Admin | Only once no seat holds chips; see `docs/contract-sunset-runbook.md` |
| `approve_emergency_withdrawal(table_id, caller)` | Approve player withdrawal | Table Admin | Up to N approvals for emergency |
| `admin_emergency_withdrawal(table_id)` | Execute emergency withdrawal | Table Admin | Requires N approvals first |

### Governance Functions

| Function | Purpose | Caller | Notes |
|----------|---------|--------|-------|
| `configure_upgrade_governance(table_id, signers, threshold, delay_ledgers)` | Setup N-of-M upgrade | Admin/Hub | Issue #504 |
| `propose_upgrade(table_id, new_wasm_hash)` | Propose code upgrade | One of N signers | Starts timelock |
| `approve_upgrade(table_id, new_wasm_hash)` | Vote to approve upgrade | One of N signers | Accumulates toward threshold |
| `execute_upgrade(table_id)` | Deploy after timelock | Anyone | Verifies N approvals & delay elapsed |
| `revert_last_upgrade(table_id, previous_wasm_hash)` | Rollback on canary failure | One of N signers | Within rollback window (6h) |

## Zero-Knowledge Verifier Contract (`zk-verifier`)

### Public Functions (Any Caller)

| Function | Purpose | Auth Required |
|----------|---------|----------------|
| `verify_proof(proof_bytes, public_inputs)` | Validate UltraHonk proof | None |

### Admin Functions (Committee)

| Function | Purpose | Caller |
|----------|---------|--------|
| `update_vk(circuit_id, verification_key)` | Update verification key | Committee |
| `configure_governance(signers, threshold)` | Setup N-of-M upgrade | Committee |

## Committee Registry Contract (`committee-registry`)

### Public Functions (Any Caller)

| Function | Purpose | Auth Required |
|----------|---------|----------------|
| `get_committee(id)` | Fetch committee details | None |
| `list_active_committees()` | All active committees | None |
| `get_committee_fee_share(committee_id, node_id)` | Stake-weighted fee share | None |

### Node Functions (MPC Nodes)

| Function | Purpose | Caller | Notes |
|----------|---------|--------|-------|
| `enroll_node(committee_id, node_pubkey, stake)` | Register as committee member | Node operator | Requires stake |
| `heartbeat(committee_id, node_id)` | Keep node active | Node operator | Extends availability |
| `claim_fees(committee_id, node_id)` | Collect rake share | Node operator | Proportional to stake |

### Admin Functions

| Function | Purpose | Caller | Notes |
|----------|---------|--------|-------|
| `create_committee(nodes, params)` | Provision MPC committee | Governance | Configures RSA/ZK params |
| `slash_node(committee_id, node_id, reason, amount)` | Penalize misbehavior | Governance | For evidence of cheating |

## Authorization Patterns

### Pattern: Self-Authorization
Player functions require the player's signature (e.g., `join_table` requires the joining player to authorize the call). This prevents one wallet from controlling another's stack.

### Pattern: Table Admin or Game Hub
Some functions (`pause`, `unpause`, `set_max_rebuys`) require either:
- The table's admin address, OR
- The table's configured Game Hub address

This allows both standalone operators and Stellar Game Studio integration.

### Pattern: Governance Threshold
Upgrade and security-critical changes use N-of-M multisig (Issue #504):
1. One signer proposes + timelock starts
2. Other signers approve (must reach threshold)
3. After delay_ledgers, any party may execute
4. Fast rollback window allows signers to cancel within 6h

### Pattern: Coordinator Orchestration
Deal/reveal/showdown operations are driven by the coordinator but signed by the MPC committee. The contract verifies ZK proofs and on-chain commitments to ensure committee honesty.

## Cross-Contract Calls

| From | To | Function | Reason | Budget Guard |
|------|-----|----------|--------|--------------|
| Poker Table | Token (USDC) | `transfer` | Transfer chips/rake | Yes (#552) |
| Poker Table | ZK Verifier | `verify_proof` | Validate deal/showdown proofs | Yes (#552) |
| Poker Table | Committee Registry | `claim_fees` | Distribute rake to MPC nodes | Yes (#552) |
| Poker Table | Game Hub | `start_game`/`end_game` | Signal hand lifecycle to Game Studio | Yes (#552) |
| ZK Verifier | (none) | N/A | Pure verification (no calls out) | N/A |
| Committee Registry | (none) | N/A | State management only | N/A |

**Note on Budget Guards**: Cross-contract calls check remaining budget via `has_sufficient_budget()` to prevent DoS chains that exhaust Soroban's per-invocation limits.

## Configuration Changes and Audit Trail

Configuration changes to a table (e.g., `set_max_rebuys`, `configure_straddle`) are tracked in the configuration change log (Issue #553):

- Each change increments the table's `config_version`
- A `ConfigChangeEvent` records: version, ledger, timestamp, and change summary
- Indexed via `get_config_history()` and `get_config_change(version)`
- Allows players to audit mid-lobby rule changes

## Summary

- **Public (No Auth)**: All read-only functions (state queries, history)
- **Player Auth**: Actions that move a player's chips or change their status
- **Admin Auth**: Table configuration and lifecycle (create, pause, close)
- **Governance Auth**: Upgrade proposals and high-security changes (N-of-M)
- **Coordinator/Committee**: Deal and settlement orchestration (signed by MPC)

