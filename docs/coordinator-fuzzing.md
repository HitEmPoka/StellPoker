# Coordinator Endpoint Fuzzing (Issue #505)

## Overview

This document describes the fuzzing infrastructure for StellPoker coordinator HTTP endpoints, ensuring robust handling of malformed requests, edge cases, and unexpected input variations across all public API surfaces.

## Fuzzing Targets

The coordinator fuzzing infrastructure implements schema-aware, coverage-guided fuzzing across 4 target families covering 20+ HTTP endpoints:

### 1. Table Lifecycle (`fuzz_table_lifecycle`)
**Endpoints:**
- `POST /api/tables/create`
- `GET /api/tables/open`
- `GET /api/tables/overview`
- `POST /api/table/{id}/join`
- `GET /api/table/{id}/lobby`

**Coverage:** Table creation, lobby management, seat assignment, state queries.

### 2. Game Actions (`fuzz_game_actions`)
**Endpoints:**
- `POST /api/table/{id}/request-deal`
- `POST /api/table/{id}/request-reveal/{phase}`
- `POST /api/table/{id}/request-showdown`
- `POST /api/table/{id}/player-action`
- `POST /api/table/{id}/transfer-chips`
- `POST /api/table/{id}/rit-opt-in`

**Coverage:** MPC proof generation, game progression, player actions, chip transfers.

### 3. State Queries (`fuzz_state_queries`)
**Endpoints:**
- `GET /api/table/{id}/state`
- `GET /api/table/{id}/mpc-status`
- `GET /api/table/{id}/spectators`
- `GET /api/table/{id}/player/{address}/cards`
- `GET /api/chain-config`

**Coverage:** Read-only state access, player card queries, chain configuration.

### 4. Node Discovery (`fuzz_node_discovery`)
**Endpoints:**
- `POST /api/node/register`
- `POST /api/node/{id}/heartbeat`
- `DELETE /api/node/{id}`
- `GET /api/node/{id}`

**Coverage:** MPC node registration, health checks, dynamic discovery.

## Fuzzing Approach

### Schema-Aware Generation
- Uses `arbitrary` crate with `#[derive(Arbitrary)]` on request types
- Generates structurally valid JSON payloads within schema constraints
- Tests edge cases: empty strings, maximum values, duplicate keys, extreme field combinations

### Invariant Testing
Each fuzz target asserts that handlers:
1. **Never panic** on any input (no `unwrap`, `expect`, `panic!`)
2. **Return typed errors** (`StatusCode` or custom error enums)
3. **Handle timeouts gracefully** (no deadlocks)
4. **Validate inputs robustly** (bounds checking, UTF-8, parsing)
5. **Maintain state consistency** (no partial updates)

### Coverage Areas
- **Input validation**: Field bounds, type mismatches, encoding issues
- **Rate limiting**: Concurrent requests, quota enforcement
- **Authentication**: Invalid signatures, replay attacks, missing auth
- **MPC coordination**: Node failures, session management, proof validation
- **Database operations**: Transaction safety, constraint violations

## CI Integration

### PR Fuzzing (`fuzz-coordinator-pr.yml`)
- **Trigger**: Every PR touching `services/coordinator/src/**`
- **Duration**: 30 seconds per target (2 minutes total)
- **Purpose**: Smoke test for immediate feedback
- **Timeout**: 15 minutes job limit

### Nightly Fuzzing (`fuzz-coordinator-nightly.yml`)
- **Schedule**: Daily at 3 AM UTC
- **Duration**: 10 minutes per target (40 minutes total)
- **Purpose**: Deep exploration, regression detection
- **Timeout**: 90 minutes job limit

### Artifact Handling
- **Crash inputs**: Uploaded on failure, 30-day retention
- **Statistics**: Execution counts, input sizes, coverage metrics
- **Notifications**: Failing jobs trigger issue creation

## Local Usage

### Running Fuzz Targets
```bash
cd services/coordinator

# Install cargo-fuzz (if not already installed)
cargo install cargo-fuzz

# Run specific target for 60 seconds
cargo fuzz run fuzz_table_lifecycle -- -max_total_time=60

# Run all targets in sequence
for target in fuzz_table_lifecycle fuzz_game_actions fuzz_state_queries fuzz_node_discovery; do
  echo "Running $target..."
  timeout 60 cargo fuzz run $target || true
done
```

### Debugging Crashes
```bash
# Reproduce a specific crash
cargo fuzz run fuzz_game_actions crash-<hash>

# Add debug prints to fuzz target
# Edit fuzz_targets/fuzz_game_actions.rs, add logging
cargo fuzz run fuzz_game_actions -- -runs=1
```

## Triage Process

### Crash Severity Matrix
- **Critical**: Panic, segfault, infinite loop, data corruption
- **High**: Authorization bypass, state inconsistency, resource leak  
- **Medium**: Performance degradation, error message leaks
- **Low**: Cosmetic issues, non-exploitable edge cases

### Investigation Steps
1. **Reproduce locally** using crash input artifact
2. **Identify root cause** (input validation, logic error, dependency issue)
3. **Assess impact** (security, availability, correctness)
4. **Create GitHub issue** with:
   - Crash input (minimized if possible)
   - Stack trace or error details
   - Affected endpoints/code paths
   - Severity assessment
5. **Fix and validate** with regression test

### Issue Template
```markdown
## Coordinator Fuzzing Crash

**Target**: fuzz_game_actions
**Input**: [attach crash artifact]
**Error**: [stack trace or panic message]

**Reproduction**:
```bash
cd services/coordinator
cargo fuzz run fuzz_game_actions crash-<hash>
```

**Impact**: [security/availability/correctness analysis]
**Priority**: [critical/high/medium/low based on matrix above]
```

## Implementation Details

### Request Type Coverage
The following request types have `#[derive(Arbitrary)]`:
- `CreateTableRequest`: max_players, solo mode, buy_in, region
- `DealRequest`: player list, circuit selection
- `PlayerActionRequest`: action type, amount, sequence number
- `RegisterNodeRequest`: node ID, endpoint URL
- `TransferChipsRequest`: destination table, amount
- `RitOptInRequest`: boolean flag

### Fuzzing Infrastructure Files
```
services/coordinator/
├── Cargo.fuzz.toml           # Fuzz target configuration
├── fuzz_targets/
│   ├── fuzz_table_lifecycle.rs   # Table creation & lobby
│   ├── fuzz_game_actions.rs      # Game progression & actions
│   ├── fuzz_state_queries.rs     # Read-only state access
│   └── fuzz_node_discovery.rs    # Node registry operations
├── Cargo.toml                 # Added arbitrary dependency
└── src/api/types.rs           # Added Arbitrary derives
```

### CI Workflows
```
.github/workflows/
├── fuzz-coordinator-pr.yml       # Short PR runs
└── fuzz-coordinator-nightly.yml  # Long nightly runs
```

## Future Enhancements

### Stateful Fuzzing
Current targets test individual requests in isolation. Future work could:
- Maintain coordinator state across fuzz iterations
- Test request sequences (create → join → deal → showdown)
- Validate state machine transitions

### Property-Based Testing
Integration with `proptest` for:
- Algebraic properties (commutativity, associativity)
- Invariant preservation across operations
- Relationship testing between endpoints

### Performance Fuzzing
- Response time regression detection
- Memory usage profiling under load
- Resource exhaustion testing (file descriptors, connections)

## References

- [libFuzzer Documentation](https://llvm.org/docs/LibFuzzer.html)
- [Arbitrary Crate](https://docs.rs/arbitrary/latest/arbitrary/)
- [Issue #505: Coordinator Endpoint Fuzzing](https://github.com/HitEmPoka/StellPoker/issues/505)
- [Issue #132: Existing Hand Evaluator Fuzzing](https://github.com/HitEmPoka/StellPoker/issues/132)