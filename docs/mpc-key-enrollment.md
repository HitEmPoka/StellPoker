# MPC Node Key Enrollment Ceremony

`mpc-enroll` runs the one-off ceremony that brings a new MPC node into the
committee. It generates the node's identity keypair, splits the secret into
Feldman-verifiable shares so no single participant holds a recoverable copy and
every holder can check their share, and registers the node with the
`committee-registry` contract along with a hash of the share commitments.

```bash
cargo run -p mpc-node --bin mpc-enroll -- --help
```

For purely local development, [`scripts/setup-dkg.sh`](../scripts/setup-dkg.sh)
is the shortcut: it generates all three nodes' keys on one machine. That is fine
for a laptop and wrong for anything real, which is what this tool is for.

## What is being protected, and what is not

[committee-setup.md](committee-setup.md) explains that coNoir REP3 needs no
threshold key pair: proving happens over replicated shares of the *inputs* (the
deck, the player salts), not under a shared signing key. There is therefore no
DKG to run.

What does need protecting is the node's own long-lived **identity key** — the
Stellar account that signs committee transactions and that `register_member`
authorises on-chain. Generated the ordinary way, that key is both a single point
of compromise (one operator's disk) and a single point of loss (that same disk).
Splitting it `t`-of-`n` across committee members fixes both: fewer than `t`
members learn nothing about the key, and any `t` can restore a node after a
hardware failure.

### Verifiable shares

Plain Shamir sharing cannot tell a holder whether the share they were handed is
genuine: a faulty dealer can issue shares that do not lie on one polynomial, and
nobody finds out until a quorum tries to reconstruct. The ceremony therefore uses
Feldman's verifiable scheme over the Ristretto255 group
(`services/node/src/crypto/feldman.rs`):

- The ceremony shares one random scalar `s`. The node's ed25519 seed is
  `SHA-256("stellpoker/node-seed/v1" || s)`, so the key is recoverable from any
  `t` shares but `s` itself is never stored. (The seed is not shared directly
  because Feldman publishes `s·G`, which is only safe for a full-entropy scalar.)
- The dealer publishes a commitment `C_k = a_k·G` to every coefficient of the
  sharing polynomial. A share `(x, y)` is genuine exactly when
  `y·G = Σ x^k · C_k`, and checking that reveals nothing about other shares.
- The commitments, the node id, the member's public key and the share count are
  bound into one **transcript hash** (SHA-256). That hash is what goes on-chain.

Each share is still 32 bytes. Ceremonies enrolled before this change used
byte-wise Shamir over GF(256); their share files carry no `scheme` field,
`combine` still restores them, and they have no commitments to verify against.

## 1. Generate

Run on an offline machine.

```bash
cargo run -p mpc-node --bin mpc-enroll -- generate \
  --node-id 0 \
  --threshold 2 --shares 3 \
  --endpoint http://mpc-node-0:8101 \
  --region us-east \
  --out-dir ./ceremony
```

Produces, in `./ceremony` (gitignored):

| File | Contents | Secret? |
|---|---|---|
| `share_0.json` … `share_N.json` | one member's share, `0600` on unix | **yes** |
| `commitments.json` | Feldman commitments `C_0 … C_{t-1}` and the transcript hash | no |
| `enrollment.json` | node id, public key, endpoint, region, `t`/`n`, transcript hash | no |

The plaintext seed is never written to disk — it is zeroized after the split.
The public key and the commitment transcript hash are printed; the public key is
the `member` address the registry will know. `generate` checks every share
against the commitments before it writes anything.

Pick `threshold` so that losing one member does not lose the key and gaining one
member does not gain it: with a 3-member committee, `--threshold 2 --shares 3`.

## 2. Distribute

Hand each `share_N.json` to exactly one committee member over an out-of-band,
authenticated channel, then delete the share files from the ceremony machine.

The tool deliberately does **not** push shares over the network. At enrollment
time there is no authenticated channel to the members yet — establishing one is
what enrollment is *for* — so an automatic push would mean trusting an
unauthenticated endpoint with precisely the material the ceremony exists to
protect.

## 3. Verify your share

Publish `commitments.json` alongside the ceremony (it contains no secret) and
have every holder check their own share against it:

```bash
cargo run -p mpc-node --bin mpc-enroll -- verify \
  --out-dir ./ceremony \
  --share ./ceremony/share_0.json
```

This confirms the commitments are internally consistent (they match the
transcript hash in `enrollment.json`) and that the share lies on the committed
polynomial. It reconstructs nothing and prints no secret, so it is safe to run on
the holder's own machine. An invalid share is reported by index and the command
exits non-zero. With no `--share` it checks the commitments alone.

`verify` refuses a ceremony from before verifiable sharing, which has no
commitments.

## 4. Verify a quorum can restore the key

Before destroying anything, confirm the shares actually reconstruct. Collect any
`threshold` share files and run:

```bash
cargo run -p mpc-node --bin mpc-enroll -- combine \
  --out-dir ./ceremony \
  --share ./ceremony/share_0.json \
  --share ./ceremony/share_1.json
```

This checks every share against `commitments.json` first and names any share that
does not match, then reconstructs the seed, re-derives the public key, and fails
loudly if it does not match `enrollment.json`. On success it prints the `S...` secret key —
run it only on the machine that is meant to hold the key, or on the offline
ceremony machine as a rehearsal.

Shares from a different ceremony are rejected by public-key mismatch, and
duplicate share indices are rejected outright.

## 5. Register on-chain

```bash
cargo run -p mpc-node --bin mpc-enroll -- register \
  --out-dir ./ceremony \
  --registry "$COMMITTEE_REGISTRY_ID" \
  --source node0 \
  --network testnet \
  --stake 1000000000 \
  --fee_rate_bps 0
```

By default this prints the `stellar contract invoke … register_member` command
and the follow-up `set_commitment_hash` command rather than running them, so the
transactions can be reviewed before submission. Pass `--execute` to submit both.
`register` refuses to run if `commitments.json` has been altered since `generate`.

`set_commitment_hash` stores the transcript hash on the registry, where anyone
can read it back:

```bash
stellar contract invoke --id "$COMMITTEE_REGISTRY_ID" --network testnet -- \
  get_commitment_hash --member G...
```

The hash can be set once per registration. `deregister_member` and a fresh
`register_member` both clear it, so re-enrolling a node means running a new
ceremony and recording its new hash.

### Checking at session start

`mpc_node::crypto::feldman::verify_before_deal` is the gate a node uses before
any hand is dealt: it recomputes the transcript hash from the commitments it
holds, refuses to continue unless that equals the on-chain value, and then
checks each share against the commitments. The transcript is checked first, so
forged commitments are never used to "verify" a forged share. **Wiring this into
the node's session start is not done yet**: today the check is available as a
library function and, for holders, as `mpc-enroll verify`.

`--stake` must be at least the registry's configured minimum, and the source
account must already hold that stake in the registry's stake token — the
contract transfers it on registration.

After registration the admin still has to pack the member into an active epoch
(`create_epoch`); see [committee-setup.md](committee-setup.md).

## Recovering a node

Restoring a failed node is step 4 with a real quorum: collect `threshold` share
files from their holders, run `combine`, and load the printed `S...` key into
the replacement node. The registry entry, endpoint, and stake are unchanged, so
no re-registration is needed unless the endpoint moved.
