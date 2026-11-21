# Incident Response — Severity Definitions and Paging Policy

This document defines StellPoker's incident severities (P0–P3), who gets paged
for each level, and the response expectations. It complements the existing
runbooks (`disaster-recovery.md`, `coordinator-troubleshooting.md`,
`mpc-committee-resilience.md`) and the post-mortem template
(`postmortem-template.md`).

Treat this as the authoritative source: if a runbook disagrees on severity or
paging, update the runbook, not this document.

---

## Severity Levels

Severity reflects **customer impact and reversibility**, not the technical
complexity of the fix.

### P0 — Critical

**Definition.** Funds at risk, player data exposed, or the system is unusable
for everyone.

**Examples.**
- Smart-contract pause triggered or an exploit is suspected to be in-flight.
- MPC committee has lost quorum and cannot sign; new hands cannot start.
- Coordinator is down for all users for more than 2 minutes.
- Confirmed PII/key leak or unauthorized access to coordinator or node hosts.
- Database corruption or data-loss event detected in production.

**Who is paged.** On-call engineer **and** on-call security lead,
**immediately** via PagerDuty high-urgency. Incident commander auto-assigned.
Executive notification within 15 minutes.

**Response SLO.**
- Acknowledge: 5 minutes.
- Mitigate (traffic routed away, contract paused, keys rotated): 30 minutes.
- Public status-page update: 15 minutes, then every 30 minutes.
- Post-mortem published: within 5 business days (`postmortem-template.md`).

---

### P1 — High

**Definition.** Major functionality broken for a large subset of users, or a
single-tenant outage with no self-service recovery. No immediate fund risk.

**Examples.**
- One MPC node down (committee still meets threshold but is at risk).
- Hand-history writes failing for one or more tables.
- Deposits/withdrawals succeeding on-chain but not reflected in the UI.
- Coordinator error rate above 5% for 10 minutes.
- Staging mainnet-mirror broken ahead of a planned release.

**Who is paged.** On-call engineer via PagerDuty high-urgency. Security on-call
is informed (not paged) if the symptom could indicate a security issue.

**Response SLO.**
- Acknowledge: 15 minutes.
- Mitigate: 2 hours.
- Public status-page update if user-visible: 30 minutes.
- Post-mortem: within 10 business days if customer-visible.

---

### P2 — Moderate

**Definition.** Degraded experience, partial feature outage, or a workaround
exists. Minority of users affected.

**Examples.**
- Elevated p99 latency on `coordinator_request_latency_seconds`.
- One load-balancer region unhealthy with others serving traffic.
- Metrics or logging pipeline degraded (observability loss, not product loss).
- Non-critical dashboard broken.

**Who is paged.** On-call engineer via PagerDuty low-urgency (business hours
only). Filed as a tracked incident regardless.

**Response SLO.**
- Acknowledge: next business hour.
- Mitigate: 1 business day.
- Post-mortem: optional, at on-call's discretion.

---

### P3 — Low

**Definition.** Cosmetic bugs, minor logging noise, or issues with a trivially
clear fix and no user impact.

**Examples.**
- Grafana panel showing a stale title after a dashboard rename.
- Noisy warning in logs with no functional impact.
- Documentation drift.

**Who is paged.** Nobody. File a ticket on the next sprint board.

**Response SLO.** Best-effort during normal engineering work.

---

## Paging Rotations

| Rotation | Covers | Hours | Tool |
|---|---|---|---|
| Platform on-call | Coordinator, DB, infra | 24/7 | PagerDuty — `stellpoker-platform` |
| MPC on-call | MPC nodes, committee health | 24/7 | PagerDuty — `stellpoker-mpc` |
| Security on-call | Suspected exploit, key compromise, PII leak | 24/7 | PagerDuty — `stellpoker-security` |
| Frontend on-call | Web app, wallet integration | Business hours | PagerDuty — `stellpoker-frontend` |

**Escalation.** If the primary on-call does not acknowledge within the SLO
above, PagerDuty auto-escalates to the secondary, then to the engineering
manager on-call.

---

## Declaring and Running an Incident

1. **Declare.** Anyone — engineer, support, or auto-alert — can declare an
   incident by paging the relevant rotation. Use the highest severity that
   fits; the on-call can downgrade later.
2. **Incident channel.** Open `#inc-YYYYMMDD-<slug>` in Slack. Pin the alert,
   the dashboard link, and the current incident commander.
3. **Roles.** For P0/P1 assign explicitly: Incident Commander, Comms Lead,
   Scribe. One person can hold multiple roles for P1 if the team is small.
4. **Status page.** Comms Lead posts on the external status page per the SLOs
   above. Do not wait for root cause; "we are investigating" is a valid update.
5. **Mitigate first, understand later.** Prefer rollback or failover over
   diagnosis in the live window. The `disaster-recovery.md` runbook has the
   emergency levers.
6. **Resolve.** Downgrade severity as impact subsides. Close the incident when
   metrics return to baseline and no mitigation is still active.
7. **Post-mortem.** Fill in `docs/postmortem-template.md` within the SLO above.
   Blameless, written for people who were not there.

---

## Related Runbooks

- `disaster-recovery.md` — emergency pause, failover, PITR restore.
- `coordinator-troubleshooting.md` — coordinator-specific triage.
- `mpc-committee-resilience.md` — committee degradation and recovery.
- `postmortem-template.md` — the form every P0/P1 incident fills in.
