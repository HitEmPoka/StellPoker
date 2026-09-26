# Changelog

All notable changes to this project are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

### Added
- **#53** Collapsible multi-table mini-map (`TableMiniMap`) with seat counts, chip stacks, and click-to-navigate; coordinator `GET /api/tables/overview`
- **#55** Player seat HUD stats tooltip (VPIP, PFR, aggression factor, hands played) via `GET /api/stats/player/:address`
- **#60** Lightweight i18n (English + Spanish) with browser auto-detect and manual override in settings/header
- **#70** On-chain `player-rating` Soroban contract (ELO, min-hands leaderboard gate, recorder auth) and `/stats` rating leaderboard UI
- **#558** Exhaustive button-rotation tests for 2–6 seat tables (sit-outs, busted seats, players leaving between hands, 2-max)
- **#559** Queryable rake configuration history for `poker-table`: `get_rake_history`, `get_rake_history_len`, `get_rake_bps_at` and a `rake_config_changed` event (`docs/rake-configuration-history.md`)
- **#560** `get_player_positions` batch read (bounded to 20 tables, `BatchTooLarge` over the limit); the dashboard lists a wallet's seats with one call
- **#561** Property tests for pot, rake and bet math at `i128` boundaries

### Fixed
- **#561** `apply_rake` / `split_jackpot_rake` overflowed `i128` for pots above `i128::MAX / rake_bps`, and a `Raise` or pot-limit check with an oversized amount overflowed instead of returning `NotEnoughChips`

---

## [0.1.0] — 2026-05-16

### Added
- `poker-table` Soroban contract: full Texas Hold'em state machine, betting rounds, pot/side-pot calculation, timeout auto-fold, and onchain settlement
- `zk-verifier` Soroban contract: UltraHonk proof verification using Soroban's native BN254 host functions (Protocol 25)
- `committee-registry` Soroban contract: MPC committee registration and slashing logic
- `game-hub` Soroban contract: mock Stellar Game Studio interface
- Noir ZK circuits: `deal_valid`, `reveal_board_valid`, `showdown_valid` — all proved inside TACEO coNoir MPC
- Shared Noir library (`circuits/lib`): card encoding, Poseidon2 commitments, Merkle tree, shuffle verification
- `stellar-zk-cards` reusable Rust crate: card encoding and hand evaluation for Soroban apps
- Coordinator service (Axum): orchestrates MPC sessions, submits proofs and actions to Soroban
- MPC node service: TACEO coNoir participant implementing REP3 secret sharing
- Next.js frontend: lobby, pixel-art table, Freighter wallet integration, solo mode vs AI
- Docker Compose stack for full local development
- Deploy and setup scripts for testnet
