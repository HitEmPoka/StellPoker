# Security Audit Report

**Project:** StellPoker  
**Audit Date:** YYYY-MM-DD  
**Auditor(s):**  
**Version/Commit:**  
**Scope:**  

---

## 1. Executive Summary

| Category | Rating (1–5) | Notes |
|---|---|---|
| Overall Risk | | |
| Threat Model Completeness | | |
| Attack Surface Exposure | | |
| Code Quality | | |
| MPC Protocol Soundness | | |
| Circuit Soundness | | |
| Contract Invariants | | |

---

## 2. Threat Model

### 2.1 Assets

| Asset | Description | Criticality |
|---|---|---|
| Player funds (XLM) | On-chain balances committed to table pots | Critical |
| Private cards | Card values known only to each player before reveal | Critical |
| ZK proofs | Proofs of correct deal/reveal/settlement | High |
| MPC coordinator state | Lobby, session tokens, game phase | High |
| Committee registration | MPC node identities and stake | High |

### 2.2 Trust Assumptions

- The Soroban RPC endpoint is honest but may be unavailable.
- At least 2 of 3 MPC nodes are honest (dishonest majority can forge proofs).
- The player's wallet (Freighter/Lobstr) properly protects the secret key.
- The Stellar network consensus is correct.

### 2.3 Threat Actors

| Actor | Capability | Motivations |
|---|---|---|
| Malicious player | Can run modified frontend, craft arbitrary API calls | Cheat, steal funds, DoS |
 | Malicious MPC node | Controls its own machine, network position | Forge proofs, censor |
| External attacker | Network-level MITM, RPC spam | Intercept, replay, DoS |
| Compromised coordinator | Full read/write on coordinator state | Steal session tokens, manipulate lobby |

### 2.4 Attack Trees

```
Cheat at poker
├── See opponent's hole cards
│   ├── Compromise MPC node → leak private cards
│   └── Replay deal proof → reconstruct randomness
├── Change own hole cards after deal
│   ├── Forge a deal proof (requires dishonest MPC majority)
│   └── Modify on-chain contract state (requires contract vulnerability)
├── Fold without losing buy-in
│   └── Revert settlement transaction (requires Soroban reversion bug)
└── Win pot without best hand
    └── Forge showdown proof (requires dishonest MPC majority)
```

---

## 3. Attack Surface Analysis

### 3.1 Entry Points

| Entry Point | Protocol | Authentication | Risk |
|---|---|---|---|
| Coordinator HTTP API | HTTP/REST | Session token (wallet signature) | High |
| Soroban contract methods | Stellar RPC | Wallet signature | Medium |
| MPC node gRPC | gRPC (internal) | mTLS (planned) | Medium |
| Frontend static assets | HTTPS | None | Low |
| WebSocket (if enabled) | WS/WSS | Session token | Medium |

### 3.2 Data Flow

```
[Browser/Frontend]
    │  wallet.sign(payload)
    ▼
[Coordinator] ──gRPC──► [MPC Node 0]
    │                    [MPC Node 1]
    │                    [MPC Node 2]
    │
    │  submitTx(signedXdr)
    ▼
[Soroban RPC] ──► [Stellar Network]
```

### 3.3 Trust Boundary Crossings

| Boundary | Direction | Risk |
|---|---|---|
| Browser → Coordinator | Outbound | Signature replay if nonce missing |
| Coordinator → MPC nodes | Inbound/Outbound | gRPC without mTLS allows MITM |
| Coordinator → Soroban RPC | Outbound | RPC responses could be spoofed |
| MPC node → MPC node | Peer-to-peer | Network partition could enable equivocation |

---

## 4. Code Review Checklist

### 4.1 Coordinator (`services/coordinator/`)

- [ ] Input validation on all API endpoints (length, type, range checks)
- [ ] Session token generated with cryptographically secure RNG
- [ ] Session token tied to a specific wallet address (no token reuse across users)
- [ ] Rate limiting enforced before any expensive operation
- [ ] Proof size validated before forwarding to MPC nodes
- [ ] MPC node responses validated (signature, schema, timeout)
- [ ] No sensitive data (private keys, signing secrets) in logs
- [ ] Database queries use parameterised statements (no SQL injection)
- [ ] TLS termination handled by reverse proxy, not the coordinator binary

### 4.2 MPC Nodes (`services/mpc-node/`)

- [ ] Peer-to-peer connections use authenticated encryption
- [ ] Incoming coordinator requests are authenticated
- [ ] Private randomness never logged or persisted
- [ ] Proof generation runs in a resource-limited sandbox
- [ ] Allocated memory bounded to prevent OOM
- [ ] Node identity registered on-chain with slashing condition
- [ ] Protocol version checked before participation

### 4.3 Frontend (`app/src/`)

- [ ] Wallet signatures include a nonce/domain separator to prevent replay
- [ ] Sensible CSP headers set (no `'unsafe-inline'` for scripts)
- [ ] No hardcoded secrets or API keys in client bundle
- [ ] All coordinator API calls go through a centralised module (validates responses)
- [ ] User-controlled input (table ID, aliases) is escaped before rendering
- [ ] LocalStorage does not contain private keys or signing material
- [ ] Error messages do not leak stack traces or internal state

### 4.4 Contracts (`contracts/`)

- [ ] All public functions have access control checks
- [ ] Integer overflow/underflow protected (checked math or safe math lib)
- [ ] Re-entrancy guards on settlement functions
- [ ] `require` / `panic` messages are informative but not verbose
- [ ] Contract upgrade mechanism timelocked (if upgradeable)
- [ ] Constants are truly constant (not stored in writable storage)
- [ ] `payout` function can only be called once per hand

---

## 5. MPC Protocol Review

### 5.1 Setup

- [ ] CRS generated via a secure multi-party ceremony or downloaded from a trusted source
- [ ] CRS file integrity verified (SHA-256 hash matches published value)
- [ ] Committee registration uses on-chain stakes to prevent Sybil attacks

### 5.2 Deal Protocol

- [ ] Each MPC node contributes entropy to the deal randomness
- [ ] Commit-reveal scheme used so no node learns the final randomness early
- [ ] Deal proof verifies that cards are valid (no duplicates, correct suit/rank encoding)
- [ ] Proof binds to `table_id` and `hand_number`

### 5.3 Reveal Protocol

- [ ] Flop/turn/river reveal uses shared randomness from the deal phase
- [ ] Each revealed card is accompanied by a proof that it was committed in the deal
- [ ] Proof verifies the card index is within the deck and not already dealt

### 5.4 Showdown Protocol

- [ ] Hand rank comparison is computed inside the circuit (not revealed off-chain)
- [ ] Winner determination uses public inputs only (board + player hand hashes)
- [ ] Showdown proof verifies the winner's hand rank beats all other players

### 5.5 Slashing

- [ ] Misbehaviour (timeout, invalid proof, equivocation) is detectable on-chain
- [ ] Slashing condition is provable via a ZK proof (not subjective)
- [ ] Slashed node's stake is distributed to affected players

---

## 6. Circuit Soundness

### 6.1 Noir Circuits (`circuits/`)

- [ ] All circuits compile without warnings
- [ ] Circuit constraints are sufficient to guarantee soundness (no under-constrained paths)
- [ ] Public inputs are verified against on-chain commitments
- [ ] Private inputs are properly witnesses (not leaked in public outputs)
- [ ] Fuzzing harness exists for each circuit
- [ ] Arithmetic gates do not overflow the field modulus
- [ ] Range checks enforce valid card indices (0–51), suit (0–3), rank (0–12)
- [ ] Duplicate card check prevents the same card appearing twice

### 6.2 Test Coverage

| Circuit | Unit Tests | Prover Tests | Fuzz Tests |
|---|---|---|---|
| `deal` | ✓ | ✓ | — |
| `reveal` | ✓ | ✓ | — |
| `showdown` | ✓ | — | — |
| `hand_rank` | ✓ | ✓ | — |

- [ ] All prover tests pass with the configured CRS
- [ ] Fuzz tests added for edge cases (empty deck, all cards same suit, etc.)

---

## 7. Contract Invariants

### 7.1 Poker Table Contract (`contracts/poker-table`)

- [ ] `total_buy_in + total_folds + total_raises == pot` at all times
- [ ] No player can act out of turn
- [ ] `hand_number` monotonically increases
- [ ] `buy_in` is locked until hand settles or player leaves
- [ ] Settlement pays exactly `pot - house_fee` to the winner(s)
- [ ] House fee does not exceed `MAX_HOUSE_FEE` (configurable, default 2.5 %)
- [ ] A player cannot join a table that is already in progress
- [ ] `player_action` only succeeds when `current_turn == caller`
- [ ] `player_action` only succeeds when the player has enough chips for the bet/raise

### 7.2 Committee Registry Contract

- [ ] Registered node count is exactly `NUM_MPC_NODES` (currently 3)
- [ ] Node deregistration requires a majority vote or slashing proof
- [ ] Slashed nodes are removed from the active committee
- [ ] Node stake meets the `MIN_STAKE` requirement

---

## 8. Findings

| ID | Severity | Title | Status |
|---|---|---|---|
| A-01 | | | Open / Fixed / Acknowledged |
| A-02 | | | Open / Fixed / Acknowledged |
| A-03 | | | Open / Fixed / Acknowledged |
| B-01 | | | Open / Fixed / Acknowledged |
| B-02 | | | Open / Fixed / Acknowledged |
| C-01 | | | Open / Fixed / Acknowledged |

### Finding Template

```
**ID:** A-01  
**Title:**  
**Severity:** Critical / High / Medium / Low / Info  
**Location:**  
**Description:**  

**Impact:**  

**Recommendation:**  

**Status:** Open / Fixed / Acknowledged  
**Fixed in commit:**  
```

---

## 9. Appendix

### 9.1 Tools Used

| Tool | Version | Purpose |
|---|---|---|
| | | |
| | | |

### 9.2 References

- StellPoker Threat Model (internal)
- Noir language security considerations
- Stellar Soroban security best practices
- TACEO CO-NOIR MPC protocol documentation

---

*This report is confidential and intended for the StellPoker development team.*
