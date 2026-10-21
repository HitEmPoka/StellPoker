# End-to-End Encryption Flow

This document describes how encryption and authentication are applied across StellPoker's architecture.
It covers external TLS, internal coordinator-to-node transport, Soroban transaction signing, proof confidentiality, and wallet-based signing.

## Components

- `Browser / Frontend` — Next.js UI and wallet integrations (Freighter, Lobstr)
- `Coordinator` — Axum service that orchestrates MPC sessions and submits on-chain transactions
- `MPC nodes` — Axum-based MPC participants running TACEO coNoir proof generation
- `Soroban RPC` — Stellar RPC endpoint used for contract invocation and transaction submission

## 1. External TLS: Browser / HTTP Clients

- Every user-facing web request must use HTTPS/TLS.
- In production, the frontend and coordinator should be behind TLS-terminating proxies or ingress controllers.
- Browser clients communicate with the coordinator over a TLS-protected REST API.
- This protects session tokens, wallet authentication headers, and any data passed between browser and coordinator.

### What is encrypted

- HTTP request/response bodies
- API headers used for wallet authentication
- Cookies and local storage are protected only in the browser; secrets must never be stored there

### What is authenticated

- TLS authenticates the coordinator endpoint to the client.
- Browser wallets verify the origin before signing messages.

## 2. Coordinator ↔ MPC Nodes: Internal Transport

- MPC nodes expose Axum HTTP endpoints such as:
  - `POST /table/:id/prepare-deal`
  - `POST /table/:id/dispatch-shares`
  - `GET /session/:id/proof`
- The coordinator currently uses `reqwest::Client` to call these endpoints.
- Local development often uses plaintext URLs such as `http://localhost:8101`.

### Recommended production setup

- Use mTLS between coordinator and MPC nodes.
- Each coordinator and node should present a certificate and verify the peer certificate.
- This provides both encryption and strong service authentication.
- If mTLS is not available, deploy these services inside a private network or service mesh that ensures encrypted internal traffic.

### Why this matters

- MPC shares and proof preparation payloads contain sensitive secret-share data.
- Transport-layer encryption prevents eavesdropping or tampering of node orchestration traffic.

## 3. Coordinator → Soroban RPC

- The coordinator submits generated proofs and contract actions to a Soroban RPC endpoint.
- In production, this endpoint must be accessed over HTTPS.
- The coordinator may interact with a local Soroban proxy or a trusted external RPC node.

### What is signed

- On-chain transactions are signed by a wallet or configured identity.
- The coordinator does not hold a user's wallet key material for browser-authenticated players.
- For local or bot identities, the coordinator may use configured Soroban source identities.

## 4. Proof Confidentiality and Encryption

- The MPC protocol uses REP3 secret sharing to keep private card values and randomness confidential.
- Each MPC node holds only its own secret contribution, never the full deck.
- UltraHonk proofs are generated from these secret-shared inputs.

### Important distinction

- The proof itself is not a ciphertext that must remain secret.
- Proofs are public evidence of correct deal, reveal, or showdown execution.
- Confidentiality is achieved by secret sharing, not by encrypting the proof object.

### Sensitive data boundaries

- Private cards and random salts only exist as secret shares inside MPC nodes.
- Prepared share payloads exchanged between coordinator and MPC nodes should be protected in transit.
- Public outputs (proofs, table state, winner result) may be visible to the coordinator and Soroban.

## 5. Wallet Signing

### Wallet authentication for coordinator API

- The frontend uses wallet message signing to authenticate player actions.
- `buildAuthMessage(...)` creates a message like:

  `stellar-poker|{address}|{tableId}|{action}|{nonce}|{timestamp}`

- The signature is sent to the coordinator in HTTP headers:
  - `x-player-address`
  - `x-auth-signature`
  - `x-auth-nonce`
  - `x-auth-timestamp`

- The wallet private key never leaves the browser wallet.
- Supported wallets include Freighter and Lobstr.

### On-chain transaction signing

- On-chain actions use wallet transaction signing rather than message signing.
- For Lobstr, the app calls `window.lobstr.signTransaction(txXdr, opts)`.
- For Freighter, the app calls `signTransaction(txXdr, opts)` from `@stellar/freighter-api`.
- The wallet returns a signed XDR string that is forwarded to Soroban RPC.

### Why this matters

- Wallet signing ensures the player authorizes any table join, bet, reveal, or settlement transaction.
- Signatures prove ownership of the Stellar account and prevent unauthorized on-chain actions.

## 6. Deployment Recommendations

- Serve the frontend and coordinator over HTTPS.
- Protect internal coordinator-to-node traffic with mTLS and private networking.
- Use a trusted Soroban RPC endpoint over HTTPS.
- Keep wallet signing operations confined to the browser wallet.
- Treat MPC share payloads and node orchestration traffic as confidential.

## 7. Witness Buffer Zeroization

After coNoir proof generation, witness inputs (permutations, salts, TOML share buffers) must be
cleared from heap memory to prevent secret material from lingering in freed pages.

### Policy

- `PartyContribution` in `services/node/src/private_table.rs` derives `ZeroizeOnDrop`.  Its
  `permutation` (`Vec<u32>`) and `salts` (`Vec<String>`) fields are zeroed automatically whenever
  the struct is dropped — including when a new deal replaces the prior contribution or when the
  table entry is removed from `PrivateTableState`.
- All serialized TOML buffers (`input_toml`) produced by `build_*_partial_toml` are wrapped in
  `zeroize::Zeroizing<String>` inside `prepare_deal`, `prepare_reveal`, and `prepare_showdown`.
  This ensures the plaintext permutation/salt TOML string is zeroed when it goes out of scope
  (immediately after the `split_partial_input` subprocess writes it to a temp file).
- File-backed buffers (the `Prover.toml` share file written to disk by `co-noir
  merge-input-shares`) are not addressed by in-process zeroization. The work directory should be
  placed on a `tmpfs` or encrypted volume and cleaned up by the caller after proof generation.

### What is covered

| Buffer | Zeroization mechanism |
|---|---|
| `PartyContribution.permutation` | `ZeroizeOnDrop` via derive |
| `PartyContribution.salts` | `ZeroizeOnDrop` via derive |
| `input_toml` (deal/reveal/showdown) | `Zeroizing<String>` wrapper, dropped after subprocess |

## 8. Cross-Layer Summary

| Layer | Protection | Notes |
|---|---|---|
| Browser → Coordinator | TLS | Encrypts API calls and auth headers |
| Coordinator → MPC nodes | mTLS recommended | Protects secret share orchestration |
| Coordinator → Soroban RPC | HTTPS | Protects signed transaction submission |
| Wallet auth | Message signature | Authenticates player actions to coordinator |
| Wallet transaction sign | Signed XDR | Authorizes on-chain contract calls |
| Proofs | Public ZK proofs over secret shares | Confidentiality via MPC secret sharing |
| Witness buffers | `ZeroizeOnDrop` + `Zeroizing<String>` | Cleared from heap after proving |
