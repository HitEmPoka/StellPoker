# Circuit Soundness Assumptions & Verification Checklist

**Reference**: Issue #312 (Peer Review Appendix)  
**Cross-References**: [ADR-002: UltraHonk Proving System](adr/ADR-002-ultrahonk-proving-system.md), [Circuit Soundness Documentation](circuit-soundness.md), [CRS Verification Guide](crs-verification.md), [UltraHonk Verifier ABI](ULTRASONK_VERIFIER_ABI.md)

---

## 1. Overview & Scope

StellPoker enforces poker game invariants (deck correctness, non-overlapping card distribution, hidden information privacy, legitimate showdown ranking, side pot calculation, and burn card correctness) using zero-knowledge proofs compiled from Noir to the Barretenberg **UltraHonk** proving system over the **BN254** elliptic curve scalar field ($\mathbb{F}_r$).

This document provides a formal cryptographic assumptions appendix produced in response to the #312 peer review. It details:
1. The explicit mathematical and cryptographic assumptions required for UltraHonk proof validity and zero-knowledge privacy.
2. The transcript and Fiat-Shamir transformation model.
3. The Common Reference String (CRS) trust and integrity architecture.
4. The algebraic hash security model (Poseidon2).
5. A comprehensive **Review Checklist** that all future circuit additions and modifications must satisfy prior to landing in production.

---

## 2. Explicit Cryptographic Assumptions

### 2.1 UltraHonk Knowledge-Soundness & Zero-Knowledge

* **Polynomial IOP Soundness**: UltraHonk operates as a multi-round polynomial interactive oracle proof (PIOP) compiled via the Fiat-Shamir heuristic into a non-interactive argument of knowledge (SNARK). We assume the underlying PIOP achieves information-theoretic knowledge soundness with error bounded by:
  $$\epsilon_{\text{sound}} \le \frac{d \cdot \text{poly}(\mu)}{|\mathbb{F}_r|}$$
  where $d \le 2^{20}$ is the circuit constraint degree bound, $\mu$ is the number of sumcheck and permutation rounds, and $|\mathbb{F}_r| \approx 2^{254}$. Thus, the security level is $\ge 128$ bits against computationally bounded adversaries.
* **KZG / Polynomial Commitment Soundness**: We assume the hardness of the Discrete Logarithm problem and the $q$-Strong Diffie-Hellman ($q$-SDH) assumption over the BN254 pairing groups $(\mathbb{G}_1, \mathbb{G}_2, \mathbb{G}_T, e)$. Given commitment $[f(\tau)]_1$, an adversary cannot compute a valid opening proof $[w(\tau)]_1$ for $f(z) \neq v$ without knowing $\tau$ or breaking $q$-SDH.
* **Special Honest-Verifier Zero-Knowledge (SHVZK)**: Prover messages are randomized with masking scalars such that the proof transcript leaks zero information regarding private witness elements (party permutations, salts, unrevealed cards) beyond what is logically implied by the public inputs.

### 2.2 Common Reference String (CRS) / SRS Trust Model

UltraHonk requires an updatable, universal Structured Reference String (SRS) consisting of powers of tau:
$$\{ [\tau^i]_1 \}_{i=0}^{d}, \quad [\tau]_2$$

* **Ceremony Integrity (Aztec Ignition)**: StellPoker consumes the universal reference string generated during the Aztec Ignition multi-party trusted setup ceremony. Soundness assumes that at least one of the 176 ceremony participants acted honestly and irrevocably destroyed their toxic waste contribution $\tau_i$.
* **No Subversion / Pinned Integrity**: A malicious or corrupted CRS can allow an adversary to forge arbitrary proofs. StellPoker enforces strict, pinned SHA-256 verification of `bn254_g1.dat` (`c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470`) across all CI runners, MPC nodes, and local development scripts via `scripts/download-crs.sh`. Any alteration immediately aborts execution.
* **Degree Bound Enforcement**: No compiled circuit degree $N$ may exceed the SRS capacity ($N \le 2^{20} = 1,048,576$ gates). The largest production circuit (`showdown_valid`) occupies $\approx 237,000$ backend gates, leaving substantial margin.

### 2.3 Fiat-Shamir Transformation & Random Oracle Model (ROM)

To convert the public-coin interactive protocol into a non-interactive proof, challenges are derived algorithmically from transcript hashes:

* **Random Oracle Assumption**: The transcript hash function is modeled as a cryptographically secure random oracle.
* **Strong Fiat-Shamir Implementation**: To prevent "weak Fiat-Shamir" attacks (where an adversary alters unbound public parameters without changing the verifier challenges), UltraHonk binds all of the following into the transcript before challenge generation:
  1. Circuit verification key selector commitments ($\{ [q_i]_1 \}$).
  2. Public input wires ($P_0, P_1, \dots$).
  3. Preceding wire commitments ($[w_1]_1, [w_2]_1, [w_3]_1, [w_4]_1$).
  4. Permutation accumulator commitments ($[z]_1$).
  5. Sumcheck round univariate coefficients.
* **No Transcript Malleability**: The verifier reconstructs the identical transcript order. Public inputs passed to the Soroban on-chain verifier are strictly typed and endianness-checked against `docs/ULTRASONK_VERIFIER_ABI.md`.

### 2.4 Hash Function Model: Poseidon2 over BN254

All commitments in circuits (`commit_card`, `commit_hand`, `commit_board`, Merkle trees) use Poseidon2 configured for BN254 with state width $t = 4$, rate $r = 3$, capacity $c = 1$, and degree-5 S-boxes ($x^5$):

* **Collision Resistance**: Finding two distinct inputs $(x, y) \neq (x', y')$ such that $\text{Poseidon2}(x, y) = \text{Poseidon2}(x', y')$ requires $\ge 2^{128}$ operations. This ensures a player cannot commit to one hole card and reveal a different card under the same root.
* **Pre-image Resistance**: Given a leaf commitment or root $R$, determining any valid pre-image $(c, s)$ without knowledge of the salt scalar $s \in \mathbb{F}_r$ is computationally infeasible ($\ge 2^{128}$ security).
* **Algebraic Attack Immunity**: The round structure (4 full rounds, 56 partial rounds, 4 full rounds) provides security against Gröbner basis, interpolation, and higher-order differential cryptanalysis over $\mathbb{F}_r$.

### 2.5 Multi-Party Computation (coNoir) Privacy Model

For collaborative proving in MPC:
* **Threshold Secrecy**: Private inputs (permutation shares and salt scalars) are secret-shared among 3 committee nodes ($N=3$, $t=1$). Collusion of up to $t$ malicious nodes yields zero information regarding the permutation or dealt cards of unrevealed players.
* **Reconstruction Validation**: As implemented in `services/coordinator/src/mpc_validation.rs`, all reconstructed hole cards and board cards are verified against their on-chain Poseidon2 commitments before being relayed to clients. Byzantine or faulty node contributions are rejected.

### 2.6 Soroban Host Verification Environment

* **Protocol 25/26 Host Function Soundness**: On-chain verification delegates BN254 multi-scalar multiplication (MSM) and pairing checks to native Soroban host functions. We assume correct cryptographic implementation in stellar-core.
* **Deterministic Fuel / Budget Limits**: Proof verification must consistently execute within Soroban's transaction CPU and memory instruction limits.

---

## 3. Review Checklist for Future Circuit Changes

Any pull request introducing new circuits or modifying existing Noir circuits must pass this checklist prior to review approval and merge.

### Phase 1: Cryptographic Constraints & Soundness Completeness
- [ ] **No Under-Constrained Signals**: Every private witness variable is uniquely constrained. Verify there are no free variables or unconstrained quotient limbs.
- [ ] **Strict Range & Bit-Length Checks**: All integer casts (`u8`, `u32`, `u64`) are explicitly range-checked to prevent field element wrap-around modulo $r$.
- [ ] **Uniqueness Invariants**: Circuits proving card mechanics (deck validity, hand deals, board reveals, burn cards) enforce strict pairwise distinctness or bitmask occupancy checks (e.g. `used_indices[idx] == false`).
- [ ] **Bijection Verification**: Permutation arguments verify that the mapping is a valid bijection over $\{0 \dots 51\}$ (or $N$-deck shoe equivalent).

### Phase 2: Public Inputs & ABI Stability
- [ ] **Public Input Minimization**: Only signals that the smart contract verifier needs to inspect or forward to state storage are marked `pub`.
- [ ] **Domain Separation**: All packed digests and Merkle leaves incorporate explicit domain tags (e.g. `DOMAIN_HAND`, `DOMAIN_INPUTS`).
- [ ] **Endianness & Layout Alignment**: Public input order and byte alignment match `docs/ULTRASONK_VERIFIER_ABI.md` and the Soroban verifier contract data structures.
- [ ] **Verification Key Update**: If constraints change, regenerate verification keys via `bb write_vk --scheme ultra_honk` and verify checksums in `scripts/convert-vk.py`.

### Phase 3: Test Coverage & Negative Soundness Tests
- [ ] **Positive Test Vectors**: Unit tests pass with valid witnesses across minimum, average, and maximum player counts (2p, 3p, 6p).
- [ ] **Negative / Malicious Prover Tests**: Every security assertion has at least one companion negative test (`#[test(should_fail)]` or error witness assertion) verifying that forged, swapped, out-of-range, or duplicate inputs are rejected.
- [ ] **Differential Testing with Rust**: Logic shared with Rust contracts (e.g. hand ranking, kicker tiebreakers) has matching tests in `stellar-zk-cards` and `circuits/test-vectors`.

### Phase 4: CI Gates & Resource Budgets
- [ ] **Constraint Regression Gate**: Run `scripts/circuit_constraints.py`. ACIR opcodes and UltraHonk backend gate counts must stay within configured limits in `circuits/constraint-budgets.json`.
- [ ] **Verification Instruction Budget**: Soroban contract verification simulation confirms gas/CPU instructions remain within allowable network limits.
- [ ] **Deterministic Proof Generation**: Prover execution is benchmarked and completes within the SLA budget (< 60s for showdown, < 100ms for deal/burn).
