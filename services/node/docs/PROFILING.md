# MPC Node Session Profiling (#244)

On-demand CPU/memory profiling per proof-generation session phase.

## Why not `pprof`?

The original issue asks for output "as pprof or flamegraph-compatible
format." A sampling profiler like the `pprof` crate walks *this node
process's own* call stack — but the actual MPC compute work for a session
runs in `co-noir` child processes (`session::run_proof_generation` spawns
one per phase: `merge_shares`, `witness_generation`, `proof_generation`).
An in-process profiler attached to the node would show almost nothing —
just the time spent spawning the child and waiting on it — because the
expensive work never executes on this process's stack at all.

Instead, profiling here samples each `co-noir` child process's OS-reported
CPU% and memory (RSS) on a fixed 200ms interval for the duration of its
phase, and aggregates that into a `PhaseProfile` per phase. This is
exported as JSON, not the pprof wire format: pprof's call-graph model
doesn't apply to an opaque external process whose internals this node has
no visibility into (a flamegraph needs sampled *stack traces*, which
`co-noir` doesn't expose to callers).

## API

Profiling is strictly **opt-in per session** — a session nobody asks to
profile pays zero sampling overhead beyond one registry lookup in
`post_generate`.

```
POST /session/:id/profile   — enable profiling for this session.
                               Must be called before POST /session/:id/generate;
                               enabling it after generation has started (or
                               finished) has nothing left to sample.
GET  /session/:id/profile   — returns the SessionProfile collected so far,
                               as JSON. 404 if profiling was never enabled
                               for this session_id.
```

### Example response

```json
{
  "session_id": "abc123",
  "phases": [
    { "phase": "merge_shares", "duration_ms": 812, "peak_memory_bytes": 41943040, "sample_count": 4, "avg_cpu_percent": 12.5, "peak_cpu_percent": 30.0 },
    { "phase": "witness_generation", "duration_ms": 15420, "peak_memory_bytes": 536870912, "sample_count": 77, "avg_cpu_percent": 88.0, "peak_cpu_percent": 100.0 },
    { "phase": "proof_generation", "duration_ms": 42110, "peak_memory_bytes": 2147483648, "sample_count": 210, "avg_cpu_percent": 95.0, "peak_cpu_percent": 100.0 }
  ]
}
```

A retried `proof_generation` attempt (see the retry loop in
`run_proof_generation` for transient resource errors) appends another
`"proof_generation"` entry rather than overwriting the previous attempt's,
so all attempts remain visible.

## Precision note

`duration_ms` is measured at the sampler's 200ms sampling granularity, not
true child-process wall-clock time — a phase that finishes faster than one
sampling interval is reported as taking roughly one interval with zero
samples. In practice, MPC witness/proof generation phases run for seconds
to minutes, well above that granularity, so this is a deliberate
simplicity/precision tradeoff rather than a correctness gap for the phases
this actually profiles.

## Implementation

- `src/profiling.rs` — `ProfileRegistry` (which sessions are enabled + what's
  been collected), `sample_process_until_exit` (the sampling loop, spawned
  as its own task per phase so it runs concurrently with awaiting the
  child).
- `src/session.rs`'s `run_profiled` helper wraps each `co-noir` subprocess
  call: when profiling isn't enabled for the session it's exactly
  `cmd.output().await` (zero extra cost); when it is, it spawns the child
  with piped stdio, starts a sampler task against the child's pid, and
  awaits both.
