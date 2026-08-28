# Developer Onboarding Checklist

Work top to bottom. Each step has a **verify** line — if it fails, fix that step
before moving on, because everything below assumes it worked.

Budget roughly an hour on a fast machine; the Rust and circuit builds dominate.

---

## 0. Toolchain

- [ ] Rust (stable), with the `wasm32-unknown-unknown` target
- [ ] Node.js 18+
- [ ] Docker + Docker Compose
- [ ] `nargo` 1.0.0-beta.17 — `noirup -v 1.0.0-beta.17`
- [ ] Stellar CLI — `cargo install stellar-cli --features opt`
- [ ] `co-noir` — `cargo install --git https://github.com/TaceoLabs/co-snarks --branch main co-noir`
- [ ] Python 3 with `requests` and `pynacl` (for `scripts/test-flow.py`)

> **Nix users:** `nix develop` provides Rust (with the wasm target), Node,
> Python, Docker Compose, and the system libraries in one shot. See
> [flake.nix](../flake.nix). `nargo` and `co-noir` are not in nixpkgs and are
> still installed via `noirup` / `cargo install` — the shell hook prints the
> exact commands and keeps them in a project-local `.nix/` directory.

**Verify:** `rustc --version && node --version && nargo --version && stellar --version && co-noir --version`

## 1. Clone

- [ ] `git clone https://github.com/HitEmPoka/StellPoker.git && cd StellPoker`

**Verify:** `git status` is clean.

## 2. Install dependencies

- [ ] `./scripts/setup.sh` — installs toolchains and verifies the workspace builds
- [ ] `cd app && npm ci && cd ..` — frontend dependencies

**Verify:** `cargo check --workspace` succeeds.

## 3. Configure environment

- [ ] `cp .env.example .env`
- [ ] Read through `.env` — the defaults target the local stack; nothing needs a
      real secret for local development.

`.env.local` is written for you by `scripts/deploy-local.sh` in step 6 and holds
the deployed contract ids. Do not commit either file; both are gitignored.

**Verify:** `.env` exists and `grep -c '=' .env` is non-zero.

## 4. Download the CRS

- [ ] `./scripts/download-crs.sh` — BN254 common reference string used by
      UltraHonk proving. Large; run it once.

**Verify:** the `crs/` directory is non-empty.

## 5. Start a local Soroban network

- [ ] `docker-compose up -d soroban`
- [ ] Wait for the container to report healthy: `docker-compose ps`

**Verify:** `curl -s http://localhost:8000/friendbot?addr=GAAA... ` returns a JSON
error rather than a connection refusal — the RPC is up.

## 6. Compile and deploy the contracts

- [ ] `./scripts/compile-circuits.sh` — compiles the Noir circuits (needed
      before proving; also produces the verification keys)
- [ ] `cargo build --release --target wasm32-unknown-unknown` — builds the
      Soroban contracts
- [ ] `./scripts/deploy-local.sh` — deploys `poker-table`, `zk-verifier`,
      `committee-registry` and writes `.env.local`

**Verify:** `.env.local` contains `POKER_TABLE_ID` and `ZK_VERIFIER_ID`.

## 7. Set up the MPC committee

- [ ] `./scripts/setup-dkg.sh` — generates the three nodes' TLS keys and party
      configs, funds their Stellar accounts, and registers them on-chain

For the production-shaped ceremony (node key split into Shamir shares across
committee members) see [mpc-key-enrollment.md](mpc-key-enrollment.md);
`setup-dkg.sh` is the local shortcut and keeps keys on one machine.

**Verify:** `services/node/config/local/party_0.toml` and
`services/node/data/key0.der` exist.

## 8. Start the MPC nodes

- [ ] `docker-compose up -d mpc-node-0 mpc-node-1 mpc-node-2`

**Verify:** all three answer `curl -s localhost:8101/health` (and `:8102`, `:8103`)
with `ok`.

## 9. Start the coordinator

- [ ] `docker-compose up -d coordinator`

The coordinator waits for Soroban and all three nodes to be healthy before it
starts (`depends_on: condition: service_healthy`), so a failure here usually
means step 5 or 8 is not actually green.

**Verify:** `curl -s localhost:8080/health` returns success.

## 10. Build and run the frontend

- [ ] `cd app && npm run dev`
- [ ] Open <http://localhost:3000>
- [ ] Install the [Freighter](https://freighter.app) wallet extension and point
      it at your local network

**Verify:** the lobby renders and the wallet connect button responds.

## 11. Run a test hand

- [ ] `python3 scripts/test-flow.py`

This drives a full hand end to end: deal → preflop → flop → turn → river →
showdown, with on-chain betting between MPC phases. It is the single best signal
that your whole stack is wired correctly.

**Verify:** the script exits 0 and prints a showdown winner.

---

## Before your first PR

- [ ] `cargo fmt --all && cargo clippy --all-targets`
- [ ] `cargo test -p poker-table`
- [ ] `cd circuits/lib && nargo test`
- [ ] `cd app && npm run lint && npm test`
- [ ] Read [CONTRIBUTING.md](../CONTRIBUTING.md)

## When something breaks

| Symptom | Look at |
|---|---|
| A service will not go healthy | [CONTRIBUTING.md](../CONTRIBUTING.md#diagnosing-unhealthy-services) |
| Coordinator errors or session failures | [coordinator-troubleshooting.md](coordinator-troubleshooting.md) |
| MPC committee / node registration | [committee-setup.md](committee-setup.md), [local-committee-dev-guide.md](local-committee-dev-guide.md) |
| Circuit compilation or proving | [NOIR_TESTING_GUIDE.md](NOIR_TESTING_GUIDE.md) |
| Wallet or frontend integration | [wallet-integration-testing.md](wallet-integration-testing.md) |
