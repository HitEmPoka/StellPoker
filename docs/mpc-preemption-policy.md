# MPC Session Preemption Policy

Issue #588. When the committee has less capacity than demand — fewer healthy
MPC nodes than hands waiting to be dealt — the coordinator has to choose which
sessions run. This is the policy it applies, and the starvation guarantees it
provides.

Implementation: `services/coordinator/src/session_preemption.rs`.
Inspect the live state at `GET /api/mpc/preemption/status`.

## Priority classes

Every table is assigned a [`PriorityClass`][class]:

| Class | Meaning | Base rank |
|---|---|---|
| `priority` | Operator-committed, latency-sensitive tables (televised / contractual uptime). | 0 (first) |
| `standard` | Ordinary paid tables. | 1 |
| `casual` | Free / casual play. | 2 (last) |

The class is attached by the caller of `enqueue`. Unknown labels parse to
`standard`, never to `priority`: a misconfigured caller can never accidentally
receive elevated priority, and is never silently dropped.

[class]: ../services/coordinator/src/session_preemption.rs

## Scheduling

Admission runs whenever a request is enqueued or a running session completes:

1. While `running < max_concurrent`, admit the best queued request.
2. When full, a request may **preempt** a running session:
   - only a strictly higher class preempts a lower one, unless the queued
     request is starving (see below);
   - the victim is **requeued**, never dropped, keeping its original arrival
     time and incrementing its preemption counter;
   - a victim that has already been preempted `max_preemptions` times, or that
     was admitted while starving, is **protected** and is never preempted
     again.
3. Ordering key for the queue is `(promoted, base_rank, arrival_seq)` — the
   smallest key wins, so ties within a class are FIFO.

A preempted session is not lost: it flows back through the queue and its
coordinator-side session state is preserved. On the MPC nodes the session's
share material is unaffected until it is actually dispatched again.

## Starvation bounds

Each class has a maximum wait (`priority_max_wait`, `standard_max_wait`,
`casual_max_wait`). Once a request has waited longer than its bound it is
**promoted**: it moves to the head of the queue and runs next time a slot is
free, and it is protected from preemption. Promotion is what turns "priority
always wins" into a bounded-latency policy:

> Worst-case wait for a class ≈ `starvation_bound`
> + `(max_preemptions + 1) × max_session_duration`

The second term only applies while every slot is occupied by a protected
session; it is bounded because a protected session is never preempted and
therefore always makes forward progress.

## Configuration

All knobs are environment variables with safe defaults:

| Variable | Default | Meaning |
|---|---|---|
| `MPC_PREEMPTION_MAX_CONCURRENT` | `16` | Sessions allowed to run at once. |
| `MPC_PREEMPTION_PRIORITY_MAX_WAIT_SECS` | `30` | `priority` starvation bound. |
| `MPC_PREEMPTION_STANDARD_MAX_WAIT_SECS` | `60` | `standard` starvation bound. |
| `MPC_PREEMPTION_CASUAL_MAX_WAIT_SECS` | `120` | `casual` starvation bound. |
| `MPC_PREEMPTION_MAX_PREEMPTIONS` | `2` | Times a session may be preempted. |
| `MPC_PREEMPTION_MAX_QUEUED` | `10000` | Hard queue bound; beyond this `enqueue` returns `QueueFull` so back-pressure reaches the caller. |

## Status endpoint

```bash
curl http://localhost:8080/api/mpc/preemption/status
```

```json
{
  "max_concurrent": 16,
  "running": 4,
  "queued": 2,
  "queued_priority": 0,
  "queued_standard": 1,
  "queued_casual": 1,
  "max_preemptions": 2,
  "priority_max_wait_secs": 30,
  "standard_max_wait_secs": 60,
  "casual_max_wait_secs": 120,
  "stats": {
    "admitted": 128,
    "preemptions": 3,
    "starvation_promotions": 1,
    "rejected_queue_full": 0
  }
}
```

## Tests

The controller is pure and time-injected, so the policy is unit-tested
directly (see the `tests` module in `session_preemption.rs`): priority-before-
casual, FIFO tie-breaks, starvation promotion ahead of fresh priority,
strictly-higher-class preemption, equal-class non-preemption, protection of
promoted/maximally-preempted sessions, slot release on completion, duplicate
rejection, and the queue bound.
