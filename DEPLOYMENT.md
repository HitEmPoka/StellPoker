# StellPoker Production Deployment Runbook

This guide covers the end-to-end process for deploying StellPoker to a production environment.

## 1. Infrastructure Prerequisites
- **PostgreSQL Database**: Minimum version 14. Used for coordinator state persistence.
- **MPC Nodes**: 3 independent servers/containers for the REP3 MPC protocol.
- **Coordinator Server**: Minimum 2 CPUs, 4GB RAM. Must have access to the MPC nodes and PostgreSQL.
- **Soroban RPC Provider**: Access to a reliable Stellar network RPC node.
- **Frontend Hosting**: Vercel, Netlify, or any static/Node.js hosting provider for the Next.js app.

## 2. Contract Deployment
1. Navigate to the `contracts/` directory.
2. Ensure you have the `stellar-cli` installed and configured with your production account.
3. Build the smart contracts:
   ```bash
   stellar contract build
   ```
4. Deploy the `poker-table` contract to the network:
   ```bash
   stellar contract deploy --wasm target/wasm32-unknown-unknown/release/poker_table.wasm --source <production-account> --network <mainnet-or-testnet>
   ```
5. Note the deployed contract ID. You will need it for the coordinator configuration.

## 3. MPC Bootstrapping
1. Compile the Noir circuits on a build machine:
   ```bash
   cd circuits && nargo compile
   ```
2. Generate or obtain the production CRS (Common Reference String).
3. Distribute the compiled `.json` artifacts and CRS to all 3 MPC nodes.
4. Start the 3 MPC nodes on their respective infrastructure. Note their URLs (e.g., `http://mpc-node-0.internal:8101`).

## 4. Coordinator Configuration & Deployment
1. Set up the production database.
2. Create an `.env` file for the Coordinator (refer to `CONFIGURATION.md` for all variables):
   ```env
   DATABASE_URL=postgres://user:password@db-host:5432/coordinator
   MPC_NODE_0=http://mpc-node-0.internal:8101
   MPC_NODE_1=http://mpc-node-1.internal:8102
   MPC_NODE_2=http://mpc-node-2.internal:8103
   POKER_TABLE_CONTRACT=<Deployed-Contract-ID>
   NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
   COMMITTEE_SECRET=<Production-Stellar-Secret>
   ```
3. Run the coordinator service. On startup, it will apply pending database migrations automatically.

## 5. Frontend Build and Deploy
1. In the `app/` (or frontend) directory, configure the environment variables:
   ```env
   NEXT_PUBLIC_COORDINATOR_URL=https://api.stellpoker.com
   ```
2. Build the Next.js application:
   ```bash
   npm run build
   ```
3. Deploy the build artifacts to your hosting provider.

## 6. Monitoring
- **Coordinator Logs**: Monitor standard output for `ERROR` and `WARN` logs.
- **MPC Nodes**: Ensure all 3 nodes remain online. The protocol will halt if any node goes down.
- **Database**: Monitor connection counts and disk space.
- **Contract Events**: Monitor the Soroban RPC for failed transactions or unexpected state changes.

## 7. Rollback Procedure
If a deployment fails or introduces critical bugs:
1. **Frontend**: Revert the deployment to the previous commit via your hosting provider's dashboard.
2. **Coordinator**: Stop the service, checkout the previous release tag, and restart. (Note: Database rollbacks require manual intervention and restoring from a backup).
3. **Contracts**: Contracts are typically immutable or have specific upgrade paths. Follow the `MAINNET_MIGRATION.md` for safe contract upgrades/rollbacks if implemented.

## 8. Container Image Signing and Verification (cosign)

CI signs every GHCR image keylessly with [cosign](https://docs.sigstore.dev/) in
`.github/workflows/docker-build-push.yml`. The signing certificate identity is
this workflow running on `main`, so only images built by CI verify.

### Verify before deploy

```bash
./scripts/verify-image-signature.sh ghcr.io/<org>/stellpoker/coordinator:<tag>
```

Run it for every image referenced by the Helm charts before `helm install` /
`helm upgrade` (also documented in `infrastructure/helm/README.md`).

### Failure mode (documented): unsigned images are refused

- The verify script is **fail-closed**: any image that is unsigned, signed by a
  different workflow/repo, or unverifiable causes a non-zero exit.
- On failure the deploy **must abort** — do not bypass with `|| true`, an
  insecure flag, or by skipping the check.
- To remediate, re-push the image through CI so it is signed, then retry.
