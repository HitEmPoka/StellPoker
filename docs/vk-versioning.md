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
