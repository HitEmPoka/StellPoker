# StellPoker System Architecture

Onchain Texas Hold'em on Stellar with cryptographically private cards (ZK-MPC / coSNARKs).
No single party — coordinator, node, or player — ever sees the full deck.

> Related docs: [hand lifecycle](docs/hand-lifecycle.md) (end-to-end sequence),
> [ZK proof pipeline](docs/zk-proof-pipeline.md) (Noir → UltraHonk → Soroban),
> [commit-reveal](docs/commit-reveal-scheme.md), [MPC session encryption](docs/mpc-session-encryption.md),
> [committee setup](docs/committee-setup.md), [CRS verification](docs/crs-verification.md).

## 1. System overview

```mermaid
flowchart TB
    subgraph Clients
        PA[Player A<br/>Freighter wallet]
        PB[Player B ... up to 6]
        SPEC[Spectators]
    end
    subgraph Frontend["Web App — app/src (Next.js 15)"]
        UI[Game UI + wallet<br/>lib/wallet.ts, freighter.ts, lobstr.ts]
        API[Coordinator client<br/>lib/api.ts]
        ONCHAIN[Chain client<br/>lib/onchain.ts]
    end
    subgraph Backend["Coordinator — services/coordinator/src (Axum)"]
        REST[REST + WS API<br/>api/mod.rs]
        MPC[MPC orchestrator<br/>mpc.rs]
        SOR[Soroban submitter<br/>soroban/proofs.rs]
    end
    subgraph MPCNet["MPC committee — services/node (TACEO coNoir, REP3, 3-party)"]
        N0[Node 0]
        N1[Node 1]
        N2[Node 2]
    end
    subgraph Chain["Soroban — contracts/"]
        PT[(poker-table<br/>betting, state machine, settlement)]
        ZKV[(zk-verifier<br/>UltraHonk BN254)]
        CR[(committee-registry<br/>members, epochs, slashing, fees)]
        GH[(game-hub<br/>Game Studio hook)]
    end
    PA --> UI
    PB --> UI
    SPEC --> UI
    UI --> API
    UI --> ONCHAIN
    API -->|HTTP/WS| REST
    ONCHAIN -->|signed tx| PT
    REST --> MPC
    MPC -->|mTLS dispatch/collect| N0 & N1 & N2
    MPC -->|commit_deal / reveal_board / submit_showdown| PT
    PT -->|verify_deal/reveal/showdown| ZKV
    PT -->|track_game_phase / report_timeout| CR
    PT -->|notify_start| GH
```

Key properties:

- **Private cards** — the deck exists only as REP3 secret shares across the 3 nodes
  (`services/node/src/private_table.rs`, `session.rs`). Privacy holds if ≥2 nodes are honest.
- **ZK-verified** — every deal, board reveal, and showdown carries an UltraHonk proof
  verified onchain by `zk-verifier` via Protocol 25/26 BN254 host functions.
- **Trustless settlement** — bets, pots (`contracts/poker-table/src/pot.rs`), and payouts
  settle in `poker-table`; the coordinator only relays proofs, it cannot forge them.

## 2. Contract flow

```mermaid
flowchart LR
    PT[poker-table]
    ZKV[zk-verifier]
    CR[committee-registry]
    GH[game-hub]
    USER((player tx))

    USER -->|create_table| PT
    USER -->|join_table / buy_in_with_currency| PT
    USER -->|player_action / commit_action + reveal_action| PT
    PT -->|verify_deal / verify_reveal / verify_showdown| ZKV
    PT -->|register_member / create_epoch / report_slash<br/>deposit_rake / distribute_fees| CR
    PT -->|notify_start / end_game| GH
    PT -->|get_hand_history / chunk / meta| USER
```

`poker-table` state machine (`contracts/poker-table/src/types.rs`, `GamePhase`):
`Waiting → Dealing → Preflop → DealingFlop → Flop → DealingTurn → Turn →
DealingRiver → River → Showdown → Settlement` (+ `Dispute`, `AwaitingRunItTwice`,
`ShowdownRun1/2`, `RitSettlement`). Entry points: `create_table`, `join_table`,
`start_hand`, `commit_deal`, `player_action` (`betting.rs:process_action`),
`reveal_board`, `submit_showdown` (`game.rs:settle_showdown`, `pot.rs:distribute_pots`),
`cancel_hand`, `rit_opt_in`.

## 3. MPC lifecycle

```mermaid
sequenceDiagram
    participant C as Coordinator<br/>(mpc.rs)
    participant N0 as Node 0
    participant N1 as Node 1
    participant N2 as Node 2
    participant S as Soroban<br/>(poker-table)

    Note over N0,N2: Per-hand: prepare → dispatch → merge → prove → collect
    C->>N0: POST /table/:id/prepare-deal {players, circuit_dir}
    C->>N1: POST /table/:id/prepare-deal
    C->>N2: POST /table/:id/prepare-deal
    Note over N0,N2: Each node builds its own permutation+salt shares<br/>(private_table.rs) — no plaintext deck anywhere
    C->>N0: POST /table/:id/dispatch-shares
    N0->>N1: share fragment (nonce-guarded, session_encryption)
    N0->>N2: share fragment
    N1->>N0: share fragment
    N2->>N0: share fragment
    Note over N0,N2: co-noir merge-input-shares → Prover.toml<br/>generate-witness --protocol REP3 → witness.gz<br/>build-and-generate-proof --protocol REP3 (session.rs)
    N0-->>C: GET /session/:id/proof (14624 B UltraHonk)
    C->>C: proof_cache dedup + convert_keccak_proof_to_soroban
    C->>S: commit_deal / reveal_board / submit_showdown
    S->>S: zk-verifier verify_* (sumcheck + shplonk)
```

Reveal/showdown reuse the same pattern with
`prepare-reveal/:phase` and `prepare-showdown` (see `services/node/src/api.rs`).
Coordinator routes (`services/coordinator/src/main.rs`): `request-deal`,
`request-reveal/:phase`, `request-showdown`, `player-action`, `rit-opt-in`.
Committee economics (epochs, slashing, fee split) live in `committee-registry`
(see `docs/committee-fee-distribution.md`, `docs/threshold-committee-signing.md`).

## 4. Card commitment scheme

```mermaid
flowchart LR
    subgraph Private ["Private inputs (REP3-shared, never onchain)"]
        D[deck 52]
        S2[salts 52]
        P0[party0 permutation+salts]
        P1[party1 permutation+salts]
        P2[party2 permutation+salts]
    end
    subgraph Public ["Public outputs (onchain)"]
        R[deck_root<br/>Merkle 64 leaves, depth 6]
        HC[hand_commitments 6]
        IDX[dealt indices 2p,2p+1]
        BC[revealed cards + indices]
        W[winner_index + tie_mask]
    end
    D -->|Poseidon2 commit_card = H card,salt| HC
    D -->|compute_merkle_root_generic| R
    HC -->|showdown: hole match + rank table| W
    R -->|reveal: card match, no index reuse| BC
```

- `commit_card = Poseidon2(card, salt)` (`circuits/lib/src/commitments.nr`);
  `commit_hand = H(c1, c2)`, Omaha `H(H(c1,c2),H(c3,c4))`.
- `deck_root` = Merkle root over 64 leaves (52 cards + padding), depth 6.
- Deal (`circuits/deal_valid`): proves the deck is a valid 52-card permutation,
  the root matches, and each `hand_commitments[i]` matches dealt cards `2p, 2p+1`.
- Reveal (`circuits/reveal_board_valid`): proves revealed cards match the committed
  deck and no index is reused (`previously_used_indices[16]`).
- Showdown (`circuits/showdown_valid`): proves hole cards match commitments, hand
  evaluation against the rank table is correct, and `winner_index`/`tie_mask` are right.
- Player *actions* use a separate hash commit-reveal
  (`Keccak(action||amount||nonce)`, `contracts/poker-table/src/commit_reveal.rs`).

## 5. Proof verification pipeline

```mermaid
flowchart LR
    NC[nargo compile<br/>scripts/compile-circuits.sh] --> CB[circuit bytecode<br/>circuits/*/target/*.json]
    CRS[.crs/bn254_g1.dat<br/>Aztec Ignition + sha256 pin] --> PRV[coSNARK prove<br/>co-noir build-and-generate-proof<br/>--protocol REP3 --hasher keccak]
    CB --> PRV
    WIT[witness.gz<br/>co-noir generate-witness] --> PRV
    PRV -->|proof 14624 B + public inputs| COORD[coordinator<br/>soroban/proofs.rs<br/>convert + dedup]
    COORD -->|invoke commit_deal/reveal_board/submit_showdown| ZKV[zk-verifier contract<br/>set_verification_key + verify_*]
    ZKV -->|sumcheck + shplemini<br/>BN254 host fns P25/P26| OK{valid?}
    OK -->|yes| PT[poker-table advances phase]
    OK -->|no| REVERT[tx fails, hand disputable]
```

Stages: Noir compilation (`nargo 1.0.0-beta.17`) → CRS (`scripts/download-crs.sh`,
pinned in `scripts/crs.sha256`) → witness (`generate-witness --protocol REP3`) →
UltraHonk proving (Barretenberg `bb`, keccak hasher) → serialization
(14624 B proof + field elements as hex/Bytes) → Soroban verification
(`zk-verifier`, vendored verifier in `vendor/ultrahonk-rust-verifier`) →
coordinator submission with retry + `proof_cache` dedup. Full detail in
[docs/zk-proof-pipeline.md](docs/zk-proof-pipeline.md).
