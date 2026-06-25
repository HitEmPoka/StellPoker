const API_BASE = process.env.NEXT_PUBLIC_COORDINATOR_URL || "http://localhost:8080";
const INSECURE_AUTH_ENV = process.env.NEXT_PUBLIC_ALLOW_INSECURE_DEV_AUTH;
export const COORDINATOR_API_BASE = API_BASE;

function parseEnvBool(value: string | undefined): boolean | null {
  if (value === undefined) return null;
  const v = value.trim().toLowerCase();
  if (["1", "true", "yes", "on"].includes(v)) return true;
  if (["0", "false", "no", "off"].includes(v)) return false;
  return null;
}

const USE_INSECURE_DEV_AUTH = parseEnvBool(INSECURE_AUTH_ENV) ?? false;

export interface DealResponse {
  status: string;
  deck_root: string;
  hand_commitments: string[];
  proof_size: number;
  session_id: string;
  tx_hash: string | null;
}

export interface RevealResponse {
  status: string;
  cards: number[];
  proof_size: number;
  session_id: string;
  tx_hash: string | null;
}

export interface ShowdownResponse {
  status: string;
  winner: string;
  winner_index: number;
  proof_size: number;
  session_id: string;
  tx_hash: string | null;
}

export interface PlayerActionResponse {
  status: string;
  action: string;
  amount: number | null;
  player: string;
  tx_hash: string | null;
}

export interface TableStateResponse {
  state: string;
}

export interface ParsedTableStateResponse {
  raw: string;
  parsed: Record<string, unknown> | null;
}

export interface PlayerCardsResponse {
  card1: number;
  card2: number;
  salt1: string;
  salt2: string;
}

export interface CommitteeStatusResponse {
  nodes: number;
  healthy: boolean[];
  status: string;
}

export interface ChainConfigResponse {
  rpc_url: string;
  network_passphrase: string;
  poker_table_contract: string;
}

export interface CreateTableResponse {
  table_id: number;
  max_players: number;
  joined_wallets: number;
}

export interface JoinTableResponse {
  table_id: number;
  seat_index: number;
  seat_address: string;
  joined_wallets: number;
  max_players: number;
}

export interface OpenTableInfo {
  table_id: number;
  phase: string;
  max_players: number;
  joined_wallets: number;
  open_wallet_slots: number;
}

export interface OpenTablesResponse {
  tables: OpenTableInfo[];
}

export interface LobbySeat {
  seat_index: number;
  chain_address: string;
  wallet_address: string | null;
}

export interface TableLobbyResponse {
  table_id: number;
  phase: string;
  max_players: number;
  seats: LobbySeat[];
  joined_wallets: number;
}

export interface AuthSigner {
  address: string;
  signMessage: (message: string) => Promise<string>;
}

let lastNonce = 0;

async function readApiError(res: Response, fallback: string): Promise<string> {
  try {
    const text = await res.text();
    if (!text) return fallback;
    try {
      const json = JSON.parse(text) as { error?: string; message?: string };
      return json.error || json.message || text;
    } catch {
      return text;
    }
  } catch {
    return fallback;
  }
}

function nextNonce(): string {
  const now = Date.now() * 1000;
  if (now > lastNonce) {
    lastNonce = now;
  } else {
    lastNonce += 1;
  }
  return String(lastNonce);
}

function buildAuthMessage(
  address: string,
  tableId: number,
  action: string,
  nonce: string,
  timestamp: number
): string {
  return `stellar-poker|${address}|${tableId}|${action}|${nonce}|${timestamp}`;
}

async function buildAuthHeaders(
  tableId: number,
  action: string,
  auth: AuthSigner
): Promise<Record<string, string>> {
  const nonce = nextNonce();
  const timestamp = Math.floor(Date.now() / 1000);
  const message = buildAuthMessage(auth.address, tableId, action, nonce, timestamp);
  const signature = await auth.signMessage(message);

  return {
    "x-player-address": auth.address,
    "x-auth-signature": signature,
    "x-auth-nonce": nonce,
    "x-auth-timestamp": String(timestamp),
  };
}

function buildInsecureHeaders(auth: AuthSigner): Record<string, string> {
  return {
    "x-player-address": auth.address,
  };
}

function withMergedHeaders(
  init: RequestInit,
  extra: Record<string, string>
): RequestInit {
  const merged = new Headers(init.headers);
  for (const [key, value] of Object.entries(extra)) {
    merged.set(key, value);
  }
  return {
    ...init,
    headers: merged,
  };
}

async function authedFetch(
  url: string,
  init: RequestInit,
  tableId: number,
  action: string,
  auth: AuthSigner
): Promise<Response> {
  if (USE_INSECURE_DEV_AUTH) {
    const insecureAttempt = await fetch(
      url,
      withMergedHeaders(init, buildInsecureHeaders(auth))
    );
    if (insecureAttempt.status !== 401) {
      return insecureAttempt;
    }
  }

  const signedHeaders = await buildAuthHeaders(tableId, action, auth);
  return fetch(url, withMergedHeaders(init, signedHeaders));
}

export async function requestDeal(
  tableId: number,
  players: string[] = [],
  _auth: AuthSigner
): Promise<DealResponse> {
  const res = await fetch(
    `${API_BASE}/api/table/${tableId}/request-deal`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ players }),
    }
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Deal failed: ${res.status}`));
  }
  return res.json();
}

export async function createTable(
  auth: AuthSigner,
  maxPlayers: number,
  solo = false,
  buyIn?: string
): Promise<CreateTableResponse> {
  const payload: {
    max_players: number;
    solo: boolean;
    buy_in?: string;
  } = {
    max_players: maxPlayers,
    solo,
  };
  if (buyIn) {
    payload.buy_in = buyIn;
  }

  const res = await authedFetch(
    `${API_BASE}/api/tables/create`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    },
    0,
    "create_table",
    auth
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Create table failed: ${res.status}`));
  }
  return res.json();
}

export async function joinTable(
  tableId: number,
  auth: AuthSigner
): Promise<JoinTableResponse> {
  const res = await authedFetch(
    `${API_BASE}/api/table/${tableId}/join`,
    {
      method: "POST",
    },
    tableId,
    "join_table",
    auth
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Join table failed: ${res.status}`));
  }
  return res.json();
}

export async function listOpenTables(): Promise<OpenTablesResponse> {
  const res = await fetch(`${API_BASE}/api/tables/open`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Open tables failed: ${res.status}`));
  }
  return res.json();
}

export async function getChainConfig(): Promise<ChainConfigResponse> {
  const res = await fetch(`${API_BASE}/api/chain-config`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Chain config failed: ${res.status}`));
  }
  return res.json();
}

export async function getTableLobby(
  tableId: number
): Promise<TableLobbyResponse> {
  const res = await fetch(`${API_BASE}/api/table/${tableId}/lobby`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Lobby lookup failed: ${res.status}`));
  }
  return res.json();
}

export async function requestReveal(
  tableId: number,
  phase: "flop" | "turn" | "river",
  _auth: AuthSigner
): Promise<RevealResponse> {
  const res = await fetch(
    `${API_BASE}/api/table/${tableId}/request-reveal/${phase}`,
    {
      method: "POST",
    }
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Reveal failed: ${res.status}`));
  }
  return res.json();
}

export async function requestShowdown(
  tableId: number,
  _auth: AuthSigner
): Promise<ShowdownResponse> {
  const res = await fetch(
    `${API_BASE}/api/table/${tableId}/request-showdown`,
    {
      method: "POST",
    }
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Showdown failed: ${res.status}`));
  }
  return res.json();
}

export async function playerAction(
  tableId: number,
  action: "fold" | "check" | "call" | "bet" | "raise" | "allin",
  amount: number | undefined,
  auth: AuthSigner
): Promise<PlayerActionResponse> {
  const res = await authedFetch(
    `${API_BASE}/api/table/${tableId}/player-action`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ action, amount }),
    },
    tableId,
    `player_action:${action}`,
    auth
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Player action failed: ${res.status}`));
  }
  return res.json();
}

export async function getPlayerCards(
  tableId: number,
  address: string,
  auth: AuthSigner
): Promise<PlayerCardsResponse> {
  const res = await authedFetch(
    `${API_BASE}/api/table/${tableId}/player/${address}/cards`,
    {},
    tableId,
    "get_player_cards",
    auth
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Failed to get cards: ${res.status}`));
  }
  return res.json();
}

export async function getTableState(
  tableId: number
): Promise<TableStateResponse> {
  const res = await fetch(`${API_BASE}/api/table/${tableId}/state`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Failed to get table state: ${res.status}`));
  }
  return res.json();
}

export async function getParsedTableState(
  tableId: number
): Promise<ParsedTableStateResponse> {
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

export async function getCommitteeStatus(): Promise<CommitteeStatusResponse> {
  const res = await fetch(`${API_BASE}/api/committee/status`);
  if (!res.ok) throw new Error(`Failed to get status: ${res.status}`);
  return res.json();
}

// ── Stats ────────────────────────────────────────────────────────────────────

export interface GlobalStats {
  hands_played: number;
  biggest_pot: number;
  total_players_joined: number;
}

export interface PlayerStats {
  address: string;
  hands_played: number;
  hands_won: number;
  biggest_pot_won: number;
}

export interface StatsResponse {
  global: GlobalStats;
  leaderboard: PlayerStats[];
  cached_at: number;
}

export async function getStats(): Promise<StatsResponse> {
  const res = await fetch(`${API_BASE}/api/stats`);
  if (!res.ok) throw new Error(`Failed to get stats: ${res.status}`);
  return res.json();
}

export interface WalletChallengeResponse {
  challenge: string;
}

export interface WalletVerifyResponse {
  verified: boolean;
}

export async function getWalletChallenge(address: string): Promise<WalletChallengeResponse> {
  const res = await fetch(`${API_BASE}/api/wallet/challenge`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ address }),
  });
  if (!res.ok) {
    throw new Error(await readApiError(res, `Failed to get wallet challenge`));
  }
  return res.json();
}

export async function verifyWalletChallenge(
  address: string,
  challenge: string,
  signature: string
): Promise<WalletVerifyResponse> {
  const res = await fetch(`${API_BASE}/api/wallet/verify`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ address, challenge, signature }),
  });
  if (!res.ok) {
    throw new Error(await readApiError(res, `Wallet verification failed`));
  }
  return res.json();
}
