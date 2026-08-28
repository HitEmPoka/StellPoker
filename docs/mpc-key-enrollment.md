# MPC Node Key Enrollment Ceremony

`mpc-enroll` runs the one-off ceremony that brings a new MPC node into the
committee. It generates the node's identity keypair, splits the secret into
Shamir shares so no single participant holds a recoverable copy, and registers
the node with the `committee-registry` contract.

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

The split is Shamir over GF(256), applied byte-wise to the 32-byte ed25519 seed,
so each share is the same size as the seed.

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
| `enrollment.json` | node id, public key, endpoint, region, `t`/`n` | no |

The plaintext seed is never written to disk — it is zeroized after the split.
The public key is printed; that is the `member` address the registry will know.

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

## 3. Verify a quorum can restore the key

Before destroying anything, confirm the shares actually reconstruct. Collect any
`threshold` share files and run:

```bash
cargo run -p mpc-node --bin mpc-enroll -- combine \
  --out-dir ./ceremony \
  --share ./ceremony/share_0.json \
  --share ./ceremony/share_1.json
```

This reconstructs the seed, re-derives the public key, and fails loudly if it
does not match `enrollment.json`. On success it prints the `S...` secret key —
run it only on the machine that is meant to hold the key, or on the offline
ceremony machine as a rehearsal.

Shares from a different ceremony are rejected by public-key mismatch, and
duplicate share indices are rejected outright.

## 4. Register on-chain

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
rather than running it, so the transaction can be reviewed before submission.
Pass `--execute` to submit it.

`--stake` must be at least the registry's configured minimum, and the source
account must already hold that stake in the registry's stake token — the
contract transfers it on registration.

After registration the admin still has to pack the member into an active epoch
(`create_epoch`); see [committee-setup.md](committee-setup.md).

## Recovering a node

Restoring a failed node is step 3 with a real quorum: collect `threshold` share
files from their holders, run `combine`, and load the printed `S...` key into
the replacement node. The registry entry, endpoint, and stake are unchanged, so
no re-registration is needed unless the endpoint moved.
