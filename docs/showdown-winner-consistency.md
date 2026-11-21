# Showdown Winner Consistency Suite

The showdown circuit decides who wins a hand and publishes two values the
poker-table contract settles from: `winner_index` (the first seat with the maximal
score) and `tie_mask` (one bit per seat sharing that score). The contract's own hand
evaluator is `stellar_zk_cards::evaluate_hand`. If the two disagree about the winner,
the pot is paid to the wrong seat, so this suite checks the declaration itself, not
only the hand category (which `docs/hand-evaluator-fuzzing.md` covers).

## What it does

`stellar-zk-cards/src/showdown_winner_consistency.rs` deals random hands to two to
six players, scores each seat with both evaluators, applies the circuit's declaration
rule to each set of scores, and compares `(winner_index, tie_mask)`.

| Result | Meaning | Outcome |
|--------|---------|---------|
| Exact | Same winner and mask | Pass |
| Kicker blind | The circuit ties seats the contract separates by kicker | Counted and reported |
| Hard | The circuit drops or reorders a seat the contract ranks best, or ties different categories | Fails |

A difference in a kicker-free category (high card, straight, flush, full house,
straight flush, royal flush) also fails.

## Known divergence

The circuit's `score_five` (`circuits/lib/src/cards.nr`) encodes no kickers for one
pair, two pair, three of a kind or four of a kind. Two seats making a pair of aces
tie in the circuit even when one has a king kicker and the other a queen. The
contract's evaluator separates them. `circuit_ignores_the_kicker_on_a_pair_the_contract_ranks`
pins a concrete deal.

The suite only counts these cases rather than failing on them. Fixing the circuit
is a larger change: its rank table (`rank_table.nr`) is generated from the same
scoring, and ranking every kicker distinctly needs about 7,500 scores, more than the
table's 4,096 leaves. `strict_agreement_including_kicker_ties` is the same run with every
divergence a failure. It is `#[ignore]`d; enable it when the circuit encodes kickers.

## Running it

```bash
# Default: 10,000 deals, fixed seed
cargo test -p stellar-zk-cards showdown_winner_consistency -- --nocapture

# More deals, or another seed
SHOWDOWN_CONSISTENCY_ITERATIONS=250000 SHOWDOWN_CONSISTENCY_SEED=42 \
  cargo test -p stellar-zk-cards --release showdown_winner_consistency -- --nocapture

# See how many deals hit the known divergence
cargo test -p stellar-zk-cards showdown_winner_consistency -- --ignored --nocapture
```

The result line is `[showdown-consistency] iterations=... seed=... exact=... kicker_blind=...`.
A failure prints the seed, the iteration, the cards, and both sets of scores, so it
reproduces with `SHOWDOWN_CONSISTENCY_SEED` set to that seed.

## Nightly job

`.github/workflows/showdown-winner-consistency.yml` runs at 04:00 UTC with 250,000
deals and a fresh seed each night, well above the 10,000 required, and with 10,000
deals on pull requests that touch the evaluators or the showdown circuits. The seed,
result line and log are attached to the run.
