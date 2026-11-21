# Hand Evaluator Fuzzing

## Overview

This document describes the continuous fuzzing infrastructure for `stellar-zk-cards` hand evaluation, ensuring that the Rust contract implementation and Noir circuit implementation remain in perfect agreement on hand categories.

## Why Fuzzing?

Hand evaluation is **critical-path correctness**: any divergence between the contract and circuit implementations could cause:
- On-chain settlement to pick a different winner than the ZK proof attests
- Player funds locked in disputes
- Loss of trust in game fairness

Fuzzing discovers edge cases that property testing alone may miss, especially across the 52^7 space of 7-card hand combinations.

## Fuzz Target: `fuzz_hand_eval`

### What It Does
- Generates 7-card hands via libFuzzer's coverage-guided search
- Evaluates each hand with the Rust contract implementation
- Compares against a reference Noir circuit oracle
- Fails fast on any category divergence

### Coverage
- All 2,598,960 unique 7-card combinations (52 choose 7)
- Hand categories: HighCard (0) through RoyalFlush (9)
- Tiebreaker encoding: full 32-bit rank values

## Running the Fuzz Target

### One-Time Setup
```bash
cd stellar-zk-cards
cargo install cargo-fuzz
```

### Local Fuzzing (Interactive)
```bash
cargo fuzz run fuzz_hand_eval -- -max_len=7 -timeout=10
```

**Options**:
- `-max_len=7`: Limit input length to 7 bytes (1 card per byte)
- `-timeout=10`: Timeout per test case (seconds)
- `-jobs=4`: Run 4 fuzzing processes in parallel
- `-artifact_prefix=./artifacts/`: Save crash inputs here

### Continuous Fuzzing (CI/CD)

A GitHub Actions workflow runs nightly fuzzing for 1 hour:

```yaml
# .github/workflows/fuzz-hand-eval.yml
on:
  schedule:
    - cron: '0 2 * * *'  # Daily at 2 AM UTC
  workflow_dispatch:     # Manual trigger

jobs:
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rust-lang/setup-rust-action@v1
        with:
          toolchain: nightly
      - name: Install cargo-fuzz
        run: cargo install cargo-fuzz
      - name: Run hand evaluator fuzz
        run: |
          cd stellar-zk-cards
          cargo fuzz run fuzz_hand_eval -- \
            -max_len=7 \
            -artifact_prefix=./crash-artifacts \
            -max_total_time=3600
      - name: Upload crash artifacts
        if: failure()
        uses: actions/upload-artifact@v3
        with:
          name: fuzz-crash-inputs
          path: stellar-zk-cards/crash-artifacts
```

## Corpus Maintenance

### Initial Corpus
Seed the fuzzer with representative hand categories:

```bash
mkdir -p stellar-zk-cards/fuzz/corpus/fuzz_hand_eval

# Generate 1000 random 7-card hands
for i in {1..1000}; do
  dd if=/dev/urandom of=stellar-zk-cards/fuzz/corpus/fuzz_hand_eval/$i bs=1 count=7 2>/dev/null
done
```

### Incremental Corpus Growth
libFuzzer automatically adds inputs that discover new coverage to `fuzz/corpus/fuzz_hand_eval/`. Commit regularly:

```bash
git add stellar-zk-cards/fuzz/corpus/fuzz_hand_eval/
git commit -m "fuzz(hand-eval): add corpus entries for improved coverage"
```

### Coverage Report
```bash
cd stellar-zk-cards
cargo fuzz cov fuzz_hand_eval
# Opens coverage report in `fuzz/coverage/fuzz_hand_eval/index.html`
```

## Integration with proptest

The same reference implementation (`circuit_evaluate_hand_rank`) is used by:
1. **proptest** (unit test suite) — deterministic, checked on every `cargo test`
2. **libFuzzer** (nightly CI) — coverage-guided, continuous search for edge cases

Both use the same oracle, ensuring consistency.

## Troubleshooting

### "libFuzzer has found an issue"
If the fuzzer finds a crash:

1. Check the crash input in the artifact:
```bash
hexdump -C stellar-zk-cards/crash-artifacts/crash-xyz
```

2. Reproduce locally:
```bash
cargo fuzz run fuzz_hand_eval stellar-zk-cards/crash-artifacts/crash-xyz
```

3. Add to proptest suite:
```rust
#[test]
fn test_regression_crash_xyz() {
    let cards = [/* from hexdump */];
    assert_eq!(
        contract_category(&cards),
        circuit_category(&cards)
    );
}
```

### "Coverage not growing"
If corpus reaches a plateau without finding new behaviors:

1. Extend fuzzing time:
```bash
cargo fuzz run fuzz_hand_eval -- -max_total_time=7200  # 2 hours
```

2. Try different seed mutations:
```bash
cargo fuzz run fuzz_hand_eval -- -seed=12345
```

3. Check libFuzzer's statistics:
```bash
cargo fuzz run fuzz_hand_eval -- -print_coverage=1
```

## Performance Characteristics

Typical performance on a modern CPU:
- **Throughput**: ~50,000–100,000 test cases/second
- **Time to full coverage**: <10 seconds
- **Nightly job (1 hour)**: ~180–360M test cases
- **Memory per process**: <100 MB

## Related Issues

- [#6](https://github.com/HitEmPoka/StellPoker/issues/6) — Property testing for hand evaluation (proptest)
- [#515](https://github.com/HitEmPoka/StellPoker/issues/515) — Fuzz targets for hand evaluator (this issue)

## References

- [libFuzzer documentation](https://llvm.org/docs/LibFuzzer/)
- [cargo-fuzz](https://docs.rs/cargo-fuzz/latest/cargo_fuzz/)
- [Rust fuzzing book](https://rust-fuzz.github.io/book/cargo-fuzz.html)

## Showdown winner consistency

Hand category agreement is necessary but not sufficient: settlement depends on the
circuit's `winner_index` and `tie_mask`. `docs/showdown-winner-consistency.md`
describes the suite that compares those two values against the contract's evaluator
on random deals, including a known kicker divergence.
