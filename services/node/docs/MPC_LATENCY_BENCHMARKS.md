# MPC Deal-Phase Latency vs Message Size (#580)

Expands on #100's round-trip latency measurements (2–6 players, broken down
by phase) with a message-size dimension: how deal-phase message
(de)serialization latency scales as the table's player count — and
therefore the size of the `public_inputs.json` payload `run_proof_generation`
reads back after every proof — grows.

## Scope

This benchmark measures the (de)serialization step specifically, via
`services/node/benches/mpc_message_size.rs` (criterion, `cargo bench`), not
a full live multi-node MPC round trip. A true end-to-end matrix would need a
running 3-node cluster driving real `co-noir` subprocesses at varying
payload sizes — accurate, but slow and not CI-friendly. Serialization cost
is the part of the round trip that scales directly and measurably with
message size without that infrastructure; the coordinator's existing
`mpc_benchmark.rs` (#100) already captures live phase-by-phase timings
end-to-end for production sessions.

## Methodology

The `deal_valid` circuit's public outputs are `[Field; MAX_PLAYERS]`-shaped
arrays (`circuits/deal_valid/src/main.nr`), so the public-inputs payload
size grows with player count. The benchmark builds a JSON payload shaped
like the real `public_inputs.json` (hex-encoded Field strings, one per
player slot) for 2, 3, 4, 5, and 6 players, and measures `serde_json`
serialize and deserialize latency for each size via criterion's
`BenchmarkId`-parameterized groups.

## Running it

```sh
cd services/node
cargo bench --bench mpc_message_size
```

Criterion writes full statistical reports (mean, median, outliers) to
`target/criterion/mpc_deal_phase_message_size/`; this doc intentionally
does not hardcode specific timing numbers, since those are
hardware-dependent and should be read from a fresh run on the machine
being used to size timeouts, not copied from this file.

## Using results to set default timeouts

`services/node/src/limits.rs::ResourceLimits::default()` currently sets:

- `max_session_cpu_seconds: 300` (5 min)
- `max_session_wall_seconds: 600` (10 min)

Both bound the *entire* session (merge_shares + witness_generation +
proof_generation), of which message (de)serialization is one small,
measured component — not the dominant cost (witness generation and proving
are). This benchmark's role in timeout-setting is to confirm serialization
overhead stays negligible relative to those budgets as player count scales
to the maximum (6); it is not expected to be the basis for *changing* the
existing 300s/600s defaults on its own. If a run on representative hardware
shows serialization cost becoming non-negligible at 6 players, revisit
`max_session_cpu_seconds`/`max_session_wall_seconds` together with #100's
witness/proving timings, not from this benchmark in isolation.
