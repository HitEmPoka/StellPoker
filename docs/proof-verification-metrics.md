# Proof Verification Failure Metrics

The `zk-verifier` contract maintains a labeled failure counter per reason
code so verification failures are observable beyond opaque error returns.

## Countable-failure semantics

A Soroban invocation that returns `Err` is fully rolled back — storage
writes and contract events from that frame are erased. Countable
verification failures therefore record the counter **and return
`Ok(false)`** so the write and its event persist. `Err` is reserved for
operational rejections (e.g. paused). Callers already treat `Ok(false)`
as "verification failed" (`if !verify_deal(...)`).

Note: if a caller absorbs a `Ok(false)` and then fails the *top-level*
transaction (e.g. poker-table aborting the hand), Soroban rolls back the
whole transaction including the verifier's counter write. On-chain
counters therefore capture standalone/absorbed verification calls
(coordinator pre-checks, integration probes); in-game aborts are covered
by the off-chain budget/failed-tx alerts below.

## Reason codes

| Reason | On-chain sources | Auto-counted |
|---|---|---|
| `BadVk` | `NoVkForCircuit`, `VkParseError` | Yes |
| `BadProof` | `ProofSizeError`, `VerificationFailed` | Yes |
| `MalformedInputs` | `PublicInputSizeError`, `PublicInputMismatch`, `WrongCommitmentCount`, `WrongBoardIndicesCount` | Yes |
| `Budget` | Host out-of-budget (OOG) trap | No — see below |

`Budget` rejections trap at the host level and abort the invocation before any
storage write, so the aborting call cannot increment its own on-chain counter.
Budget anomalies are therefore alerted from failed-transaction rates (RPC
diagnostic events / submission failures) rather than the on-chain counter.

## Reading counters

- `get_failure_count(reason)` — per-reason counter (view call)
- Event `proof_verification_failed` — `(reason, new_count)` on every failure

## Alert rules

| Alert | Condition | Severity |
|---|---|---|
| `zk_verify_bad_vk_spike` | `BadVk` counter increases within 5m | Warning |
| `zk_verify_bad_proof_spike` | `BadProof` counter increases within 5m | Warning |
| `zk_verify_malformed_spike` | `MalformedInputs` counter increases within 5m | Warning |
| `zk_verify_budget_spike` | Rate of failed `verify_*` submissions with budget-exhausted diagnostics > 3 in 5m | Critical |

A sustained increase in any on-chain counter (or budget failure rate) means
either a client integration bug (malformed inputs), a VK deployment problem
(bad VK), or an adversarial/proof-quality issue (bad proof). Page on-call for
any counter that increases monotonically for 15m.
