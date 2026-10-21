# Contract state testing

The poker-table contract has two complementary regression layers:

## Property-based betting tests

`contracts/poker-table/src/state_machine_test.rs` uses `proptest` to generate
player counts, stacks, and betting intents. Each generated action is applied
through the same betting transition function used by the contract and checks
turn rotation, legal phase transitions, chip conservation, non-negative
stacks, and round termination. Run it with:

```bash
cargo test -p poker-table state_machine_test
```

The test intentionally generates legal actions from higher-level intents so a
failure points to a state-machine invariant rather than merely a rejected
random payload. Generated regression artifacts are ignored; the test suite is
the reproducible source of those cases.

## Soroban state snapshots

The hand-lifecycle tests use Soroban test snapshots for contract storage after
important transitions, including table creation, joining, betting, dealing,
timeouts, and settlement. The committed fixtures live under
`contracts/poker-table/test_snapshots/`. Run the contract tests to compare
current state transitions with those fixtures:

```bash
cargo test -p poker-table
```

When an intentional contract-storage change updates a snapshot, review the
diff carefully before committing it. A snapshot update is expected to include
only the intended state-transition change; accidental fixture churn should be
discarded. CI runs the same contract test command, so a changed transition is
visible in the pull request rather than silently changing the baseline.
