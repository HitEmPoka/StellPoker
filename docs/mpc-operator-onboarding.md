# MPC Node Operator Onboarding Kit

Step-by-step guide for running an independent MPC node for Stellar Poker
(#685). `PROJECT.md` explicitly marks the current committee as demo
infrastructure — production needs operators who are **not** the core team,
each holding their own key material and infrastructure. Follow this document
start to finish; every command is copy-pasteable and every step has a
verification check.

> **Acceptance for this doc**: it is considered validated only after a
> second person has followed it end to end on fresh hardware, without
> asking anything not written here. Sign-off table at the bottom.

Time budget: ~half a day, plus the key ceremony (scheduling-dependent).

## 0. What you are operating

Three active nodes run the REP3 secret-sharing protocol (threshold 3-of-3
active, with 2 optional standby nodes — see
[mpc-committee-resilience.md](mpc-committee-resilience.md)). No single node
— including the coordinator — ever sees the full deck. Your duties:

- Keep the node process healthy and reachable.
- Protect your node's private key (enrollment ceremony).
- Apply updates when the committee rotates configs/CRS.
- Respond to committee incident pages.

Read these first (they are referenced throughout):
[mpc-key-enrollment.md](mpc-key-enrollment.md),
[node-attestation.md](node-attestation.md),
[committee-fee-distribution.md](committee-fee-distribution.md),
[mpc-api-key-auth.md](mpc-api-key-auth.md).

## 1. Hardware

Minimum per node (from `docs/production-config.md` sizing):

| Resource | Minimum | Notes |
|---|---|---|
| CPU | 2 vCPU (4 recommended) | Proof generation is CPU-bound; slower CPU = slower deals |
| RAM | 2 GB | ~500 MB of CRS data is loaded per node at start |
| Disk | 50 GB SSD (NVMe preferred) | CRS + circuit artifacts + logs |
| Network | 100 Mbit, stable latency to the other 2 nodes | **Co-locate with the other nodes only if you are a different operator** — independent operators should be in different regions/providers |
| OS | Linux x86_64 (Ubuntu 22.04/24.04 LTS verified) | Docker Engine ≥ 24 |

Verification:
```bash
nproc          # ≥ 2
free -h        # ≥ 2 GiB total
df -h /        # ≥ 50 GB free
docker version # ≥ 24
```

## 2. Keys

Your node's identity is a private key generated **by you**, distributed to
the other operators, and registered on-chain — never sent by anyone else.

1. Follow [mpc-key-enrollment.md](mpc-key-enrollment.md) exactly:
   - **Generate** on your machine (offline is better).
   - **Distribute** to the other operators over an authenticated channel
     they initiated (never accept an inbound "here is your key").
   - **Verify** a quorum can restore the key before registering.
   - **Register** on-chain from your own wallet.
2. Store a backup of the key material in a password manager / sealed
   envelope; document the recovery path for your organization.
3. Keep your operator wallet funded with XLM for on-chain registration and
   fee deposits (see step 7).

Verification: your node's public identity appears in the on-chain committee
registry and matches what you generated locally.

## 3. Provision and harden the host

```bash
# dedicated non-root user with docker access
sudo useradd -m -s /bin/bash mpc && sudo usermod -aG docker mpc
sudo su - mpc

git clone <repo-url> stellpoker && cd stellpoker
./scripts/download-crs.sh            # CRS artifacts (verify checksums it prints)
./scripts/generate-tls-certs.sh      # TLS material for node/coordinator traffic
```

Hardening checklist:

- [ ] Firewall: expose **nothing** publicly except the node health port
      (`8101`) if you want external monitoring — even that preferably behind
      your own allowlist. The MPC port (`10000`) must only be reachable from
      the coordinator and peer nodes.
- [ ] Enable automatic security updates (`unattended-upgrades`).
- [ ] Full-disk encryption for the key-material volume.
- [ ] SSH: key-only auth, no root login; consider a jump host.
- [ ] Rate limiting is configured (`services/node/config/rate-limits.toml`).
- [ ] Node API authentication keys are handled per
      [mpc-api-key-auth.md](mpc-api-key-auth.md) — never commit them.

Verification: `ss -tulpn` from a non-allowed host shows no `10000`/`8101`
listening; `sudo ufw status` shows default deny inbound.

## 4. Deploy the node

Using the provided Dockerfile (recommended):

```bash
cp services/node/config/party_0.toml services/node/config/local/  # your role's config
$EDITOR services/node/config/party_0.toml   # identity, peers' hostnames, ports
docker compose -f docker-compose.yml up -d --build mpc-node-0
```

- One node process per role (`party_0/1/2.toml`); the coordinator addresses
  nodes at `http://<host>:810x`.
- Session transport is TLS + per-session encryption — see
  [mpc-session-encryption.md](mpc-session-encryption.md).
- Standbys are optional and start only with the `standby` profile
  ([mpc-committee-resilience.md](mpc-committee-resilience.md)).

Verification:
```bash
curl -fsS http://localhost:8101/health        # → ok
docker compose logs mpc-node-0 | tail -50     # no CRS/circuit load errors
```

## 5. Attestation and committee admission

The coordinator only accepts peers with valid build attestation
([node-attestation.md](node-attestation.md)):

1. Build/publish your node image provenance as described in the attestation
   doc (or use the signed upstream image and register its digest).
2. The coordinator's policy (strict mode) refuses unattested peers — you are
   admitted once your attestation verifies.
3. Run a smoke session with the coordinator: a test hand should complete
   deal → bet → showdown without your node erroring.

Verification: coordinator logs show your node as an accepted peer; one test
hand settles.

## 6. Monitoring and alerting

Minimum monitoring setup:

| Signal | Source | Alert when |
|---|---|---|
| Health endpoint | `GET :8101/health` every 60 s | 2 consecutive failures |
| Container status | `docker ps` / restart count | restart > 2 in 15 min |
| Session participation | JSON request logs (`REQUEST_LOG_FORMAT=json`) | missed/late MPC sessions (compare latency vs [MPC_LATENCY_BENCHMARKS.md](../services/node/docs/MPC_LATENCY_BENCHMARKS.md)) |
| Disk/CPU/RAM | node exporter or cloud monitor | disk > 80%, CPU sustain > 90% |
| CRS/circuit integrity | startup log checksum lines | any mismatch at boot |

- Ship logs to your own log store; logs are redaction-safe by design
  (`scripts/check_log_redaction.py` — keep it green).
- Profiling when things slow down: [PROFILING.md](../services/node/docs/PROFILING.md).
- Page-rotation/ownership belongs with your org's on-call, not the core
  team's.

Verification: stop the container, confirm your alert fires, restart it.

## 7. Fees, stake, and accounting

Operators earn a share of the rake in proportion to stake
([committee-fee-distribution.md](committee-fee-distribution.md)):

1. **Deposit** the required stake from your operator wallet.
2. Fees accrue per settled hand; check your balance with the registry's
   read functions (or the coordinator's accounting endpoints).
3. **Withdraw** follows the documented cooldown; never bypass it.
4. Keep bookkeeping of deposits/withdrawals — you are running a business
   relationship, not a hobby box.

Verification: a test hand's settlement shows a fee accrual line for your
node; deposit and withdraw round-trip on testnet before mainnet.

## 8. Incident duties

- Node down > 15 min → notify committee operators (see resilience doc for
  failover/standby promotion: `scripts/promote-mpc-standby.sh <role>`).
- Key compromise → immediate rotation per the recovery section of
  [mpc-key-enrollment.md](mpc-key-enrollment.md).
- Planned maintenance → announce ahead; the committee schedules around it.

## 9. Onboarding checklist (sign off when done)

- [ ] Hardware provisioned and verified (step 1)
- [ ] Keys generated/distributed/verified/registered by me, never received (step 2)
- [ ] Host hardened; ports locked down (step 3)
- [ ] Node deployed; `/health` green (step 4)
- [ ] Attestation accepted; test hand settles (step 5)
- [ ] Monitoring live and alert tested (step 6)
- [ ] Stake deposited; fee accrual observed (step 7)
- [ ] Incident contacts and key-recovery path documented (step 8)

| Operator | Date | Second-person reviewer (doc-only run-through) | Date |
|---|---|---|---|
| | | | |

Feedback loop: anything you had to ask that this document did not answer is
a doc bug — open an issue with label `mpc` + `documentation`.
