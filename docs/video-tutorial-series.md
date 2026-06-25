# Building a Custom Game on StellPoker — Video Tutorial Series

A 5-part video series covering the complete workflow: from setting up a development environment to deploying a custom frontend.

---

## Part 1: Setting Up the Dev Environment

**Duration:** ~15 minutes  
**Prerequisites:** Basic familiarity with the command line and Git.

### Topics Covered

1. **Repository setup**
   - Clone the repository
   - Install system dependencies (Rust, Node.js 20+, Docker, Noirup)
   - Run `make setup` to bootstrap the environment

2. **Local devnet**
   - Start the stack with `docker compose up -d`
   - Verify all services: Soroban RPC, 3 MPC nodes, coordinator
   - Health checks: `curl http://localhost:8080/api/health`

3. **Frontend preview**
   - Install frontend dependencies: `cd app && npm ci`
   - Start Next.js dev server: `npm run dev`
   - Connect a Freighter wallet on testnet

4. **Running the test suite**
   - Run unit tests: `cd app && npm test`
   - Run integration flow: `python3 scripts/test-flow.py`

### Key Commands

```bash
git clone https://github.com/HitEmPoka/StellPoker.git
cd StellPoker
make setup
docker compose up -d
cd app && npm ci && npm run dev
```

### Links

- [Repository README](https://github.com/HitEmPoka/StellPoker#readme)
- [Docker Compose setup](https://github.com/HitEmPoka/StellPoker/blob/main/docker-compose.yml)
- [Soroban RPC documentation](docs/soroban-rpc-node.md)

---

## Part 2: Deploying Custom Contracts

**Duration:** ~20 minutes  
**Prerequisites:** Part 1 completed, Rust toolchain installed.

### Topics Covered

1. **Contract architecture overview**
   - `poker-table`: core game logic contract
   - `committee-registry`: MPC node registration
   - Custom contract templates

2. **Writing a custom contract**
   - Scaffold with `stellar contract init`
   - Implement a simple betting variant
   - Add custom state and access control

3. **Building and deploying**
   - Compile to WASM: `cd contracts && cargo build --target wasm32-unknown-unknown --release`
   - Deploy to local devnet: `stellar contract deploy`
   - Deploy to testnet: obtain test XLM from Friendbot, then deploy

4. **Verifying deployment**
   - Read contract state: `stellar contract invoke --id <ID> --fn state`
   - Write a quick test with `pytest`

### Key Commands

```bash
# Deploy to local devnet
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/poker_table.wasm \
  --rpc-url http://localhost:8000

# Deploy to testnet
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/poker_table.wasm \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"
```

### Links

- [Soroban contract development guide](https://developers.stellar.org/docs/soroban/contract-development)
- [poker-table contract source](contracts/poker-table/src/)

---

## Part 3: Writing a Noir Circuit

**Duration:** ~25 minutes  
**Prerequisites:** Part 1 completed, familiarity with basic ZK concepts.

### Topics Covered

1. **Noir language basics**
   - Data types, functions, and constraints
   - Public vs. private inputs
   - The `main` function and its return value

2. **Existing StellPoker circuits**
   - `deal`: verifies that a dealt hand is a valid subset of a shuffled deck
   - `reveal`: proves a card was committed during the deal
   - `showdown`: proves the winner has the best hand
   - `hand_rank`: evaluates a 5-card poker hand

3. **Writing a custom circuit**
   - Define the circuit's goal (e.g., verify a custom scoring rule)
   - Write constraints using Noir's standard library
   - Test with `nargo test`

4. **Compiling and generating proofs**
   - Compile: `nargo compile`
   - Generate a proof: `nargo prove`
   - Verify: `nargo verify`

### Key Commands

```bash
cd circuits/lib
nargo test
nargo compile
nargo prove -p <proof-name>
nargo verify -p <proof-name>
```

### Resources

- [Noir language documentation](https://noir-lang.org/docs)
- [Noir standard library](https://noir-lang.org/docs/standard-library)
- [StellPoker circuit source](circuits/lib/src/)

---

## Part 4: Integrating with the Coordinator

**Duration:** ~20 minutes  
**Prerequisites:** Parts 1–3 completed, a deployed contract and compiled circuit.

### Topics Covered

1. **Coordinator API overview**
   - `POST /api/create-table`
   - `POST /api/join-table`
   - `POST /api/request-deal`
   - `POST /api/player-action`
   - `POST /api/request-reveal`
   - `POST /api/request-showdown`

2. **Registering a custom contract**
   - Point the coordinator to your contract address
   - Configure the contract ABI in `coordinator.toml`

3. **Registering a custom circuit**
   - Upload circuit artifacts to the coordinator
   - Map circuit to game phase in configuration

4. **Testing the integration**
   - Run the full flow with `python3 scripts/test-flow.py`
   - Monitor logs: `docker compose logs coordinator -f`
   - Inspect proofs via the coordinator's proof explorer endpoint

### Key API Call

```bash
curl -X POST http://localhost:8080/api/create-table \
  -H "Content-Type: application/json" \
  -d '{
    "wallet_address": "G...",
    "signature": "0x...",
    "max_players": 4,
    "solo": false,
    "buy_in": 10
  }'
```

### Links

- [Coordinator API reference](docs/api/README.md)
- [OpenAPI specification](docs/api/openapi.yaml)
- [API conformance tests](scripts/check-api-spec.py)

---

## Part 5: Building a Custom Frontend

**Duration:** ~25 minutes  
**Prerequisites:** Parts 1–4 completed, familiarity with React/Next.js.

### Topics Covered

1. **Frontend architecture**
   - Next.js App Router structure
   - Component tree: Table, ActionPanel, Board, PlayerSeat, ProofExplorer
   - State management via React hooks (`usePokerActions`)

2. **Wallet integration**
   - Detect Freighter / Lobstr
   - Connect and sign messages
   - Silent reconnect from LocalStorage

3. **Building a custom UI**
   - Fork the existing components
   - Add a new game mode or variant
   - Styling with Tailwind CSS

4. **Integrating with the coordinator**
   - Use the `api.ts` client to call coordinator endpoints
   - Sync state via polling and Soroban event subscriptions

5. **Deploying the frontend**
   - Build: `cd app && npm run build`
   - Deploy to Vercel, Docker, or a static host
   - Configure environment variables

### Key Files

- `app/src/app/page.tsx` — main game page
- `app/src/components/Table.tsx` — poker table orchestration
- `app/src/lib/api.ts` — coordinator API client
- `app/src/lib/onchain.ts` — Soroban transaction builder
- `app/src/lib/wallet.ts` — wallet abstraction layer

### Deployment Quick Start

```bash
cd app
npm run build
docker build -t stellpoker-frontend .
docker run -p 3000:3000 \
  -e NEXT_PUBLIC_COORDINATOR_URL=http://your-coordinator:8080 \
  stellpoker-frontend
```

### Links

- [Frontend state management documentation](docs/frontend-state-management.md)
- [Next.js deployment guide](https://nextjs.org/docs/app/building-your-application/deploying)
- [Component source code](app/src/components/)

---

## Series Production Notes

### Target Audience

Developers with basic blockchain and TypeScript knowledge who want to build custom games on top of StellPoker.

### Format

- **Length:** 10–25 minutes per episode
- **Style:** Live coding with voiceover
- **Output:** 1080p, 30 fps
- **Platform:** YouTube (unlisted during review)

### Repository Companion

Each part has a corresponding branch with the starting state and a branch with the completed state:
- `tutorial/part-1-start` / `tutorial/part-1-end`
- `tutorial/part-2-start` / `tutorial/part-2-end`
- etc.

### Checklist

- [ ] Script reviewed and approved
- [ ] Screen recordings cleaned of sensitive data
- [ ] Key commands and links verified against current `main`
- [ ] Closed captions added
- [ ] Description includes timestamps and links to docs
- [ ] Thumbnail created
