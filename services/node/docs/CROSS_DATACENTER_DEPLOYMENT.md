# Cross-Datacenter MPC Node Deployment Guide (#243)

The default deployment (`infrastructure/helm/mpc-node`) puts all 3 REP3
parties in one Kubernetes cluster, talking over cluster-internal DNS and
`ClusterIP`/headless services. That's the right default for most
deployments, but it also means a single cluster outage takes down every
party at once — defeating a large part of the point of a 3-party MPC
scheme. This guide covers what changes when the 3 nodes are spread across
different cloud providers or physically separate data centers instead.

## Network layers this contract touches

Three independent network paths exist between the coordinator and the 3
MPC nodes, and each has its own transport/auth story:

| Path | Transport | Config |
|---|---|---|
| Coordinator → node | HTTP(S), optional mTLS pinning | `TLS_SERVER_CERT_PATH`/`_KEY_PATH`, `COORDINATOR_TLS_PIN_*` (see `src/tls.rs`) |
| Node → node (private-table share dispatch) | HTTP | `NODE_HTTP_ENDPOINTS` (comma-separated, see `main.rs`) |
| Node → node (co-noir MPC protocol) | Raw TCP, per-party TLS | Each party's `[network]`/`[[network.parties]]` TOML — `bind_addr`, `dns_name`, `cert_path`, `key_path` (see `config/local/party_*.toml`) |

All three need to resolve to real, routable, cross-datacenter addresses —
not the `127.0.0.1:1000X` defaults the local party configs ship with — for
a genuinely distributed deployment.

## Latency requirements

`co-noir`'s REP3 protocol is round-trip-heavy: every MPC operation
requires synchronous communication between all 3 parties, so the *slowest*
pairwise round-trip time (RTT) directly bounds proof-generation latency —
unlike most distributed systems where only the client-facing edge is
latency-sensitive.

- **Budget RTT, not one-way latency.** A witness-generation phase that
  takes 5s on a single-cluster deployment (sub-1ms RTT) can easily take
  30s+ once party RTT is in the 30-80ms cross-region range — every
  MPC round consumes it multiplicatively across the circuit's round
  count, not additively once.
- **Check against the existing phase timeouts before deploying
  cross-region.** `PhaseTimeouts` (`session.rs`, configurable via
  `PHASE_TIMEOUT_MERGE_SECONDS` / `PHASE_TIMEOUT_WITNESS_SECONDS` /
  `PHASE_TIMEOUT_PROOF_SECONDS`, defaults 60s/300s/600s) and the overall
  `SESSION_MAX_WALL_SECONDS` (default 600s) were sized against a
  low-latency, same-cluster deployment. Measure actual witness/proof
  phase duration in a cross-DC staging environment (the profiling API —
  see `PROFILING.md` — reports per-phase duration directly) before
  trusting the defaults in production; raise the timeout env vars if the
  measured p99 phase duration is within 2x of the current budget.
- **Prefer datacenters on the same continent, or the same cloud
  provider's backbone, over maximally-distant placement.** The security
  benefit of 3-party MPC comes from independent failure/compromise
  domains (different providers, different physical sites, ideally
  different legal jurisdictions for operator diversity) — it does not
  require maximizing geographic distance, which only adds RTT for no
  additional protection. A common, reasonable choice is one party per
  major cloud provider (AWS, GCP, and a colo/bare-metal provider, for
  example) within the same region tier.
- **Budget for the coordinator too.** The coordinator polls
  `GET /session/:id/status` and fans out `POST /session/:id/shares` /
  `/generate` to all 3 nodes — its own RTT to the *farthest* node adds to
  end-to-end latency independent of inter-party RTT. Co-locating the
  coordinator near one party doesn't help; co-locating it near none of
  them (a neutral, central location) avoids biasing total latency toward
  whichever party ends up farthest.

## Firewall rules

Per node, opened only to the specific peers listed below — never to
`0.0.0.0/0`:

| Port | Direction | From | Purpose |
|---|---|---|---|
| `8101` (HTTP API, `PORT` env) | Inbound | Coordinator's egress IP/CIDR only | Session lifecycle (`/session/:id/shares`, `/generate`, `/status`, `/proof`, `/profile`), table prep (`/table/:id/prepare-*`) |
| `10000`–`10002` (`MPC_PORT` / party `bind_addr`) | Inbound | The other 2 parties' egress IPs/CIDRs only | co-noir's REP3 protocol traffic |
| `8101` (HTTP API) | Outbound | To the other 2 parties' HTTP endpoints | Private-table share dispatch (`NODE_HTTP_ENDPOINTS`) — each node calls its peers, not just the coordinator calling nodes |
| `9090`-range or whatever `/metrics` is bound to | Inbound | Prometheus scraper only | Metrics scraping (`metrics::metrics_endpoint`) — never expose to the public internet |

Since the 3 parties now sit in different networks/providers by design,
this can't rely on being "inside the same VPC" or "same cluster" for
implicit trust — each of these rules needs an explicit security-group /
firewall entry per pair, in both directions where the table above says
outbound too. With 3 parties that's 3 pairs × 2 directions for the MPC
port, plus each node's egress rule to the other 2 for the HTTP path — not
a large rule set, but worth enumerating explicitly rather than discovering
a missing rule via a hung session in production.

**TLS is not optional for cross-datacenter traffic.** Every party config
already requires `cert_path`/`key_path` per peer for the MPC port (see
`config/local/party_0.toml`), and the coordinator-facing HTTP API supports
mTLS via `TLS_SERVER_CERT_PATH`/`COORDINATOR_TLS_PIN_*`. Within a single
trusted cluster, skipping the coordinator-facing TLS pin is a reasonable
simplification; across the public internet between datacenters, both
layers should be enabled and cert-pinned, not left on their permissive
defaults.

## DNS setup

- Each party's TOML config (`[[network.parties]] dns_name = "..."`) needs
  a stable, resolvable hostname (or static IP) per party — not the
  cluster-internal headless-service DNS the Helm chart's default
  `partyConfigs` example assumes. A short-TTL DNS record per party (e.g.
  `mpc-node-0.mpc.example.com`) that each datacenter's ops team can
  repoint independently is preferable to hardcoded IPs, since it lets one
  party be relocated (disaster recovery, provider migration) without
  re-keying or redistributing config to the other two parties.
- `NODE_HTTP_ENDPOINTS` (comma-separated `http://` or `https://` URLs) must
  list all 3 nodes' HTTP endpoints using the *same* stable hostnames — keep
  the MPC-port and HTTP-port DNS names consistent (e.g.
  `mpc-node-0.mpc.example.com` for both, differing only by port) so a
  config or DR update only needs to change one DNS record, not two.
- The coordinator's own address, pinned via `COORDINATOR_TLS_PIN_*` on
  each node, should similarly be a stable hostname rather than an IP the
  coordinator's own infrastructure might reassign.
- Cross-datacenter DNS resolution itself must be reliable and fast: a
  node that can't resolve a peer's hostname fails every session it
  participates in, not just the request that triggered the lookup. Use
  each datacenter's normal external DNS (not internal-only resolvers that
  can't see the other datacenters), and confirm resolution works from
  each node to each peer *before* declaring a datacenter live, not
  during the first real session.

## Disaster recovery

Because REP3 requires all 3 parties to participate in every session, the
loss of *any one* party halts all in-flight and new sessions — this
system does not tolerate a single node's permanent loss without a re-key
ceremony (see `mpc-enroll`, issue #247, and
`ADMIN_ROTATION_PLAYBOOK.md`/`INITIALIZATION_DEPLOYMENT_CHECKLIST.md` at
the repo root for the key-material side of that). Disaster recovery
planning here is specifically about *transient* datacenter loss, where the
same key material needs to come back online — not about permanently
replacing a party's key share.

- **What must survive a datacenter loss:** the party's private key/cert
  (`key_path`/`cert_path` in its TOML, or the corresponding Kubernetes
  `Secret` if `useSecret: true`), its TOML config for the *other two*
  parties' addresses, and nothing else — session state itself is
  intentionally ephemeral (in-memory `sessions: Arc<RwLock<HashMap<...>>>`,
  per `main.rs`) and is not expected to survive a node restart regardless
  of cause. A session in flight when a party goes down should be treated
  as lost and retried by the coordinator once the party recovers, not
  recovered in place.
- **Back up the party's key material out-of-band, encrypted, and
  restricted to that datacenter's own operators** — not centrally with
  the other two parties' keys, since that would recreate the single
  point of compromise cross-datacenter deployment is meant to avoid in
  the first place.
- **Recovery runbook, per party:**
  1. Provision a replacement host/pod in the same (or a new) datacenter.
  2. Restore the party's key/cert material from its out-of-band backup.
  3. Restore its TOML config — the *other two* parties' `dns_name`/`cert_path`
     entries don't change unless they also moved, so this is normally
     just redeploying the same config, not reconstructing it from scratch.
  4. Bring the party's HTTP (`8101`) and MPC (`1000X`) ports back up behind
     the *same* DNS name it had before (see DNS section above) — this is
     why a stable, independently-repointable DNS name per party matters:
     recovery doesn't require updating the other two parties' configs at
     all if the name comes back pointing at the new host.
  5. Confirm connectivity to both other parties (firewall rules, TLS
     handshake) before resuming session routing to it — bring it back
     into rotation deliberately, not by simply having it start accepting
     traffic and finding out mid-session that a firewall rule was missed
     in the new location.
  6. Any sessions that were in flight through the recovered party at the
     time of the outage are already lost (see above) — no replay/resume
     step is needed for them; the coordinator's normal retry path handles
     re-initiating affected work.
- **What does *not* need cross-datacenter replication:** circuit
  artifacts (`CIRCUIT_DIR`) and the CRS (`CRS_DIR`) are public, non-secret
  build/reference data — restore them from the same build pipeline/CDN
  used to provision any node, not from a peer party.
