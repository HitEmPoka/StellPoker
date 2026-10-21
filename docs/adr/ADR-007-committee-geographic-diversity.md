# ADR-007: Committee Geographic and Operator Diversity Policy

**Status**: Accepted

---

## Context

ADR-005 fixes the committee at 3 nodes (REP3, 2-of-3 threshold). That topology
assumes the 3 nodes are meaningfully independent — if two of the three run in
the same region or on the same cloud operator, a single datacenter outage or
provider-level incident can take out both at once, defeating the fault
tolerance ADR-005 relies on. #243 documents how to deploy nodes across
datacenters; this ADR makes that a hard, enforced policy rather than an
operational recommendation.

`CommitteeMember` already carries a `region` field, declared at registration.
There was no `operator` field and no enforcement anywhere — two members in
the same region (or on the same cloud provider) could be selected into the
same epoch.

---

## Decision

Enforce, at `create_epoch` (the point where an epoch's 3 active members are
actually selected): **no two members in the same epoch may share a `region`
or a declared `operator`.**

- `region` is required at registration (unchanged) and always compared.
- `operator` is optional, set after registration via `set_operator` (kept as
  a separate call rather than widening `register_member`'s signature). An
  empty `operator` means "not yet declared" and is never treated as a
  collision with another undeclared operator — only two members who have
  both declared the *same* operator are rejected.

This is checked once, at epoch creation, rather than at registration, because
registration happens before a member is known to be selected into any
particular epoch — the diversity constraint only has meaning relative to the
other members actually being placed in service together.

---

## Options Considered

### Option A: Enforce at registration time

Reject a `register_member` call if its region is already used by an active
member.

**Rejected** — a region can have many registered members (that's the point of
having a pool to select from); the constraint is about which members end up
*together in one epoch*, not about the total registered pool.

### Option B: N-per-region cap instead of a hard "no two" rule

Allow up to N members per region, for topologies larger than 3.

**Rejected for now** — ADR-005 fixes the committee at exactly 3 members, so
"no two share a region" and "at most 1 per region" are equivalent at this
size. If the committee size ever becomes configurable, this ADR should be
revisited.

### Option C: Soft warning instead of a hard assert

Log a warning but allow the epoch to be created anyway.

**Rejected** — the whole point of ADR-005's fault-tolerance argument is
undermined if this is advisory only; the admin forming an epoch should not be
able to silently violate it.

---

## Consequences

- `create_epoch` now fails loudly (`"duplicate region among epoch members"` /
  `"duplicate operator among epoch members"`) if the admin selects members
  that violate diversity, rather than succeeding and quietly weakening fault
  tolerance.
- Operators remain optional metadata; this policy cannot yet enforce cloud
  diversity for members that haven't called `set_operator`. Making `operator`
  mandatory is a future follow-up once node operators have had time to adopt
  it (would otherwise break existing registered members retroactively).
