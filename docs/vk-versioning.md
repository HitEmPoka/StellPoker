# Verification-key versioning

Each zk-verifier registry entry stores the compact VK bytes plus:

- `hash`: the Soroban Keccak-256 content hash of the exact VK bytes;
- `version`: a non-zero, monotonically managed release number;
- `activated_at`: the ledger sequence at registration.

Deployment passes `--circuit` to `scripts/convert-vk.py`. The converter checks
the VK header's public-input count against the selected circuit and refuses a
mismatch before upload. Clients can call `get_verification_key` and pin both
version and content hash when selecting a circuit artifact.

A circuit change requires compilation of a new VK and incrementing the
registry version. The contract rejects zero versions and any version that is
not greater than the active entry, even when the caller is the contract admin.

## Rotation and grace window (Issue #890)

`set_verification_key` and `update_vk` no longer discard the outgoing key. They
archive it under `VkByVersion(circuit_id, old_version)` with status `Stale` and
`retired_at` set to the new version's activation ledger. The new key is stored
in `VkById`, `VkByVersion(..., new_version)` and `ActiveVkVersion`; new hands
always use `verify_proof_by_id`, which resolves the active version.

Hands that were dealt before the rotation pin the version at deal time and
finish with `verify_proof_by_id_version(circuit_id, ..., version)`. That call
accepts an archived version until `retired_at + grace_period`. Once the window
elapses — or an admin calls `retire_vk_version` and its window elapses — the
call fails with `VkVersionRetired` before doing any cryptographic work.
Explicitly deprecated circuit IDs still fail with `StaleCircuitId`.

`configure_vk_grace_period` sets the window in ledgers (non-zero; default 100
on `initialize`). Every successful verification records
`ProofVkVersion(proof_hash) -> version` and emits `proof_verified_with_vk`, so
a settlement can be audited to the exact circuit version that proved it.
`get_proof_vk_version`, `get_vk_version_record` and `get_active_vk_version` are
the corresponding views.

See [the admin rotation runbook](vk-rotation-runbook.md) for the operational
sequence and coordination with the hand-rotation window.
