# StellPoker API Examples

This document provides code examples for common API operations in curl, Python, and TypeScript.

## Prerequisites

### Python Dependencies
```bash
pip install requests pynacl redis
```

### TypeScript Dependencies
```bash
npm install @stellar/stellar-sdk
```

### Environment Setup

Set the coordinator API base URL:
```bash
# Development
export COORDINATOR_URL="http://localhost:8080"

# Testnet
export COORDINATOR_URL="https://testnet-coordinator.stellpoker.com"

# Mainnet
export COORDINATOR_URL="https://coordinator.stellpoker.com"
```

### Network Configuration

The coordinator works with different Stellar networks:

| Network | RPC URL | Network Passphrase |
|---------|---------|-------------------|
| Local | `http://localhost:8000/soroban/rpc` | `Standalone Network ; February 2017` |
| Testnet | `https://soroban-testnet.stellar.org` | `Test SDF Network ; September 2015` |
| Mainnet | `https://horizon.stellar.org` | `Public Global Stellar Network ; September 2015` |

## Base URL

All examples use the coordinator API base URL:
- Development: `http://localhost:8080`
- Production: Set via environment variable

## Authentication

Most endpoints require authentication using Stellar signature headers. The auth message format is:
```
stellar-poker|{address}|{table_id}|{action}|{nonce}|{timestamp}
```

Required headers:
- `x-player-address`: Your Stellar address (G... format)
- `x-auth-signature`: Signature of the auth message (hex-encoded)
- `x-auth-nonce`: Incrementing nonce value (string)
- `x-auth-timestamp`: Unix timestamp in seconds (string)

### Key Generation and Address Encoding

#### Python
```python
import struct
import base64
from nacl.signing import SigningKey

def encode_stellar_pubkey(raw_32: bytes) -> str:
    """Encode raw ed25519 public key as Stellar G... address."""
    payload = bytes([6 << 3]) + raw_32
    crc = _crc16_xmodem(payload)
    full = payload + struct.pack("<H", crc)
    return _base32_encode(full)

def _crc16_xmodem(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            if crc & 0x8000:
                crc = (crc << 1) ^ 0x1021
            else:
                crc <<= 1
            crc &= 0xFFFF
    return crc

def _base32_encode(data: bytes) -> str:
    return base64.b32encode(data).decode("ascii").rstrip("=")

# Generate keypair and get address
sk = SigningKey.generate()
pk = bytes(sk.verify_key)
address = encode_stellar_pubkey(pk)
print(f"Address: {address}")  # GABCD...
```

#### TypeScript
```typescript
import { Keypair } from '@stellar/stellar-sdk';

// For Node.js environments, Buffer is available globally
// For browser environments, install a polyfill:
// npm install buffer
// Then add: import { Buffer } from 'buffer';

// Generate keypair and get address
const keypair = Keypair.random();
const address = keypair.publicKey();
console.log(`Address: ${address}`); // GABCD...

// Sign a message
const message = "test message";
const signature = keypair.sign(Buffer.from(message)).toString('hex');
```

### Key Management

#### Loading Keys from Environment (Python)
```python
import os
from nacl.signing import SigningKey

# Load secret key from environment (hex-encoded)
secret_key_hex = os.environ.get("STELLAR_SECRET_KEY")
if secret_key_hex:
    sk = SigningKey(bytes.fromhex(secret_key_hex))
else:
    # Generate new key for development
    sk = SigningKey.generate()
    print(f"Generated key. Save this: {sk.encode().hex()}")

pk = bytes(sk.verify_key)
address = encode_stellar_pubkey(pk)
```

#### Loading Keys from Environment (TypeScript)
```typescript
import { Keypair } from '@stellar/stellar-sdk';

// Load secret key from environment
const secretKey = process.env.STELLAR_SECRET_KEY;
let keypair: Keypair;

if (secretKey) {
  keypair = Keypair.fromSecret(secretKey);
} else {
  // Generate new key for development
  keypair = Keypair.random();
  console.log(`Generated key. Save this: ${keypair.secret()}`);
}

const address = keypair.publicKey();
```

---

## Helper Functions

### Card Decoding

#### Python
```python
SUITS = ["Spades", "Hearts", "Diamonds", "Clubs"]
RANKS = ["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"]

def decode_card(value: int) -> str:
    """Convert card integer (0-51) to readable format."""
    suit = SUITS[value // 13]
    rank = RANKS[value % 13]
    return f"{rank} of {suit}"

# Example
print(decode_card(10))  # "Queen of Spades"
```

#### TypeScript
```typescript
const SUITS = ["clubs", "diamonds", "hearts", "spades"] as const;
const RANKS = ["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"] as const;

const SUIT_SYMBOLS: Record<typeof SUITS[number], string> = {
  clubs: "♣",
  diamonds: "♦",
  hearts: "♥",
  spades: "♠",
};

function decodeCard(value: number): string {
  const suit = SUITS[Math.floor(value / 13)];
  const rank = RANKS[value % 13];
  return `${rank}${SUIT_SYMBOLS[suit]}`;
}

// Example
console.log(decodeCard(10)); // "Q♠"
```

### Nonce Management

Nonces must be monotonically increasing for each address. Choose a persistence strategy based on your needs:

#### In-Memory (Development Only)
```python
# Simple counter - resets on restart
nonce = 0

def next_nonce():
    global nonce
    nonce += 1
    return nonce
```

#### File-Based (Simple Production)
```python
import json
import os

NONCE_FILE = "nonces.json"

def load_nonces():
    if os.path.exists(NONCE_FILE):
        with open(NONCE_FILE) as f:
            return json.load(f)
    return {}

def save_nonces(nonces):
    with open(NONCE_FILE, "w") as f:
        json.dump(nonces, f)

def next_nonce(address):
    nonces = load_nonces()
    nonce = nonces.get(address, 0) + 1
    nonces[address] = nonce
    save_nonces(nonces)
    return nonce
```

#### Redis (Scalable Production)
```python
import redis

r = redis.Redis(host='localhost', port=6379, db=0)

def next_nonce(address):
    key = f"nonce:{address}"
    return r.incr(key)
```

#### TypeScript (File-Based)
```typescript
import * as fs from 'fs';
import * as path from 'path';

const NONCE_FILE = path.join(process.cwd(), 'nonces.json');

function loadNonces(): Record<string, number> {
  if (fs.existsSync(NONCE_FILE)) {
    return JSON.parse(fs.readFileSync(NONCE_FILE, 'utf-8'));
  }
  return {};
}

function saveNonces(nonces: Record<string, number>): void {
  fs.writeFileSync(NONCE_FILE, JSON.stringify(nonces, null, 2));
}

function nextNonce(address: string): number {
  const nonces = loadNonces();
  const nonce = (nonces[address] || 0) + 1;
  nonces[address] = nonce;
  saveNonces(nonces);
  return nonce;
}
```

### Error Handling

#### Python
```python
import time
import requests
import json

def api_request_with_retry(request_func, max_retries=3):
    """Make API request with exponential backoff retry."""
    for attempt in range(max_retries):
        try:
            response = request_func()
            
            if response.status_code == 200:
                return response.json()
            elif response.status_code == 429:
                # Rate limited - wait and retry
                wait_time = 2 ** attempt
                print(f"Rate limited. Waiting {wait_time}s...")
                time.sleep(wait_time)
            elif response.status_code == 401:
                # Unauthorized - check your signature
                error = response.json()
                raise Exception(f"Unauthorized: {error.get('error', 'Invalid signature')}")
            elif response.status_code == 409:
                # Conflict - don't retry
                error = response.json()
                raise Exception(f"Conflict: {error.get('error', response.text)}")
            elif response.status_code >= 500:
                # Server error - retry
                wait_time = 2 ** attempt
                print(f"Server error {response.status_code}. Retrying in {wait_time}s...")
                time.sleep(wait_time)
            else:
                # Other errors - don't retry
                error = response.json()
                raise Exception(f"API error {response.status_code}: {error.get('error', error.get('message', response.text))}")
        except requests.exceptions.Timeout:
            if attempt < max_retries - 1:
                wait_time = 2 ** attempt
                print(f"Timeout. Retrying in {wait_time}s...")
                time.sleep(wait_time)
            else:
                raise Exception("Request timed out after retries")
        except requests.exceptions.ConnectionError:
            if attempt < max_retries - 1:
                wait_time = 2 ** attempt
                print(f"Connection error. Retrying in {wait_time}s...")
                time.sleep(wait_time)
            else:
                raise Exception("Connection failed after retries")
        except json.JSONDecodeError:
            raise Exception("Invalid JSON response from API")
    
    raise Exception("Max retries exceeded")
```

#### TypeScript
```typescript
async function apiRequestWithRetry<T>(
  requestFunc: () => Promise<Response>,
  maxRetries: number = 3
): Promise<T> {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      const response = await requestFunc();
      
      if (response.ok) {
        return await response.json() as T;
      }
      
      if (response.status === 429) {
        // Rate limited - wait and retry
        const waitTime = Math.pow(2, attempt) * 1000;
        console.log(`Rate limited. Waiting ${waitTime}ms...`);
        await new Promise(resolve => setTimeout(resolve, waitTime));
      } else if (response.status === 401) {
        // Unauthorized - check your signature
        const error = await response.json();
        throw new Error(`Unauthorized: ${error.error || 'Invalid signature'}`);
      } else if (response.status === 409) {
        // Conflict - don't retry
        const error = await response.json();
        throw new Error(`Conflict: ${error.error || await response.text()}`);
      } else if (response.status >= 500) {
        // Server error - retry
        const waitTime = Math.pow(2, attempt) * 1000;
        console.log(`Server error ${response.status}. Retrying in ${waitTime}ms...`);
        await new Promise(resolve => setTimeout(resolve, waitTime));
      } else {
        // Other errors - don't retry
        const error = await response.json();
        throw new Error(`API error ${response.status}: ${error.error || error.message || await response.text()}`);
      }
    } catch (error) {
      if (error instanceof SyntaxError) {
        throw new Error('Invalid JSON response from API');
      }
      if (attempt === maxRetries - 1) {
        throw error;
      }
      const waitTime = Math.pow(2, attempt) * 1000;
      console.log(`Request failed. Retrying in ${waitTime}ms...`);
      await new Promise(resolve => setTimeout(resolve, waitTime));
    }
  }
  
  throw new Error('Max retries exceeded');
}
```

---

## 1. Creating a Session (Table)

### Create a New Table

#### curl
```bash
curl -X POST http://localhost:8080/api/tables/create \
  -H "Content-Type: application/json" \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 1" \
  -H "x-auth-timestamp: 1719316800" \
  -d '{
    "max_players": 6,
    "solo": false,
    "buy_in": "10000000"
  }'
```

#### Python
```python
import requests
import time
from nacl.signing import SigningKey

BASE_URL = "http://localhost:8080"

def make_auth_headers(signing_key: SigningKey, address: str, table_id: int, action: str, nonce: int) -> dict:
    timestamp = int(time.time())
    message = f"stellar-poker|{address}|{table_id}|{action}|{nonce}|{timestamp}"
    sig = signing_key.sign(message.encode()).signature
    return {
        "x-player-address": address,
        "x-auth-signature": sig.hex(),
        "x-auth-nonce": str(nonce),
        "x-auth-timestamp": str(timestamp),
        "Content-Type": "application/json",
    }

# Generate or load your signing key
sk = SigningKey.generate()
address = "GABCD..."  # Your Stellar address

headers = make_auth_headers(sk, address, 0, "create_table", 1)
payload = {
    "max_players": 6,
    "solo": False,
    "buy_in": "10000000"
}

response = requests.post(f"{BASE_URL}/api/tables/create", json=payload, headers=headers)
print(response.json())
```

#### TypeScript
```typescript
import { Keypair } from '@stellar/stellar-sdk';

const API_BASE = process.env.COORDINATOR_URL || "http://localhost:8080";

// Generate or load your keypair
const keypair = Keypair.random();
const address = keypair.publicKey();

// Build auth headers
function buildAuthHeaders(
  keypair: Keypair,
  address: string,
  tableId: number,
  action: string,
  nonce: number
): Record<string, string> {
  const timestamp = Math.floor(Date.now() / 1000);
  const message = `stellar-poker|${address}|${tableId}|${action}|${nonce}|${timestamp}`;
  const signature = keypair.sign(Buffer.from(message)).toString('hex');
  
  return {
    "x-player-address": address,
    "x-auth-signature": signature,
    "x-auth-nonce": String(nonce),
    "x-auth-timestamp": String(timestamp),
    "Content-Type": "application/json",
  };
}

async function createTable(
  keypair: Keypair,
  address: string,
  maxPlayers: number,
  solo: boolean = false,
  buyIn?: string
) {
  const nonce = 1;
  const headers = buildAuthHeaders(keypair, address, 0, "create_table", nonce);
  
  const payload: Record<string, unknown> = {
    max_players: maxPlayers,
    solo,
  };
  if (buyIn) {
    payload.buy_in = buyIn;
  }
  
  const response = await fetch(`${API_BASE}/api/tables/create`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Create table failed: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

const table = await createTable(keypair, address, 6, false, "10000000");
console.log(table);
// Output: { table_id: 123, max_players: 6, joined_wallets: 1 }
```

### Join an Existing Table

#### curl
```bash
curl -X POST http://localhost:8080/api/table/123/join \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 2" \
  -H "x-auth-timestamp: 1719316800"
```

#### Python
```python
headers = make_auth_headers(sk, address, 123, "join_table", 2)
response = requests.post(f"{BASE_URL}/api/table/123/join", headers=headers)
print(response.json())
```

#### TypeScript
```typescript
async function joinTable(keypair: Keypair, address: string, tableId: number) {
  const nonce = 2;
  const headers = buildAuthHeaders(keypair, address, tableId, "join_table", nonce);
  
  const response = await fetch(`${API_BASE}/api/table/${tableId}/join`, {
    method: "POST",
    headers,
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Join table failed: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

const joinResult = await joinTable(keypair, address, 123);
console.log(joinResult);
// Output: { table_id: 123, seat_index: 1, seat_address: "G...", joined_wallets: 2, max_players: 6 }
```

---

## 2. Submitting Actions

### Request Deal (Start a Hand)

#### curl
```bash
curl -X POST http://localhost:8080/api/table/123/request-deal \
  -H "Content-Type: application/json" \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 3" \
  -H "x-auth-timestamp: 1719316800" \
  -d '{
    "players": ["GABCD...", "GEFGH..."]
  }'
```

#### Python
```python
headers = make_auth_headers(sk, address, 123, "request_deal", 3)
payload = {
    "players": ["GABCD...", "GEFGH..."]
}

response = requests.post(
    f"{BASE_URL}/api/table/123/request-deal",
    json=payload,
    headers=headers,
    timeout=600
)
print(response.json())
# Output: { status: "complete", deck_root: "...", hand_commitments: [...], proof_size: 1234, session_id: "..." }
```

#### TypeScript
```typescript
async function requestDeal(
  keypair: Keypair,
  address: string,
  tableId: number,
  players: string[]
) {
  const nonce = 3;
  const headers = buildAuthHeaders(keypair, address, tableId, "request_deal", nonce);
  
  const payload = { players };
  
  const response = await fetch(`${API_BASE}/api/table/${tableId}/request-deal`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Deal failed: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

const deal = await requestDeal(keypair, 123, ["GABCD...", "GEFGH..."]);
console.log(deal);
// Output: { status: "complete", deck_root: "...", hand_commitments: [...], proof_size: 1234, session_id: "..." }
```

### Player Betting Action

#### curl
```bash
curl -X POST http://localhost:8080/api/table/123/player-action \
  -H "Content-Type: application/json" \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 4" \
  -H "x-auth-timestamp: 1719316800" \
  -d '{
    "action": "raise",
    "amount": 5000000
  }'
```

#### Python
```python
headers = make_auth_headers(sk, address, 123, "player_action:raise", 4)
payload = {
    "action": "raise",
    "amount": 5000000
}

response = requests.post(
    f"{BASE_URL}/api/table/123/player-action",
    json=payload,
    headers=headers
)
print(response.json())
# Output: { status: "success", action: "raise", amount: 5000000, player: "GABCD...", tx_hash: "..." }
```

#### TypeScript
```typescript
async function playerAction(
  keypair: Keypair,
  address: string,
  tableId: number,
  action: "fold" | "check" | "call" | "bet" | "raise" | "allin",
  amount?: number
) {
  const nonce = 4;
  const headers = buildAuthHeaders(keypair, address, tableId, `player_action:${action}`, nonce);
  
  const payload: Record<string, unknown> = { action };
  if (amount !== undefined) {
    payload.amount = amount;
  }
  
  const response = await fetch(`${API_BASE}/api/table/${tableId}/player-action`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Player action failed: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

const action = await playerAction(keypair, address, 123, "raise", 5000000);
console.log(action);
// Output: { status: "success", action: "raise", amount: 5000000, player: "GABCD...", tx_hash: "..." }
```

### Request Reveal (Flop/Turn/River)

#### curl
```bash
# Flop
curl -X POST http://localhost:8080/api/table/123/request-reveal/flop \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 5" \
  -H "x-auth-timestamp: 1719316800"

# Turn
curl -X POST http://localhost:8080/api/table/123/request-reveal/turn \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 6" \
  -H "x-auth-timestamp: 1719316800"

# River
curl -X POST http://localhost:8080/api/table/123/request-reveal/river \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 7" \
  -H "x-auth-timestamp: 1719316800"
```

#### Python
```python
# Flop
headers = make_auth_headers(sk, address, 123, "request_reveal:flop", 5)
response = requests.post(
    f"{BASE_URL}/api/table/123/request-reveal/flop",
    headers=headers,
    timeout=600
)
print(response.json())
# Output: { status: "complete", cards: [10, 24, 38], proof_size: 856, session_id: "..." }

# Turn
headers = make_auth_headers(sk, address, 123, "request_reveal:turn", 6)
response = requests.post(
    f"{BASE_URL}/api/table/123/request-reveal/turn",
    headers=headers,
    timeout=600
)
print(response.json())
# Output: { status: "complete", cards: [51], proof_size: 428, session_id: "..." }

# River
headers = make_auth_headers(sk, address, 123, "request_reveal:river", 7)
response = requests.post(
    f"{BASE_URL}/api/table/123/request-reveal/river",
    headers=headers,
    timeout=600
)
print(response.json())
# Output: { status: "complete", cards: [12], proof_size: 428, session_id: "..." }
```

#### TypeScript
```typescript
async function requestReveal(
  keypair: Keypair,
  address: string,
  tableId: number,
  phase: "flop" | "turn" | "river"
) {
  const nonce = 5;
  const headers = buildAuthHeaders(keypair, address, tableId, `request_reveal:${phase}`, nonce);
  
  const response = await fetch(`${API_BASE}/api/table/${tableId}/request-reveal/${phase}`, {
    method: "POST",
    headers,
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Reveal failed: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

// Flop
const flop = await requestReveal(keypair, address, 123, "flop");
console.log(flop);
console.log(`Cards: ${flop.cards.map(decodeCard).join(", ")}`);
// Output: { status: "complete", cards: [10, 24, 38], proof_size: 856, session_id: "..." }
// Cards: Q♠, 10♥, 3♦

// Turn
const turn = await requestReveal(keypair, address, 123, "turn");
console.log(turn);
console.log(`Cards: ${turn.cards.map(decodeCard).join(", ")}`);
// Output: { status: "complete", cards: [51], proof_size: 428, session_id: "..." }
// Cards: A♣

// River
const river = await requestReveal(keypair, address, 123, "river");
console.log(river);
console.log(`Cards: ${river.cards.map(decodeCard).join(", ")}`);
// Output: { status: "complete", cards: [12], proof_size: 428, session_id: "..." }
// Cards: 2♠
```

### Request Showdown

#### curl
```bash
curl -X POST http://localhost:8080/api/table/123/request-showdown \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 8" \
  -H "x-auth-timestamp: 1719316800"
```

#### Python
```python
headers = make_auth_headers(sk, address, 123, "request_showdown", 8)
response = requests.post(
    f"{BASE_URL}/api/table/123/request-showdown",
    headers=headers,
    timeout=600
)
print(response.json())
# Output: { status: "complete", winner: "GABCD...", winner_index: 0, proof_size: 1234, session_id: "..." }
```

#### TypeScript
```typescript
async function requestShowdown(keypair: Keypair, address: string, tableId: number) {
  const nonce = 6;
  const headers = buildAuthHeaders(keypair, address, tableId, "request_showdown", nonce);
  
  const response = await fetch(`${API_BASE}/api/table/${tableId}/request-showdown`, {
    method: "POST",
    headers,
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Showdown failed: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

const showdown = await requestShowdown(keypair, address, 123);
console.log(showdown);
// Output: { status: "complete", winner: "GABCD...", winner_index: 0, proof_size: 1234, session_id: "..." }
```

---

## 3. Querying Game State

### Get Table State

#### curl
```bash
curl http://localhost:8080/api/table/123/state
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/table/123/state")
state = response.json()
print(state)
# Output: { state: "{...json string...}" }

# Parse the state
import json
parsed_state = json.loads(state["state"])
print(parsed_state)
```

#### TypeScript
```typescript
async function getTableState(tableId: number): Promise<{ state: string }> {
  const response = await fetch(`${API_BASE}/api/table/${tableId}/state`);
  
  if (!response.ok) {
    throw new Error(`Failed to get table state: ${response.statusText}`);
  }
  
  return response.json();
}

async function getParsedTableState(tableId: number) {
  const result = await getTableState(tableId);
  try {
    return {
      raw: result.state,
      parsed: JSON.parse(result.state) as Record<string, unknown>,
    };
  } catch {
    return { raw: result.state, parsed: null };
  }
}

// Raw state
const rawState = await getTableState(123);
console.log(rawState);
// Output: { state: "{...json string...}" }

// Parsed state
const parsedState = await getParsedTableState(123);
console.log(parsedState);
// Output: { raw: "{...json string...}", parsed: {...object...} }
```

### Get Table Lobby

#### curl
```bash
curl http://localhost:8080/api/table/123/lobby
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/table/123/lobby")
lobby = response.json()
print(lobby)
# Output: { table_id: 123, phase: "Waiting", max_players: 6, seats: [...], joined_wallets: 2 }
```

#### TypeScript
```typescript
async function getTableLobby(tableId: number) {
  const response = await fetch(`${API_BASE}/api/table/${tableId}/lobby`);
  
  if (!response.ok) {
    throw new Error(`Failed to get table lobby: ${response.statusText}`);
  }
  
  return response.json();
}

const lobby = await getTableLobby(123);
console.log(lobby);
// Output: { table_id: 123, phase: "Waiting", max_players: 6, seats: [...], joined_wallets: 2 }
```

### List Open Tables

#### curl
```bash
curl http://localhost:8080/api/tables/open
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/tables/open")
tables = response.json()
print(tables)
# Output: { tables: [{ table_id: 123, phase: "Waiting", max_players: 6, joined_wallets: 2, open_wallet_slots: 4 }] }
```

#### TypeScript
```typescript
async function listOpenTables() {
  const response = await fetch(`${API_BASE}/api/tables/open`);
  
  if (!response.ok) {
    throw new Error(`Failed to list open tables: ${response.statusText}`);
  }
  
  return response.json();
}

const openTables = await listOpenTables();
console.log(openTables);
// Output: { tables: [{ table_id: 123, phase: "Waiting", max_players: 6, joined_wallets: 2, open_wallet_slots: 4 }] }
```

### Get Player Cards

#### curl
```bash
curl http://localhost:8080/api/table/123/player/GABCD.../cards \
  -H "x-player-address: GABCD..." \
  -H "x-auth-signature: <signature>" \
  -H "x-auth-nonce: 9" \
  -H "x-auth-timestamp: 1719316800"
```

#### Python
```python
headers = make_auth_headers(sk, address, 123, "get_player_cards", 9)
response = requests.get(
    f"{BASE_URL}/api/table/123/player/{address}/cards",
    headers=headers
)
cards = response.json()
print(cards)
# Output: { card1: 10, card2: 24, salt1: "abc123", salt2: "def456" }
```

#### TypeScript
```typescript
async function getPlayerCards(keypair: Keypair, playerAddress: string, tableId: number) {
  const nonce = 7;
  const headers = buildAuthHeaders(keypair, playerAddress, tableId, "get_player_cards", nonce);
  
  const response = await fetch(`${API_BASE}/api/table/${tableId}/player/${address}/cards`, {
    headers,
  });
  
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Failed to get cards: ${error.error || error.message || response.statusText}`);
  }
  
  return response.json();
}

const cards = await getPlayerCards(keypair, address, 123);
console.log(cards);
console.log(`Hole cards: ${decodeCard(cards.card1)}, ${decodeCard(cards.card2)}`);
// Output: { card1: 10, card2: 24, salt1: "abc123", salt2: "def456" }
// Hole cards: Q♠, 10♥
```

### Get Committee Status

#### curl
```bash
curl http://localhost:8080/api/committee/status
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/committee/status")
status = response.json()
print(status)
# Output: { nodes: 3, healthy: [true, true, false], status: "degraded" }
```

#### TypeScript
```typescript
async function getCommitteeStatus() {
  const response = await fetch(`${API_BASE}/api/committee/status`);
  
  if (!response.ok) {
    throw new Error(`Failed to get committee status: ${response.statusText}`);
  }
  
  return response.json();
}

const status = await getCommitteeStatus();
console.log(status);
// Output: { nodes: 3, healthy: [true, true, false], status: "degraded" }
```

### Get MPC Session Status

#### curl
```bash
curl http://localhost:8080/api/session/session-abc123/status
```

#### Python
```python
session_id = "session-abc123"
response = requests.get(f"{BASE_URL}/api/session/{session_id}/status")
session_status = response.json()
print(session_status)
# Output: { session_id: "session-abc123", table_id: 123, status: "complete", elapsed_secs: 45, cancel_reason: null }
```

#### TypeScript
```typescript
async function getMpcSessionStatus(sessionId: string) {
  const response = await fetch(`${API_BASE}/api/session/${sessionId}/status`);
  
  if (!response.ok) {
    throw new Error(`Failed to get session status: ${response.statusText}`);
  }
  
  return response.json();
}

const sessionStatus = await getMpcSessionStatus("session-abc123");
console.log(sessionStatus);
// Output: { session_id: "session-abc123", table_id: 123, status: "complete", elapsed_secs: 45, cancel_reason: null }
```

---

## 4. Verifying Proofs

Proofs are generated by the MPC committee and verified on-chain via the Soroban smart contract. The coordinator API returns proof metadata including `proof_size` and `session_id`. The actual cryptographic verification happens in the ZK verifier contract.

### Understanding Proof Verification

1. **MPC Committee**: Generates UltraHonk zero-knowledge proofs for game operations
2. **Coordinator**: Submits proofs to the Soroban blockchain via the poker-table contract
3. **ZK Verifier Contract**: Verifies proofs using the appropriate verification key (VK) for each circuit type:
   - `DealValid`: Verifies card dealing and deck shuffling
   - `RevealBoardValid`: Verifies board card reveals
   - `ShowdownValid`: Verifies hand evaluation and winner determination

### Proof Response Data

#### Python
```python
deal_response = response.json()
print(f"Proof size: {deal_response['proof_size']} bytes")
print(f"Session ID: {deal_response['session_id']}")
print(f"Deck root: {deal_response['deck_root']}")
print(f"Hand commitments: {deal_response['hand_commitments']}")
print(f"Transaction hash: {deal_response.get('tx_hash', 'N/A')}")
```

#### TypeScript
```typescript
const deal = await requestDeal(keypair, address, 123, [address, "GEFGH..."]);
console.log(`Proof size: ${deal.proof_size} bytes`);
console.log(`Session ID: ${deal.session_id}`);
console.log(`Deck root: ${deal.deck_root}`);
console.log(`Hand commitments: ${deal.hand_commitments}`);
console.log(`Transaction hash: ${deal.tx_hash || 'N/A'}`);
```

### Monitoring Proof Verification

You can monitor the on-chain transaction status using the returned `tx_hash`:

#### Python
```python
from stellar_sdk import Server

# Use appropriate server for your network
if "testnet" in BASE_URL:
    horizon_url = "https://horizon-testnet.stellar.org"
else:
    horizon_url = "https://horizon.stellar.org"

server = Server(horizon_url)
tx_hash = deal_response.get('tx_hash')
if tx_hash:
    tx = server.transactions().transaction(tx_hash).call()
    print(f"Transaction status: {tx['status']}")
    print(f"Ledger: {tx['ledger']}")
```

#### TypeScript
```typescript
import { Server } from '@stellar/stellar-sdk';

// Use appropriate server for your network
const horizonUrl = API_BASE.includes('testnet') 
  ? 'https://horizon-testnet.stellar.org'
  : 'https://horizon.stellar.org';

const server = new Server(horizonUrl);
const txHash = deal.tx_hash;

if (txHash) {
  const tx = await server.transactions().transaction(txHash).call();
  console.log(`Transaction status: ${tx.status}`);
  console.log(`Ledger: ${tx.ledger}`);
}
```

---

## Additional Endpoints

### Health Check

#### curl
```bash
curl http://localhost:8080/api/health
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/health")
print(response.json())
```

#### TypeScript
```typescript
async function getHealth() {
  const response = await fetch(`${API_BASE}/api/health`);
  
  if (!response.ok) {
    throw new Error(`Health check failed: ${response.statusText}`);
  }
  
  return response.json();
}

const health = await getHealth();
console.log(health);
```

### Get Chain Configuration

#### curl
```bash
curl http://localhost:8080/api/chain-config
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/chain-config")
config = response.json()
print(config)
# Output: { rpc_url: "...", network_passphrase: "...", poker_table_contract: "..." }
```

#### TypeScript
```typescript
async function getChainConfig() {
  const response = await fetch(`${API_BASE}/api/chain-config`);
  
  if (!response.ok) {
    throw new Error(`Failed to get chain config: ${response.statusText}`);
  }
  
  return response.json();
}

const config = await getChainConfig();
console.log(config);
// Output: { rpc_url: "...", network_passphrase: "...", poker_table_contract: "..." }
```

### Get Stats

#### curl
```bash
curl http://localhost:8080/api/stats
```

#### Python
```python
response = requests.get(f"{BASE_URL}/api/stats")
stats = response.json()
print(stats)
# Output: { global: {...}, leaderboard: [...], cached_at: 1719316800 }
```

#### TypeScript
```typescript
async function getStats() {
  const response = await fetch(`${API_BASE}/api/stats`);
  
  if (!response.ok) {
    throw new Error(`Failed to get stats: ${response.statusText}`);
  }
  
  return response.json();
}

const stats = await getStats();
console.log(stats);
// Output: { global: {...}, leaderboard: [...], cached_at: 1719316800 }
```

---

## Error Handling

All endpoints may return error responses. Handle them appropriately:

### Python
```python
response = requests.post(f"{BASE_URL}/api/tables/create", json=payload, headers=headers)

if response.status_code == 200:
    print(response.json())
else:
    error = response.json()
    print(f"Error {response.status_code}: {error.get('error', error.get('message', response.text))}")
```

### TypeScript
```typescript
try {
    const table = await createTable(auth, 6, false);
    console.log(table);
} catch (error) {
    console.error('Failed to create table:', error);
}
```

---

---

## 5. Complete Hand Flow Example

This example demonstrates a complete poker hand from deal to showdown.

#### Python
```python
import requests
import time
import struct
import base64
import json
from nacl.signing import SigningKey

BASE_URL = "http://localhost:8080"
TABLE_ID = 123

# Helper functions (copy from above)
def encode_stellar_pubkey(raw_32: bytes) -> str:
    payload = bytes([6 << 3]) + raw_32
    crc = _crc16_xmodem(payload)
    full = payload + struct.pack("<H", crc)
    return _base32_encode(full)

def _crc16_xmodem(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            if crc & 0x8000:
                crc = (crc << 1) ^ 0x1021
            else:
                crc <<= 1
            crc &= 0xFFFF
    return crc

def _base32_encode(data: bytes) -> str:
    return base64.b32encode(data).decode("ascii").rstrip("=")

def make_auth_headers(signing_key, address, table_id, action, nonce):
    timestamp = int(time.time())
    message = f"stellar-poker|{address}|{table_id}|{action}|{nonce}|{timestamp}"
    sig = signing_key.sign(message.encode()).signature
    return {
        "x-player-address": address,
        "x-auth-signature": sig.hex(),
        "x-auth-nonce": str(nonce),
        "x-auth-timestamp": str(timestamp),
        "Content-Type": "application/json",
    }

SUITS = ["Spades", "Hearts", "Diamonds", "Clubs"]
RANKS = ["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"]

def decode_card(value: int) -> str:
    suit = SUITS[value // 13]
    rank = RANKS[value % 13]
    return f"{rank} of {suit}"

# Setup authentication
sk = SigningKey.generate()
address = encode_stellar_pubkey(bytes(sk.verify_key))
nonce = 0

def next_nonce():
    global nonce
    nonce += 1
    return nonce

# Step 1: Request Deal
print("=== Step 1: Request Deal ===")
headers = make_auth_headers(sk, address, TABLE_ID, "request_deal", next_nonce())
payload = {"players": [address, "GEFGH..."]}
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/request-deal",
    json=payload,
    headers=headers,
    timeout=600
)
deal = response.json()
print(f"Deck root: {deal['deck_root']}")
print(f"Session ID: {deal['session_id']}")

# Step 2: Get Hole Cards
print("\n=== Step 2: Get Hole Cards ===")
headers = make_auth_headers(sk, address, TABLE_ID, "get_player_cards", next_nonce())
response = requests.get(
    f"{BASE_URL}/api/table/{TABLE_ID}/player/{address}/cards",
    headers=headers
)
cards = response.json()
print(f"Your cards: {decode_card(cards['card1'])}, {decode_card(cards['card2'])}")

# Step 3: Preflop Betting (example)
print("\n=== Step 3: Preflop Betting ===")
headers = make_auth_headers(sk, address, TABLE_ID, "player_action:call", next_nonce())
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/player-action",
    json={"action": "call"},
    headers=headers
)
print(f"Action result: {response.json()}")

# Step 4: Reveal Flop
print("\n=== Step 4: Reveal Flop ===")
headers = make_auth_headers(sk, address, TABLE_ID, "request_reveal:flop", next_nonce())
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/request-reveal/flop",
    headers=headers,
    timeout=600
)
flop = response.json()
print(f"Flop: {', '.join(decode_card(c) for c in flop['cards'])}")

# Step 5: Flop Betting
print("\n=== Step 5: Flop Betting ===")
headers = make_auth_headers(sk, address, TABLE_ID, "player_action:check", next_nonce())
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/player-action",
    json={"action": "check"},
    headers=headers
)
print(f"Action result: {response.json()}")

# Step 6: Reveal Turn
print("\n=== Step 6: Reveal Turn ===")
headers = make_auth_headers(sk, address, TABLE_ID, "request_reveal:turn", next_nonce())
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/request-reveal/turn",
    headers=headers,
    timeout=600
)
turn = response.json()
print(f"Turn: {decode_card(turn['cards'][0])}")

# Step 7: Reveal River
print("\n=== Step 7: Reveal River ===")
headers = make_auth_headers(sk, address, TABLE_ID, "request_reveal:river", next_nonce())
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/request-reveal/river",
    headers=headers,
    timeout=600
)
river = response.json()
print(f"River: {decode_card(river['cards'][0])}")

# Step 8: Showdown
print("\n=== Step 8: Showdown ===")
headers = make_auth_headers(sk, address, TABLE_ID, "request_showdown", next_nonce())
response = requests.post(
    f"{BASE_URL}/api/table/{TABLE_ID}/request-showdown",
    headers=headers,
    timeout=600
)
showdown = response.json()
print(f"Winner: {showdown['winner']}")
print(f"Winner index: {showdown['winner_index']}")

print("\n=== Hand Complete ===")
```

### TypeScript
```typescript
import { Keypair } from '@stellar/stellar-sdk';

const API_BASE = process.env.COORDINATOR_URL || "http://localhost:8080";
const TABLE_ID = 123;

// Helper functions (copy from above)
const SUITS = ["clubs", "diamonds", "hearts", "spades"] as const;
const RANKS = ["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"] as const;

const SUIT_SYMBOLS: Record<typeof SUITS[number], string> = {
  clubs: "♣",
  diamonds: "♦",
  hearts: "♥",
  spades: "♠",
};

function decodeCard(value: number): string {
  const suit = SUITS[Math.floor(value / 13)];
  const rank = RANKS[value % 13];
  return `${rank}${SUIT_SYMBOLS[suit]}`;
}

function buildAuthHeaders(
  keypair: Keypair,
  address: string,
  tableId: number,
  action: string,
  nonce: number
): Record<string, string> {
  const timestamp = Math.floor(Date.now() / 1000);
  const message = `stellar-poker|${address}|${tableId}|${action}|${nonce}|${timestamp}`;
  const signature = keypair.sign(Buffer.from(message)).toString('hex');
  
  return {
    "x-player-address": address,
    "x-auth-signature": signature,
    "x-auth-nonce": String(nonce),
    "x-auth-timestamp": String(timestamp),
    "Content-Type": "application/json",
  };
}

async function requestDeal(
  keypair: Keypair,
  address: string,
  tableId: number,
  players: string[]
) {
  const nonce = 1;
  const headers = buildAuthHeaders(keypair, address, tableId, "request_deal", nonce);
  const payload = { players };
  const response = await fetch(`${API_BASE}/api/table/${tableId}/request-deal`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Deal failed: ${error.error || error.message || response.statusText}`);
  }
  return response.json();
}

async function getPlayerCards(keypair: Keypair, playerAddress: string, tableId: number) {
  const nonce = 2;
  const headers = buildAuthHeaders(keypair, playerAddress, tableId, "get_player_cards", nonce);
  const response = await fetch(`${API_BASE}/api/table/${tableId}/player/${playerAddress}/cards`, {
    headers,
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Failed to get cards: ${error.error || error.message || response.statusText}`);
  }
  return response.json();
}

async function playerAction(
  keypair: Keypair,
  address: string,
  tableId: number,
  action: "fold" | "check" | "call" | "bet" | "raise" | "allin",
  amount?: number
) {
  const nonce = 3;
  const headers = buildAuthHeaders(keypair, address, tableId, `player_action:${action}`, nonce);
  const payload: Record<string, unknown> = { action };
  if (amount !== undefined) {
    payload.amount = amount;
  }
  const response = await fetch(`${API_BASE}/api/table/${tableId}/player-action`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Player action failed: ${error.error || error.message || response.statusText}`);
  }
  return response.json();
}

async function requestReveal(
  keypair: Keypair,
  address: string,
  tableId: number,
  phase: "flop" | "turn" | "river"
) {
  const nonce = 4;
  const headers = buildAuthHeaders(keypair, address, tableId, `request_reveal:${phase}`, nonce);
  const response = await fetch(`${API_BASE}/api/table/${tableId}/request-reveal/${phase}`, {
    method: "POST",
    headers,
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Reveal failed: ${error.error || error.message || response.statusText}`);
  }
  return response.json();
}

async function requestShowdown(keypair: Keypair, address: string, tableId: number) {
  const nonce = 5;
  const headers = buildAuthHeaders(keypair, address, tableId, "request_showdown", nonce);
  const response = await fetch(`${API_BASE}/api/table/${tableId}/request-showdown`, {
    method: "POST",
    headers,
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(`Showdown failed: ${error.error || error.message || response.statusText}`);
  }
  return response.json();
}

// Setup authentication
const keypair = Keypair.random();
const address = keypair.publicKey();
let nonce = 0;

function nextNonce(): number {
  return ++nonce;
}

async function playCompleteHand() {
  // Step 1: Request Deal
  console.log("=== Step 1: Request Deal ===");
  const deal = await requestDeal(keypair, address, TABLE_ID, [address, "GEFGH..."]);
  console.log(`Deck root: ${deal.deck_root}`);
  console.log(`Session ID: ${deal.session_id}`);

  // Step 2: Get Hole Cards
  console.log("\n=== Step 2: Get Hole Cards ===");
  const cards = await getPlayerCards(keypair, address, TABLE_ID);
  console.log(`Your cards: ${decodeCard(cards.card1)}, ${decodeCard(cards.card2)}`);

  // Step 3: Preflop Betting
  console.log("\n=== Step 3: Preflop Betting ===");
  const callAction = await playerAction(keypair, address, TABLE_ID, "call");
  console.log(`Action result:`, callAction);

  // Step 4: Reveal Flop
  console.log("\n=== Step 4: Reveal Flop ===");
  const flop = await requestReveal(keypair, address, TABLE_ID, "flop");
  console.log(`Flop: ${flop.cards.map(decodeCard).join(", ")}`);

  // Step 5: Flop Betting
  console.log("\n=== Step 5: Flop Betting ===");
  const checkAction = await playerAction(keypair, address, TABLE_ID, "check");
  console.log(`Action result:`, checkAction);

  // Step 6: Reveal Turn
  console.log("\n=== Step 6: Reveal Turn ===");
  const turn = await requestReveal(keypair, address, TABLE_ID, "turn");
  console.log(`Turn: ${turn.cards.map(decodeCard).join(", ")}`);

  // Step 7: Reveal River
  console.log("\n=== Step 7: Reveal River ===");
  const river = await requestReveal(keypair, address, TABLE_ID, "river");
  console.log(`River: ${river.cards.map(decodeCard).join(", ")}`);

  // Step 8: Showdown
  console.log("\n=== Step 8: Showdown ===");
  const showdown = await requestShowdown(keypair, address, TABLE_ID);
  console.log(`Winner: ${showdown.winner}`);
  console.log(`Winner index: ${showdown.winner_index}`);

  console.log("\n=== Hand Complete ===");
}

playCompleteHand().catch(console.error);
```

---

---

## Testing

### Simple Health Check Test

#### Python
```python
import requests

BASE_URL = "http://localhost:8080"

def test_health_check():
    response = requests.get(f"{BASE_URL}/api/health")
    assert response.status_code == 200
    data = response.json()
    print(f"Health check passed: {data}")
    return True

if __name__ == "__main__":
    test_health_check()
```

#### TypeScript
```typescript
async function testHealthCheck(): Promise<boolean> {
  const API_BASE = process.env.COORDINATOR_URL || "http://localhost:8080";
  const response = await fetch(`${API_BASE}/api/health`);
  
  if (!response.ok) {
    throw new Error(`Health check failed: ${response.statusText}`);
  }
  
  const data = await response.json();
  console.log(`Health check passed:`, data);
  return true;
}

testHealthCheck().catch(console.error);
```

### Integration Test Example

#### Python
```python
import pytest
import requests
import struct
import base64
import time
from nacl.signing import SigningKey

BASE_URL = "http://localhost:8080"

# Helper functions
def encode_stellar_pubkey(raw_32: bytes) -> str:
    payload = bytes([6 << 3]) + raw_32
    crc = _crc16_xmodem(payload)
    full = payload + struct.pack("<H", crc)
    return _base32_encode(full)

def _crc16_xmodem(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            if crc & 0x8000:
                crc = (crc << 1) ^ 0x1021
            else:
                crc <<= 1
            crc &= 0xFFFF
    return crc

def _base32_encode(data: bytes) -> str:
    return base64.b32encode(data).decode("ascii").rstrip("=")

def make_auth_headers(signing_key, address, table_id, action, nonce):
    timestamp = int(time.time())
    message = f"stellar-poker|{address}|{table_id}|{action}|{nonce}|{timestamp}"
    sig = signing_key.sign(message.encode()).signature
    return {
        "x-player-address": address,
        "x-auth-signature": sig.hex(),
        "x-auth-nonce": str(nonce),
        "x-auth-timestamp": str(timestamp),
        "Content-Type": "application/json",
    }

def test_create_and_join_table():
    # Generate test keypair
    sk = SigningKey.generate()
    address = encode_stellar_pubkey(bytes(sk.verify_key))
    
    # Create table
    headers = make_auth_headers(sk, address, 0, "create_table", 1)
    payload = {"max_players": 2, "solo": True}
    response = requests.post(f"{BASE_URL}/api/tables/create", json=payload, headers=headers)
    assert response.status_code == 200
    table = response.json()
    
    table_id = table["table_id"]
    
    # Join table
    headers = make_auth_headers(sk, address, table_id, "join_table", 2)
    response = requests.post(f"{BASE_URL}/api/table/{table_id}/join", headers=headers)
    assert response.status_code == 200
    
    print(f"Test passed: Created and joined table {table_id}")
```

#### TypeScript
```typescript
import { Keypair } from '@stellar/stellar-sdk';

// Helper function
function buildAuthHeaders(
  keypair: Keypair,
  address: string,
  tableId: number,
  action: string,
  nonce: number
): Record<string, string> {
  const timestamp = Math.floor(Date.now() / 1000);
  const message = `stellar-poker|${address}|${tableId}|${action}|${nonce}|${timestamp}`;
  const signature = keypair.sign(Buffer.from(message)).toString('hex');
  
  return {
    "x-player-address": address,
    "x-auth-signature": signature,
    "x-auth-nonce": String(nonce),
    "x-auth-timestamp": String(timestamp),
    "Content-Type": "application/json",
  };
}

async function testCreateAndJoinTable(): Promise<void> {
  const API_BASE = process.env.COORDINATOR_URL || "http://localhost:8080";
  
  // Generate test keypair
  const keypair = Keypair.random();
  const address = keypair.publicKey();
  
  // Create table
  const headers = buildAuthHeaders(keypair, address, 0, "create_table", 1);
  const payload = { max_players: 2, solo: true };
  const response = await fetch(`${API_BASE}/api/tables/create`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  
  if (!response.ok) {
    throw new Error(`Create table failed: ${await response.text()}`);
  }
  
  const table = await response.json();
  const tableId = table.table_id;
  
  // Join table
  const joinHeaders = buildAuthHeaders(keypair, address, tableId, "join_table", 2);
  const joinResponse = await fetch(`${API_BASE}/api/table/${tableId}/join`, {
    method: "POST",
    headers: joinHeaders,
  });
  
  if (!joinResponse.ok) {
    throw new Error(`Join table failed: ${await joinResponse.text()}`);
  }
  
  console.log(`Test passed: Created and joined table ${tableId}`);
}

testCreateAndJoinTable().catch(console.error);
```

---

## Notes

- **Nonce Management**: Nonces must be monotonically increasing for each address. Use file-based or Redis persistence for production (see examples above).
- **Timeouts**: MPC operations (deal, reveal, showdown) can take significant time. Use appropriate timeouts (e.g., 600 seconds).
- **Proof Verification**: The coordinator handles proof submission to the blockchain. The ZK verifier contract performs cryptographic verification. The `proof_size` field is provided for monitoring purposes.
- **On-Chain Integration**: When Soroban is configured, actions like betting will submit transactions to the blockchain. Check the `tx_hash` field in responses to track on-chain confirmation.
- **Rate Limiting**: Some endpoints have rate limits. Implement exponential backoff for retries (see error handling examples).
- **Real-time Updates**: The API does not currently support WebSocket or SSE. Poll the table state endpoint for updates.
- **Card Values**: Cards are represented as integers 0-51. Use the provided `decode_card`/`decodeCard` helper to convert to readable format.
- **Network Configuration**: Configure the appropriate BASE_URL and Horizon server for your network (local, testnet, mainnet).
- **Key Management**: Store secret keys securely using environment variables or a key management service. Never commit secrets to version control.
- **Error Codes**:
  - `400`: Invalid parameters
  - `401`: Unauthorized (invalid signature)
  - `404`: Resource not found
  - `409`: Conflict (e.g., table already in active hand)
  - `429`: Rate limited
  - `502`: On-chain submission failed
  - `503`: Service unavailable (Soroban not configured or no MPC nodes)
