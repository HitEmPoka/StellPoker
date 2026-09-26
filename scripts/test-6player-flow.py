#!/usr/bin/env python3
"""
End-to-end test for a complete 6-player hand on StellPoker.

Flow:
 1. Health & committee liveness check
 2. 6 simulated players join / deal request (deal_valid_6p circuit)
 3. Retrieve hole cards for all 6 players
 4. Preflop betting (all 6 players act)
 5. Flop reveal & betting
 6. Turn reveal & betting
 7. River reveal & betting
 8. Showdown (showdown_valid_6p circuit) & pot distribution verification

Supports running off-chain against local coordinator API or on-chain with local Soroban node.
"""

import json
import os
import struct
import subprocess
import time
import requests
from nacl.signing import SigningKey

DEFAULT_ENV_FILE = os.environ.get(
    "TEST_ENV_FILE",
    os.path.join(os.path.dirname(__file__), "..", ".env.local"),
)

env_vars = {}
if os.path.exists(DEFAULT_ENV_FILE):
    with open(DEFAULT_ENV_FILE) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                env_vars[k] = v.strip('"')

BASE = os.environ.get(
    "COORDINATOR_URL",
    env_vars.get("NEXT_PUBLIC_COORDINATOR_URL", "http://localhost:8080"),
).rstrip("/")
SOROBAN_RPC = os.environ.get(
    "SOROBAN_RPC",
    env_vars.get("SOROBAN_RPC", "http://localhost:8000/soroban/rpc"),
)
NETWORK_PASSPHRASE = os.environ.get(
    "NETWORK_PASSPHRASE",
    env_vars.get("NETWORK_PASSPHRASE", "Standalone Network ; February 2017"),
)
COMMITTEE_IDENTITY = os.environ.get(
    "COMMITTEE_IDENTITY",
    env_vars.get("COMMITTEE_IDENTITY", "committee-local"),
)
POKER_TABLE_CONTRACT = env_vars.get("POKER_TABLE_CONTRACT", "")
TABLE_ID = int(env_vars.get("TABLE_ID", os.environ.get("TABLE_ID", "0")))
ON_CHAIN = bool(POKER_TABLE_CONTRACT)

NUM_PLAYERS = 6

print(f"=== StellPoker 6-Player Hand E2E Test ===")
print(f"Using env file: {DEFAULT_ENV_FILE}")
print(f"Coordinator URL: {BASE}")
if ON_CHAIN:
    print(f"On-chain mode: contract={POKER_TABLE_CONTRACT}, Table ID={TABLE_ID}")
else:
    print("Off-chain mode")

# --- Key & address encoding ---

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
    import base64
    return base64.b32encode(data).decode("ascii").rstrip("=")

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

# --- On-chain contract helpers ---

def stellar_player_action(player_identity: str, table_id: int, player_address: str, action_json: str):
    if not ON_CHAIN:
        return True
    cmd = [
        "stellar", "contract", "invoke",
        "--id", POKER_TABLE_CONTRACT,
        "--source", player_identity,
        "--rpc-url", SOROBAN_RPC,
        "--network-passphrase", NETWORK_PASSPHRASE,
        "--",
        "player_action",
        "--table_id", str(table_id),
        "--player", player_address,
        "--action", action_json,
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    return result.returncode == 0

def get_on_chain_table():
    if not ON_CHAIN:
        return None
    cmd = [
        "stellar", "contract", "invoke",
        "--id", POKER_TABLE_CONTRACT,
        "--source", COMMITTEE_IDENTITY,
        "--rpc-url", SOROBAN_RPC,
        "--network-passphrase", NETWORK_PASSPHRASE,
        "--send=no",
        "--",
        "get_table",
        "--table_id", str(TABLE_ID),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
    if result.returncode == 0:
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError:
            return None
    return None

def get_on_chain_phase():
    state = get_on_chain_table()
    if isinstance(state, dict):
        return state.get("phase", "unknown")
    return "unknown"

def player_stack_total(table_state: dict) -> int:
    return sum(int(p.get("stack", 0)) for p in table_state.get("players", []))

def table_chip_total(table_state: dict) -> int:
    return (
        player_stack_total(table_state)
        + int(table_state.get("pot", 0))
        + int(table_state.get("rake_balance", 0))
    )

def ensure_on_chain_ready_for_deal():
    if not ON_CHAIN:
        return
    phase = get_on_chain_phase()
    if phase == "Dealing":
        return
    if phase in ("Waiting", "Settlement"):
        print(f"  Starting new 6-player hand on-chain...")
        cmd = [
            "stellar", "contract", "invoke",
            "--id", POKER_TABLE_CONTRACT,
            "--source", COMMITTEE_IDENTITY,
            "--rpc-url", SOROBAN_RPC,
            "--network-passphrase", NETWORK_PASSPHRASE,
            "--",
            "start_hand",
            "--table_id", str(TABLE_ID),
        ]
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if res.returncode != 0:
            print(f"  ERROR: start_hand failed: {res.stderr}")
            exit(1)

# --- Generate 6 simulated player keypairs ---

players = []
for i in range(NUM_PLAYERS):
    sk = SigningKey.generate()
    addr = encode_stellar_pubkey(bytes(sk.verify_key))
    ident = os.environ.get(f"PLAYER{i+1}_IDENTITY", f"player{i+1}-local")
    players.append({
        "id": i + 1,
        "sk": sk,
        "addr": addr,
        "ident": ident,
        "nonce": 0,
    })
    print(f"  Player {i+1}: {addr}")

def get_nonce(player):
    player["nonce"] += 1
    return player["nonce"]

# --- Step 1: Health check ---
print("\n=== 1. Coordinator Health & Committee Liveness ===")
r = requests.get(f"{BASE}/api/health")
print(f"  Health status: {r.status_code}")
r = requests.get(f"{BASE}/api/committee/status")
print(f"  Committee status: {r.status_code}")

if ON_CHAIN:
    ensure_on_chain_ready_for_deal()
    initial_table = get_on_chain_table()
    initial_chips = table_chip_total(initial_table) if initial_table else None
else:
    initial_chips = None

# --- Step 2: Request Deal for 6 players ---
print(f"\n=== 2. Request Deal ({NUM_PLAYERS} Players) ===")
p1 = players[0]
player_addrs = [p["addr"] for p in players]
headers = make_auth_headers(p1["sk"], p1["addr"], TABLE_ID, "request_deal", get_nonce(p1))
payload = {"players": player_addrs}

r = requests.post(f"{BASE}/api/table/{TABLE_ID}/request-deal", json=payload, headers=headers, timeout=600)
print(f"  Deal HTTP status: {r.status_code}")
if r.status_code == 200:
    deal_resp = r.json()
    print(f"  Deal succeeded! Hand #{deal_resp.get('hand_number', 1)}")
else:
    print(f"  Deal failed: {r.text[:500]}")
    exit(1)

# --- Step 3: Retrieve Hole Cards for all 6 players ---
print(f"\n=== 3. Retrieve Hole Cards for All 6 Players ===")
for p in players:
    h = make_auth_headers(p["sk"], p["addr"], TABLE_ID, "get_player_cards", get_nonce(p))
    resp = requests.get(f"{BASE}/api/table/{TABLE_ID}/player/{p['addr']}/cards", headers=h, timeout=30)
    if resp.status_code == 200:
        c = resp.json()
        print(f"  Player {p['id']}: Card 1={c.get('card1')}, Card 2={c.get('card2')}")
    else:
        print(f"  Player {p['id']}: card retrieval HTTP {resp.status_code}")

# --- Step 4: Preflop Betting across 6 Players ---
print(f"\n=== 4. Preflop Betting Round ===")
if ON_CHAIN:
    phase = get_on_chain_phase()
    print(f"  On-chain phase: {phase}")
    # UTG, UTG+1, Cutoff call; Button calls; SB calls; BB checks
    for p in players:
        action = '"Call"' if p["id"] < 6 else '"Check"'
        stellar_player_action(p["ident"], TABLE_ID, p["addr"], action)

# --- Step 5: Reveal Flop ---
print(f"\n=== 5. Request Reveal: Flop ===")
h = make_auth_headers(p1["sk"], p1["addr"], TABLE_ID, "request_reveal:flop", get_nonce(p1))
r = requests.post(f"{BASE}/api/table/{TABLE_ID}/request-reveal/flop", headers=h, timeout=600)
print(f"  Flop status: {r.status_code}")
if r.status_code == 200:
    flop_data = r.json()
    print(f"  Flop cards: {flop_data.get('cards')}")
else:
    print(f"  Flop reveal failed: {r.text}")
    exit(1)

# --- Step 6: Flop Betting ---
if ON_CHAIN:
    print(f"\n=== 6. Flop Betting Round ===")
    for p in players:
        stellar_player_action(p["ident"], TABLE_ID, p["addr"], '"Check"')

# --- Step 7: Reveal Turn ---
print(f"\n=== 7. Request Reveal: Turn ===")
h = make_auth_headers(p1["sk"], p1["addr"], TABLE_ID, "request_reveal:turn", get_nonce(p1))
r = requests.post(f"{BASE}/api/table/{TABLE_ID}/request-reveal/turn", headers=h, timeout=600)
print(f"  Turn status: {r.status_code}")
if r.status_code == 200:
    turn_data = r.json()
    print(f"  Turn card: {turn_data.get('cards')}")
else:
    print(f"  Turn reveal failed: {r.text}")
    exit(1)

# --- Step 8: Turn Betting ---
if ON_CHAIN:
    print(f"\n=== 8. Turn Betting Round ===")
    for p in players:
        stellar_player_action(p["ident"], TABLE_ID, p["addr"], '"Check"')

# --- Step 9: Reveal River ---
print(f"\n=== 9. Request Reveal: River ===")
h = make_auth_headers(p1["sk"], p1["addr"], TABLE_ID, "request_reveal:river", get_nonce(p1))
r = requests.post(f"{BASE}/api/table/{TABLE_ID}/request-reveal/river", headers=h, timeout=600)
print(f"  River status: {r.status_code}")
if r.status_code == 200:
    river_data = r.json()
    print(f"  River card: {river_data.get('cards')}")
else:
    print(f"  River reveal failed: {r.text}")
    exit(1)

# --- Step 10: River Betting ---
if ON_CHAIN:
    print(f"\n=== 10. River Betting Round ===")
    for p in players:
        stellar_player_action(p["ident"], TABLE_ID, p["addr"], '"Check"')

# --- Step 11: Request Showdown & Pot Distribution Verification ---
print(f"\n=== 11. Request 6-Player Showdown & Pot Settlement ===")
h = make_auth_headers(p1["sk"], p1["addr"], TABLE_ID, "request_showdown", get_nonce(p1))
r = requests.post(f"{BASE}/api/table/{TABLE_ID}/request-showdown", headers=h, timeout=600)
print(f"  Showdown status: {r.status_code}")
if r.status_code == 200:
    showdown = r.json()
    winner = showdown.get("winner")
    winner_idx = showdown.get("winner_index")
    proof_sz = showdown.get("proof_size")
    print(f"  Winner address: {winner}")
    print(f"  Winner index: {winner_idx} (0 to 5)")
    print(f"  Showdown proof size: {proof_sz} bytes")
    if winner_idx is not None and (winner_idx < 0 or winner_idx >= NUM_PLAYERS):
        print(f"  ERROR: winner_index {winner_idx} out of range [0, 5]")
        exit(1)
else:
    print(f"  Showdown failed: {r.text}")
    exit(1)

# --- Step 12: Final Pot Distribution & Chip Conservation Invariant Check ---
if ON_CHAIN:
    print("\n=== 12. Verifying On-Chain Pot Settlement & Chip Conservation ===")
    final_table = get_on_chain_table()
    if not final_table:
        print("  ERROR: Could not read final table state")
        exit(1)

    phase = final_table.get("phase")
    pot = int(final_table.get("pot", -1))
    print(f"  Final phase: {phase}")
    print(f"  Final pot balance: {pot}")

    if phase != "Settlement":
        print(f"  ERROR: Table phase is {phase}, expected Settlement")
        exit(1)

    if pot != 0:
        print(f"  ERROR: Pot is {pot}, expected 0 after showdown settlement")
        exit(1)

    final_chips = table_chip_total(final_table)
    if initial_chips is not None and final_chips != initial_chips:
        print(f"  ERROR: Total chips changed from {initial_chips} to {final_chips}")
        exit(1)

    print(f"  SUCCESS: 6-player pot distributed correctly, chip total conserved ({final_chips} chips).")

print("\n=== 6-PLAYER HAND E2E TEST COMPLETE: PASS ===")
