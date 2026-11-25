# VK rotation runbook (Issue #890)

## Goal

Rotate a circuit's verification key without invalidating hands that are
already in flight. Each settlement must be attributable to the VK version
that proved it, and retired keys must stop being usable after a bounded grace
period.

## Roles and timing

- **Admin**: multi-sig operator of the zk-verifier contract (`configure_vk_grace_period`,
  `update_vk`, `retire_vk_version`).
- **Coordinator**: deals hands and pins the active `vk_version` for the hand.
  It must ensure a hand's `settle` call happens before that version's grace
  deadline (see `services/coordinator/src/key_rotation.rs` for the existing
  rotation-overlap pattern).
- **Observers**: poll `get_active_vk_version`, `get_vk_version_record` and
  `get_proof_vk_version` for audit and alerting.

## Pre-rotation checklist

1. Convert and validate the new VK: `python3 scripts/convert-vk.py --circuit <circuit> ...`.
   Confirm the public-input count matches the deployed circuit.
2. Query the current state:
   - `get_active_vk_version(circuit_id)`
   - `get_vk_grace_period()`
   - `get_vk_version_record(circuit_id, current_version)` — note `activated_at`.
3. Estimate the longest expected hand duration in ledgers and set
   `configure_vk_grace_period(admin, grace_ledgers)` so the window exceeds it.
   Do this **before** activating the new key. A zero value is rejected.

## Rotation steps

1. `update_vk(admin, circuit_id, new_vk_bytes, new_version)` (or
   `set_verification_key` for the three legacy circuit types). The contract:
   - rejects non-monotonic versions;
   - archives the outgoing key as `Stale` with `retired_at = now`;
   - activates the new version for new hands.
2. Announce the version change to coordinators. New deals use
   `verify_proof_by_id`, which always resolves `ActiveVkVersion`.
3. In-flight hands continue with
   `verify_proof_by_id_version(circuit_id, proof, public_inputs, pinned_version)`.
   Coordinator settlement should read `get_proof_vk_version(proof_hash)` after
   success and include that version in its hand record/audit event.
4. Monitor `proof_verified_with_vk` events to confirm both versions settle
   during the overlap and that no old-version settlements arrive after the
   deadline.

## Retirement

- Automatic: a stale version stops working when
  `now >= retired_at + grace_period`; calls fail with `VkVersionRetired`.
- Explicit: `retire_vk_version(admin, circuit_id, version)` restarts the grace
  clock from the current ledger. Use this only after confirming all hands
  pinned to that version have settled. `schedule_vk_retirement` is an alias.
- `deprecate_vk(circuit_id)` remains an emergency circuit-wide stop and rejects
  all versions immediately with `StaleCircuitId`, including in-flight hands.

## Recovery

If a hand cannot settle before its version expires, do **not** lower the
version or re-activate an archived key (versions are monotonic). Escalate to
the admin: configure a longer grace period for future rotations, then use the
table's existing timeout/void path for the stuck hand. The archived key bytes
remain readable via `get_vk_version_record` for off-chain investigation.
