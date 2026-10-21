# Frequently Asked Questions

This page covers the most common issues when using the StellPoker frontend and
coordinator. For local development setup, see the
[developer onboarding guide](developer-onboarding.md).

## My wallet does not connect

Confirm that the wallet extension is installed, unlocked, and set to the same
network as the application. Refresh the page after switching networks. If the
wallet is already connected but the app still shows “Connect wallet”, remove
the site connection from the wallet and connect again. Browser privacy
extensions can also block wallet providers; try a clean browser profile.

## A transaction failed

Check that the account has enough native token for fees and enough table token
for the requested buy-in or action. Verify the network and contract addresses
loaded by the app. A rejected transaction should be retried only after reading
the wallet error: an authorization rejection is different from a contract
validation failure. For local development, inspect the coordinator and Soroban
logs and confirm that the local deployment has produced a current `.env.local`.

## The MPC committee is unavailable

The coordinator requires a healthy committee before it can create or complete
proof sessions. Check `GET /api/health` and
`GET /api/committee/status`. In the local Docker stack, wait for all MPC node
health checks to become healthy, then inspect `docker-compose logs mpc-node-0`
through `mpc-node-2`. Ensure the nodes can load the CRS and that ports
8101–8103 and 10000–10002 are not already in use.

## Why did proof verification fail?

Do not reuse a proof or submit it for a different table, hand, phase, or
session. Proofs are bound to the public inputs and the coordinator tracks the
session that produced them. Verify that all required players submitted the
expected commitments and that the coordinator, MPC nodes, circuits, and
contracts were built from compatible versions. A failed verification is not a
wallet failure; retrying the same stale proof will not fix it.

## Does ZK/MPC reveal my cards or private inputs?

Zero-knowledge proofs allow the verifier to check the required statement
without publishing the private witness, such as a player’s hidden cards.
MPC distributes computation across the committee so no single node should see
the complete private input. Public game data—actions, table state, proof
results, and values intentionally included as public inputs—can still be
observed. ZK and MPC therefore reduce private-input disclosure; they do not
make the entire application or network anonymous.

## Where can I find more diagnostics?

Start with `/api/health`, the coordinator logs, and the relevant MPC node
logs. When reporting a problem, include the table/session identifier, endpoint,
network, transaction hash (if any), and the exact error without sharing wallet
secrets or private card data.
