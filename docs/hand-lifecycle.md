# Hand Lifecycle — Deal to Showdown (End to End)

Step-by-step trace of one hand, from wallet connect to pot settlement.
For the static view see [ARCHITECTURE.md](../ARCHITECTURE.md);
for proof internals see [zk-proof-pipeline.md](zk-proof-pipeline.md).

Phases (`contracts/poker-table/src/types.rs`, `GamePhase`):
`Waiting → Dealing → Preflop → DealingFlop → Flop → DealingTurn → Turn →
DealingRiver → River → Showdown → Settlement` (plus `Dispute`, `AwaitingRunItTwice`).

## 0. Overview

```mermaid
flowchart LR
    W[1 wallet connect] --> B[2 buy-in / join]
    B --> D[3 MPC deal]
    D --> P[4 preflop betting]
    P --> F[5 flop reveal + betting]
    F --> T[6 turn reveal + betting]
    T --> R[7 river reveal + betting]
    R --> S[8 showdown + settlement]
    S --> H[9 hand history]
```

## 1. Wallet connect

```mermaid
sequenceDiagram
    participant U as User
    participant F as Frontend<br/>(lib/wallet.ts, freighter.ts, lobstr.ts)
    participant W as Freighter / Lobstr

    U->>F: open app
    F->>F: detectInstalledWallets()
    F->>W: requestAccess() / getAddress()
    W-->>F: address (triggers wallet popup)
    F->>F: WalletSession{address, walletType}
    F->>W: signMessage(challenge) — auth to coordinator
```

Frontend: `detectInstalledWallets`, `connectWallet`, `trySilentReconnect`
(`app/src/lib/wallet.ts`); Freighter calls in `lib/freighter.ts`
(`connectFreighterWallet`, `getActiveAddress`); Lobstr in `lib/lobstr.ts`.
Coordinator wallet challenge: `getWalletChallenge` / `verifyWalletChallenge`
(`lib/api.ts`).

## 2. Buy-in / join

```mermaid
sequenceDiagram
    participant U as User
    participant F as Frontend<br/>(lib/onchain.ts, lib/api.ts)
    participant S as Soroban<br/>(poker-table)
    participant C as Coordinator

    U->>F: joinTableOnChain(wallet, tableId, buyIn)
    F->>U: Freighter signTransaction (wallet popup)
    F->>S: join_table(table_id, player, buy_in)
    S-->>F: seat | queue
    F->>C: POST /api/table/:id/join
    C-->>F: lobby state
```

- Onchain escrow: `join_table` / `buy_in_with_currency` (`contracts/poker-table`).
- Offchain lobby mirror: `POST /api/table/:id/join`, `GET /api/table/:id/lobby`
  (`services/coordinator/src/api/mod.rs`, `join_table`, `get_table_lobby`).
- Table creation (admin): `create_table` onchain + `POST /api/tables/create`
  (`createTable` in `lib/api.ts`). Partial top-ups between hands:
  see `docs/poker-table-rebuys.md`.

## 3. MPC deal

```mermaid
sequenceDiagram
    participant F as Frontend<br/>(use-poker-actions.ts)
    participant C as Coordinator
    participant N as MPC nodes x3<br/>(REP3 coNoir)
    participant S as Soroban<br/>(poker-table + zk-verifier)

    F->>C: POST /api/table/:id/request-deal {players}
    C->>S: start_hand() — Waiting → Dealing
    C->>N: POST /table/:id/prepare-deal
    N->>N: per-node permutation+salt shares (no plaintext deck)
    N->>N: dispatch-shares → merge → generate-witness → prove (deal_valid)
    N-->>C: deal proof (deck_root, hand_commitments, dealt_indices)
    C->>S: commit_deal(...) — Dealing → Preflop
    S->>S: zk-verifier.verify_deal()
    C->>F: GET /api/table/:id/player/:addr/cards (own hole cards only)
```

Circuit: `circuits/deal_valid` (per-player-count variants `deal_valid_2p..6p`).
Public: `deck_root`, `hand_commitments[6]`, dealt indices; private (shared):
`deck[52]`, `salts[52]` as 3 party shares + `vrf_sk`.
Frontend hook: `usePokerActions` (`lib/use-poker-actions.ts`); private cards via
`getPlayerCards` (`lib/api.ts`).

## 4. Betting rounds

```mermaid
sequenceDiagram
    participant U as User
    participant F as Frontend
    participant S as Soroban<br/>(poker-table)
    participant C as Coordinator

    U->>F: fold / check / call / bet / raise / all-in
    F->>U: Freighter signTransaction (wallet popup)
    F->>S: player_action(table_id, player, seq, Action)
    S->>S: betting.rs:process_action (turn order, seq per account)
    S-->>F: new state
    F->>C: POST /api/table/:id/player-action (mirror for WS push)
    C-->>F: WS /api/table/:id/state/ws broadcast
```

All-in/fold early end: `settle_fold_win` without showdown.
Multi-table: submissions are sequenced per account (`docs/multi-table-play.md`).
Optional action-hiding: `commit_action` + `reveal_action`
(`docs/commit-reveal-scheme.md`).

## 5. Board reveals (flop / turn / river)

```mermaid
sequenceDiagram
    participant F as Frontend
    participant C as Coordinator
    participant N as MPC nodes x3
    participant S as Soroban

    F->>C: POST /api/table/:id/request-reveal/flop
    C->>N: POST /table/:id/prepare-reveal/flop {deck_root, previously_used_indices}
    N->>N: prove reveal_board_valid (3 cards, no index reuse)
    N-->>C: reveal proof (cards, indices)
    C->>S: reveal_board(...) — DealingFlop → Flop
    S->>S: zk-verifier.verify_reveal()
    Note over F,S: repeat with turn (1 card), river (1 card)
    S-->>F: board state → betting resumes
```

Circuit: `circuits/reveal_board_valid` (max 3 cards per call; turn/river reveal 1).
`requestReveal` in `lib/api.ts`; coordinator `request_reveal` handler.

## 6. Showdown and settlement

```mermaid
sequenceDiagram
    participant F as Frontend
    participant C as Coordinator
    participant N as MPC nodes x3
    participant S as Soroban

    F->>C: POST /api/table/:id/request-showdown
    C->>N: POST /table/:id/prepare-showdown {board_indices, hand_commitments}
    N->>N: prove showdown_valid (hole match + rank table + winner)
    N-->>C: showdown proof (holes, winner_index, tie_mask)
    C->>S: submit_showdown(...) — Showdown → Settlement
    S->>S: verify_showdown → settle_showdown → distribute_pots
    S-->>F: payouts + GET hand-history/chunk
```

Circuit: `circuits/showdown_valid` (+ `2p..6p`, Omaha variants).
Settlement: `game.rs:settle_showdown`, `pot.rs:distribute_pots(_with_ties)`,
`submit_showdown` records `bad_beat_scores` where applicable.
Run-it-twice: `rit_opt_in → ShowdownRun1/2 → RitSettlement`.
Timeouts/disputes: `committee-registry:track_game_phase/report_timeout`,
`poker-table:cancel_hand` (`GamePhase::Dispute`).

## 7. After the hand

- Next hand: `start_new_hand` reuses escrowed stacks; players top up via rebuy band.
- History: last 16 settled hands per table in a circular buffer —
  `get_hand_history` / `get_hand_history_chunk` / `get_hand_history_meta`
  (`docs/poker-table-hand-history.md`); coordinator `GET .../hand-history/chunk`;
  local mirror in `lib/hand-history.ts`, export via `lib/hand-history-export.ts`.
