# Wallet authorization circuit design (Issue #891)

## Threat model

An attacker who observes a valid action proof must not be able to:

1. submit it for a different action, table or hand;
2. submit it repeatedly for the same action;
3. submit it after the player's authorization window closes.

The ZK circuit cannot by itself bind a public key to a seat — that binding is
part of the table's committed player state. The gadget proves the signature is
valid for the key supplied to the circuit; the caller must constrain that key.

## Signed message

```
digest = Blake2s-256(
    "StellPoker/action/v1" ||
    be32(chain_id) ||
    be32(table_id) ||
    be32(hand_commitment) ||
    be32(action) ||
    be32(amount) ||
    be32(nonce) ||
    be32(expiry)
)
```

All fields are encoded as fixed 32-byte big-endian values. This avoids field
ambiguity and matches the on-chain field representation used by the verifier.

`std::ecdsa_secp256r1::verify_signature(digest, pub_x, pub_y, sig_64)` is called
inside the circuit. The public inputs of `action_order` include `chain_id`,
`table_id`, `hand_commitment`, `action`, `amount`, `nonce`, `expiry` and `now`
so a verifier can check the signed context without revealing the wallet key;
`pub_x`, `pub_y` and `signature` remain private witnesses.

## Replay protection

- **Nonce**: unique per `(table, hand, action)`. `nonce == 0` is rejected.
  The caller/contract tracks used nonces and rejects duplicates.
- **Expiry**: the circuit requires `now <= expiry`. `now` is a public
  field supplied from the proof so a relayer cannot move the deadline forward.
- **Context binding**: table, hand commitment, action and amount are hashed
  into the digest. A signature for one context is useless in another.

## Constraint overhead

The gadget adds one 244-byte Blake2s compression and one P-256 signature
verification per action proof. `action_order` is included in
`scripts/circuit_constraints.py` and `circuits/constraint-budgets.json`; the
Circuit Benchmarks workflow publishes ACIR and backend gate counts with the
baseline delta so the overhead is measured on every change.

## Testing

Negative tests in `circuits/lib/src/wallet_auth.nr` and
`circuits/action_order/src/main.nr` assert that zero signatures, expired
authorizations and zero nonces are rejected. A positive test requires a real
P-256 key/signature fixture generated off-circuit; the on-chain integration
suite covers that path with a wallet-signed action.
