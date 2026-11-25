# TLA+ Model — Deal/Reveal Protocol

This directory contains a TLA+ model of the StellPoker deal and reveal protocol.

## Files

| File | Purpose |
|---|---|
| `DealReveal.tla` | TLA+ specification of the protocol |
| `DealReveal.cfg` | TLC model configuration (6 players, 3 nodes) |
| `run-tlc.sh` | Script to run TLC model checker |

## Actors modelled

- **Players** — up to 6 seats; each receives two hole cards via a commitment.
- **Committee** — 3 MPC nodes that jointly produce deal, board-reveal, and showdown proofs.
- **Coordinator** — routes phases; modelled implicitly through phase transitions.
- **Contract** — on-chain state machine; modelled as the phase variable and proof-verified flags.

## Invariants

| Invariant | What it checks | Contract mapping |
|---|---|---|
| `UniqueCardOwnership` | No deck index appears in more than one player's hand | Pairwise index checks in `game.rs` and `deal_valid` circuit |
| `MonotonicPhase` | The game phase never decreases | `NotInDealingPhase` / `NotInRevealPhase` / `NotInShowdownPhase` error guards |
| `SettlementRequiresProof` | Settlement is only reachable after a verified deal proof, at least one verified reveal proof, and either a showdown proof or a fold win | `DealProofVerificationFailed` (27), `RevealProofVerificationFailed` (28), `ShowdownProofVerificationFailed` (29) |

## Running TLC

Install TLC from [https://github.com/tlaplus/tlaplus/releases](https://github.com/tlaplus/tlaplus/releases) and place `tla2tools.jar` at `~/tla/tla2tools.jar`, then:

```bash
./tla/run-tlc.sh
```

Override the jar location with:

```bash
TLC_JAR=/path/to/tla2tools.jar ./tla/run-tlc.sh
```

TLC uses all available CPU cores (`-workers auto`) and checks all three invariants for the full state space with 6 players and 3 MPC nodes.

## Counterexamples found during modelling

None. The three invariants hold across the full state space. The key design decision that makes `SettlementRequiresProof` hold without counterexamples is that every phase-advancing action that leads toward `PhaseSettlement` requires the corresponding `*Verified` flag to be `TRUE` as an enabling condition. Without this guard the invariant would be violated on the path `Init → StartHand → FoldWin` where `revealProofVerified` is still `FALSE` — this is intentionally permitted by `FoldWin` since a fold win does not require board reveals, which the invariant accommodates via the `foldWin` flag.
