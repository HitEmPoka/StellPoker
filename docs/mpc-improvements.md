# MPC Improvements

## Issue #94: MPC Node Heartbeats and Failure Detection

### Implementation
- MPC nodes send periodic heartbeats to coordinator (default: 10s)
- Coordinator tracks consecutive failures per node
- Nodes marked unhealthy after N missed heartbeats (default: 3)
- Committee failover triggered when quorum lost

### Configuration
```bash
MPC_HEARTBEAT_INTERVAL_SECS=10
MPC_MAX_MISSED_HEARTBEATS=3
MPC_STARTUP_GRACE_PERIOD_SECS=30
```

### API Endpoints
- `POST /api/mpc/heartbeat/:node_id` - Node sends heartbeat
- `GET /api/mpc/health` - Get all node health status

## Issue #93: TLS Encryption

### Development Setup
Generate self-signed certificates:
```bash
./scripts/generate-tls-certs.sh ./certs
```

### Coordinator Configuration
```bash
MPC_TLS_ENABLED=true
MPC_CLIENT_CERT_PATH=./certs/coordinator-cert.pem
MPC_CLIENT_KEY_PATH=./certs/coordinator-key.pem
MPC_CA_CERT_PATH=./certs/ca-cert.pem
```

### MPC Node Configuration
```bash
TLS_SERVER_CERT_PATH=./certs/node0-cert.pem
TLS_SERVER_KEY_PATH=./certs/node0-key.pem
COORDINATOR_TLS_PIN_CERT_PATH=./certs/coordinator-cert.pem
```

### Production Setup
1. Obtain CA-signed certificates for all nodes and coordinator
2. Set TLS_MIN_VERSION=1.3 (enforced by default)
3. Use certificate pinning for coordinator auth
4. Rotate certificates before expiry

## Issue #92: CRS Download Optimization

### Features
- Automatic CRS download before first compilation
- SHA-256 hash verification
- Skip re-download if hash matches
- Progress bar during download

### Usage
```bash
./scripts/download-crs.sh  # Manual download
./scripts/compile-circuits.sh  # Auto-downloads if needed
```

### Configuration
```bash
CRS_DIR=./.crs  # Custom CRS directory
```

## Issue #91: Recursive Proof Aggregation

### Circuit
New `tournament_aggregator` circuit verifies multiple hand proofs and computes tournament winner.

### Compilation
```bash
cd circuits/tournament_aggregator
nargo compile
```

### Usage
The circuit accepts:
- `hand_proofs`: Array of hand proof hashes
- `hand_winners`: Winner of each hand
- `num_hands`: Number of hands in tournament
- `tournament_winner`: Final tournament winner (public output)

### Integration
Future coordinator endpoint will aggregate proofs for multi-table tournaments.

## Issue #246: Peer Connection Pooling

Each REP3 session makes a node talk to both peers (share dispatch, proactive
share refresh, heartbeats). Every call site used to construct its own
`reqwest::Client`, and a client owns its connection pool — so each request paid
a fresh TCP handshake (plus a full TLS handshake for `https://` peers) and then
dropped the connection.

`services/node/src/pool.rs` keeps one process-wide client whose keep-alive pool
is shared by every session, so only the first request to a peer pays handshake
cost. A background task probes `GET {endpoint}/health` on each peer and records
the result; `pool::is_healthy(endpoint)` exposes it so callers can skip a known
dead peer instead of discovering it through a mid-session timeout. An endpoint
that has never been probed reports healthy, so no peer is taken out of rotation
at startup before the first probe runs.

### Configuration

```bash
MPC_POOL_MAX_IDLE_PER_HOST=8      # idle connections kept per peer
MPC_POOL_IDLE_TIMEOUT_SECS=90     # idle connection eviction
MPC_POOL_TCP_KEEPALIVE_SECS=30    # TCP keep-alive probe interval
MPC_POOL_HEALTH_INTERVAL_SECS=15  # peer health check period
MPC_POOL_HEALTH_TIMEOUT_SECS=5    # per-probe timeout
```
