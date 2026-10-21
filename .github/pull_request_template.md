## Summary
<!-- Provide a clear, concise summary of the changes introduced in this PR. -->

## Linked Issues
<!-- Reference the issue(s) this PR closes or relates to, e.g. Closes #123 -->
Closes #

## Scope of Changes
<!-- Describe what components were modified and the motivation behind the change. -->
- [ ] `circuits/`
- [ ] `services/node/`
- [ ] `services/coordinator/`
- [ ] `contracts/`
- [ ] `docs/` / `scripts/` / other

---

## Security & Side-Channel Resistance Review Checklist
<!--
MANDATORY: If this PR touches `services/node/`, `circuits/`, or `contracts/zk-verifier/`
(or any cryptographic, MPC, or private state handling paths), all of the following
side-channel resistance checks MUST be evaluated and checked off by author and reviewers.
-->

### 1. Secret-Dependent Branches
- [ ] No branching (`if`, `match`, loop conditions, early returns) depends on secret data (e.g. private cards, deck permutation indices, salt shares, or private keys).
- [ ] All private operations in circuits and cryptographic primitives execute along constant-time execution paths or fixed-depth constraint trees.
- [ ] Conditional selections on secret values use branchless/multiplexer primitives (e.g. arithmetic selection `b * (x - y) + y` or constant-time conditional moves).

### 2. Log Leakage
- [ ] No plaintext cards, deck permutations, salt shares, unblinded commitments, or secret shares are emitted to stdout, stderr, or `tracing::*` events.
- [ ] Diagnostic/debug logs only emit nonces, public hashes, public commitments, party IDs, session IDs, or aggregate metrics.
- [ ] Formatted error contexts (e.g. in `Result::Err`, `anyhow`, or panic messages) do not interpolate secret values.

### 3. Error-Message & Timing Oracles
- [ ] Error messages returned across API endpoints or RPC calls do not leak whether a failure was due to secret values, individual secret shares, or specific card indices.
- [ ] Return status codes and error responses are uniform and do not provide an oracle allowing adversaries to differentiate valid vs invalid secret guesses.
- [ ] Timing variations across rejection paths (e.g. invalid signature, bad proof, out-of-order phase message) do not reveal secret information.

### 4. Memory & Allocation Patterns
- [ ] Sensitive memory buffers, private key material, and ephemeral secret keys implement `zeroize::Zeroize` / `ZeroizeOnDrop` so secret bytes are securely cleared on drop.
- [ ] Dynamic allocations (vector capacities, buffer resizes) do not correlate in size or count with private data values.
- [ ] Pre-allocated buffers or fixed-size arrays are utilized where feasible to prevent heap allocation timing side-channels.

---

## Verification & Testing
- [ ] Unit tests added / updated and passing
- [ ] Integration / end-to-end tests passing
- [ ] Property / fuzz tests added where applicable
- [ ] Static analysis, lints, and format checks passing (`cargo clippy`, `cargo fmt`)
