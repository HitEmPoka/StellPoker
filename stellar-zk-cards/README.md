# stellar-zk-cards

[![Crates.io](https://img.shields.io/crates/v/stellar-zk-cards)](https://crates.io/crates/stellar-zk-cards)
[![Docs.rs](https://docs.rs/stellar-zk-cards/badge.svg)](https://docs.rs/stellar-zk-cards)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../LICENSE)

Reusable ZK card-game primitives for [Stellar Soroban](https://soroban.stellar.org) smart contracts.

Part of [Stellar Poker](https://github.com/HitEmPoka/StellPoker) — onchain Texas Hold'em with ZK-MPC private cards.

## Features

- Compact 6-bit card encoding (`0`–`51`) compatible with BN254 field elements used in Noir ZK circuits
- Best-of-seven Texas Hold'em hand evaluator (no heap allocation — safe for `#![no_std]` Soroban contracts)
- [`HandCategory`] and [`HandRank`] types that are directly comparable on-chain

## Installation

```toml
[dependencies]
stellar-zk-cards = "0.1"
soroban-sdk = "22"
```

## Usage

```rust,ignore
use stellar_zk_cards::{Card, evaluate_hand, HandCategory};

// Cards are encoded as suit * 13 + rank
// Suits: 0=Clubs, 1=Diamonds, 2=Hearts, 3=Spades
// Ranks: 0=2 … 8=10, 9=J, 10=Q, 11=K, 12=A

let hole1 = Card::new(3, 12); // Ace of Spades  (value = 51)
let hole2 = Card::new(2, 11); // King of Hearts (value = 37)

let board = [
    Card::new(0, 12), // Ace of Clubs
    Card::new(1, 12), // Ace of Diamonds
    Card::new(0, 11), // King of Clubs
    Card::new(1, 11), // King of Diamonds
    Card::new(2, 10), // Queen of Hearts
];

let seven = [
    hole1.value, hole2.value,
    board[0].value, board[1].value, board[2].value,
    board[3].value, board[4].value,
];

let rank = evaluate_hand(&seven);
assert_eq!(rank.category(), HandCategory::FullHouse as u32);

// Compare two hands
let other = evaluate_hand(&[0, 1, 2, 3, 13, 14, 15]); // some other hand
if rank.beats(&other) {
    // rank wins
}
```

## Card encoding reference

| Suit     | Value |   | Rank | Value |
|----------|-------|---|------|-------|
| Clubs    | 0     |   | 2    | 0     |
| Diamonds | 1     |   | 3–9  | 1–7   |
| Hearts   | 2     |   | 10   | 8     |
| Spades   | 3     |   | J    | 9     |
|          |       |   | Q    | 10    |
|          |       |   | K    | 11    |
|          |       |   | A    | 12    |

## Hand categories

`HandRank::category()` returns a `u32` matching the `HandCategory` enum:

| Value | Category       |
|-------|----------------|
| 0     | High Card      |
| 1     | One Pair       |
| 2     | Two Pair       |
| 3     | Three of a Kind|
| 4     | Straight       |
| 5     | Flush          |
| 6     | Full House     |
| 7     | Four of a Kind |
| 8     | Straight Flush |
| 9     | Royal Flush    |

## License

[MIT](../LICENSE)
