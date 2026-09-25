const API_BASE = process.env.NEXT_PUBLIC_COORDINATOR_URL || "http://localhost:8080";
const INSECURE_AUTH_ENV = process.env.NEXT_PUBLIC_ALLOW_INSECURE_DEV_AUTH;
/** COORDINATOR API BASE.
 * @param value - value.
 * @returns boolean | null.
 * @remarks Calls the coordinator REST API.
 */
export const COORDINATOR_API_BASE = API_BASE;

function parseEnvBool(value: string | undefined): boolean | null {
  if (value === undefined) return null;
  const v = value.trim().toLowerCase();
  if (["1", "true", "yes", "on"].includes(v)) return true;
  if (["0", "false", "no", "off"].includes(v)) return false;
  return null;
}

const USE_INSECURE_DEV_AUTH = parseEnvBool(INSECURE_AUTH_ENV) ?? false;

/** Deal Response shape. */
export interface DealResponse {
  status: string;
  deck_root: string;
  hand_commitments: string[];
  proof_size: number;
  session_id: string;
  tx_hash: string | null;
}

/** Reveal Response shape. */
export interface RevealResponse {
  status: string;
  cards: number[];
  proof_size: number;
  session_id: string;
  tx_hash: string | null;
}

/** Showdown Response shape. */
export interface ShowdownResponse {
  status: string;
  winner: string;
  winner_index: number;
  proof_size: number;
  session_id: string;
  tx_hash: string | null;
}

/** Player Action Response shape. */
export interface PlayerActionResponse {
  status: string;
  action: string;
  amount: number | null;
  player: string;
  tx_hash: string | null;
}

/** Table State Response shape. */
export interface TableStateResponse {
  state: string;
}

/** Parsed Table State Response shape. */
export interface ParsedTableStateResponse {
  raw: string;
  parsed: Record<string, unknown> | null;
}

/** Push payload from `GET /api/table/:table_id/state/ws` (Issue #105). */
export interface GameStateEvent {
  table_id: number;
  phase: string;
  deck_root: string;
  board_indices: number[];
  dealt_indices: number[];
  deal_tx_hash: string | null;
  reveal_tx_hashes: Record<string, string>;
  showdown_tx_hash: string | null;
  onchain_state: string | null;
  /** Live anonymous spectators on this table (Issue #171). */
  spectator_count?: number;
}

/** Pushed on a table's game-state channel whenever its spectator count changes (Issue #171). */
export interface SpectatorCountEvent {
  type: "spectators";
  table_id: number;
  spectator_count: number;
}

/** Game State Socket Message shape. */
export type GameStateSocketMessage = GameStateEvent | SpectatorCountEvent;

/** Checks is Spectator Count Event.
 * @param msg - msg.
 * @returns msg is SpectatorCountEvent.
 * @remarks Calls the coordinator REST API.
 */
export function isSpectatorCountEvent(
  msg: GameStateSocketMessage
): msg is SpectatorCountEvent {
  return (msg as SpectatorCountEvent).type === "spectators";
}

/** Derives the coordinator's ws(s):// origin from its http(s):// base URL. */
export function coordinatorWsBase(): string {
  return COORDINATOR_API_BASE.replace(/^http:/, "ws:").replace(/^https:/, "wss:");
}

/** Game State Socket Handle shape. */
export interface GameStateSocketHandle {
  stop: () => void;
}

/**
 * Subscribes to real-time game state pushes for a table. Reconnects with a
 * fixed backoff on drop. Returns `null` if the browser has no WebSocket
 * support at all — callers should fall back to polling `getParsedTableState`
 * in that case (and generally keep a slower poll running regardless, as a
 * safety net for missed/dropped messages).
 */
export function subscribeGameState(
  tableId: number,
  onUpdate: (event: GameStateSocketMessage) => void,
  options: { spectate?: boolean } = {}
): GameStateSocketHandle | null {
  if (typeof WebSocket === "undefined") {
    return null;
  }

  let active = true;
  let ws: WebSocket | null = null;
  let reconnectTimeout: ReturnType<typeof setTimeout> | undefined;

  const connect = () => {
    if (!active) return;
    // Spectators connect to `/spectate/ws`, which carries the same public
    // snapshots but counts the connection towards the table's spectator
    // indicator (Issue #171). No wallet or auth is needed for either.
    const path = options.spectate ? "spectate/ws" : "state/ws";
    const wsUrl = `${coordinatorWsBase()}/api/table/${tableId}/${path}`;
    ws = new WebSocket(wsUrl);

    ws.onmessage = (event) => {
      try {
        onUpdate(JSON.parse(event.data) as GameStateSocketMessage);
      } catch {
        // Ignore malformed frames.
      }
    };

    ws.onclose = () => {
      if (active) {
        reconnectTimeout = setTimeout(connect, 3000);
      }
    };

    ws.onerror = () => {
      ws?.close();
    };
  };

  connect();

  return {
    stop: () => {
      active = false;
      clearTimeout(reconnectTimeout);
      ws?.close();
    },
  };
}

/** Player Cards Response shape. */
export interface PlayerCardsResponse {
  card1: number;
  card2: number;
  salt1: string;
  salt2: string;
}

/** Committee Status Response shape. */
export interface CommitteeStatusResponse {
  nodes: number;
  healthy: boolean[];
  status: string;
}

/** Chain Config Response shape. */
export interface ChainConfigResponse {
  rpc_url: string;
  network_passphrase: string;
  poker_table_contract: string;
  /** Player avatar registry contract address (Issue #153). Optional — older
   *  network deploys may not have it; frontend falls back to Identicon. */
  player_avatar_contract?: string;
}

/** Create Table Response shape. */
export interface CreateTableResponse {
  table_id: number;
  max_players: number;
  joined_wallets: number;
}

/** Join Table Response shape. */
export interface JoinTableResponse {
  table_id: number;
  seat_index: number;
  seat_address: string;
  joined_wallets: number;
  max_players: number;
}

/** Open Table Info shape. */
export interface OpenTableInfo {
  table_id: number;
  phase: string;
  max_players: number;
  joined_wallets: number;
  open_wallet_slots: number;
  /** Live anonymous spectators (Issue #171). Absent on older coordinators. */
  spectators?: number;
}

/** Open Tables Response shape. */
export interface OpenTablesResponse {
  tables: OpenTableInfo[];
}

/** Multi-table overview for mini-map (Issue #53). */
export interface TableOverviewInfo {
  table_id: number;
  phase: string;
  max_players: number;
  seated: number;
  total_chips: number;
  stacks: number[];
  /** Live anonymous spectators (Issue #171). Absent on older coordinators. */
  spectators?: number;
}

/** Table Overview Response shape. */
export interface TableOverviewResponse {
  tables: TableOverviewInfo[];
}

/** Per-player HUD stats for seat tooltip (Issue #55). */
export interface PlayerHudStats {
  address: string;
  hands_played: number;
  vpip: number;
  pfr: number;
  aggression_factor: number;
}

/** On-chain ELO rating entry (Issue #70). */
export interface RatingEntry {
  address: string;
  rating: number;
  hands_played: number;
  hands_won: number;
}

/** Rating Leaderboard Response shape. */
export interface RatingLeaderboardResponse {
  entries: RatingEntry[];
  min_hands: number;
  total: number;
}

/** Lobby Seat shape. */
export interface LobbySeat {
  seat_index: number;
  chain_address: string;
  wallet_address: string | null;
}

/** Table Lobby Response shape. */
export interface TableLobbyResponse {
  table_id: number;
  phase: string;
  max_players: number;
  seats: LobbySeat[];
  joined_wallets: number;
}

/** Auth Signer shape. */
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

/** Runs Deal.
 * @param tableId - table Id.
 * @param players - players.
 * @param _auth - auth.
 * @returns Promise<DealResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** Creates Table.
 * @param auth - auth.
 * @param maxPlayers - max Players.
 * @param solo - solo.
 * @param buyIn - buy In.
 * @param token - token.
 * @returns Promise<CreateTableResponse>.
 */
export async function createTable(
  auth: AuthSigner,
  maxPlayers: number,
  solo = false,
  buyIn?: string,
  token?: string | undefined
): Promise<CreateTableResponse> {
  const payload: {
    max_players: number;
    solo: boolean;
    buy_in?: string;
    token?: string | null;
  } = {
    max_players: maxPlayers,
    solo,
  };
  if (buyIn) {
    payload.buy_in = buyIn;
  }
  if (token) {
    payload.token = token;
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

/** Runs Table.
 * @param tableId - table Id.
 * @param auth - auth.
 * @returns Promise<JoinTableResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** List Open Tables.
 * @returns Promise<OpenTablesResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function listOpenTables(): Promise<OpenTablesResponse> {
  const res = await fetch(`${API_BASE}/api/tables/open`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Open tables failed: ${res.status}`));
  }
  return res.json();
}

/** Issue #53 — all known tables with seat counts and chip stacks. */
export async function listTableOverview(): Promise<TableOverviewResponse> {
  const res = await fetch(`${API_BASE}/api/tables/overview`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Table overview failed: ${res.status}`));
  }
  return res.json();
}

/** Issue #55 — VPIP / PFR / AF / hands for seat hover tooltip. */
export async function getPlayerHudStats(
  address: string
): Promise<PlayerHudStats> {
  const res = await fetch(
    `${API_BASE}/api/stats/player/${encodeURIComponent(address)}`
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `Player stats failed: ${res.status}`));
  }
  return res.json();
}

/** Issue #70 — on-chain ELO leaderboard (coordinator cache / contract read). */
export async function getRatingLeaderboard(
  offset = 0,
  limit = 20
): Promise<RatingLeaderboardResponse> {
  const res = await fetch(
    `${API_BASE}/api/ratings/leaderboard?offset=${offset}&limit=${limit}`
  );
  if (!res.ok) {
    throw new Error(
      await readApiError(res, `Rating leaderboard failed: ${res.status}`)
    );
  }
  return res.json();
}

/** Loads Chain Config.
 * @returns Promise<ChainConfigResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function getChainConfig(): Promise<ChainConfigResponse> {
  const res = await fetch(`${API_BASE}/api/chain-config`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Chain config failed: ${res.status}`));
  }
  return res.json();
}

/** Loads Table Lobby.
 * @param tableId - table Id.
 * @returns Promise<TableLobbyResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function getTableLobby(
  tableId: number
): Promise<TableLobbyResponse> {
  const res = await fetch(`${API_BASE}/api/table/${tableId}/lobby`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Lobby lookup failed: ${res.status}`));
  }
  return res.json();
}

/** Runs Reveal.
 * @param tableId - table Id.
 * @param phase - phase.
 * @param _auth - auth.
 * @returns Promise<RevealResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** Runs Showdown.
 * @param tableId - table Id.
 * @param _auth - auth.
 * @returns Promise<ShowdownResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** Runs Run It Twice.
 * @param tableId - table Id.
 * @param optIn - opt In.
 * @param auth - auth.
 * @returns Promise<.
 * @remarks Calls the coordinator REST API.
 */
export async function requestRunItTwice(
  tableId: number,
  optIn: boolean,
  auth: AuthSigner
): Promise<{ status: string }> {
  const res = await authedFetch(
    `${API_BASE}/api/table/${tableId}/rit-opt-in`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ opt_in: optIn }),
    },
    tableId,
    "rit_opt_in",
    auth
  );
  if (!res.ok) {
    throw new Error(await readApiError(res, `RIT opt-in failed: ${res.status}`));
  }
  return res.json();
}

/** Runs player Action.
 * @param tableId - table Id.
 * @param action - action.
 * @param amount - amount.
 * @param auth - auth.
 * @returns Promise<PlayerActionResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** Loads Player Cards.
 * @param tableId - table Id.
 * @param address - address.
 * @param auth - auth.
 * @returns Promise<PlayerCardsResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** Loads Table State.
 * @param tableId - table Id.
 * @returns Promise<TableStateResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function getTableState(
  tableId: number
): Promise<TableStateResponse> {
  const res = await fetch(`${API_BASE}/api/table/${tableId}/state`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Failed to get table state: ${res.status}`));
  }
  return res.json();
}

/** Spectator Count Response shape. */
export interface SpectatorCountResponse {
  table_id: number;
  spectator_count: number;
}

/** Public, unauthenticated spectator count for a table (Issue #171). */
export async function getSpectatorCount(
  tableId: number
): Promise<SpectatorCountResponse> {
  const res = await fetch(`${API_BASE}/api/table/${tableId}/spectators`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `Failed to get spectators: ${res.status}`));
  }
  return res.json();
}

/** Loads Parsed Table State.
 * @param tableId - table Id.
 * @returns Promise<ParsedTableStateResponse>.
 */
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

/** Loads Committee Status.
 * @returns Promise<CommitteeStatusResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function getCommitteeStatus(): Promise<CommitteeStatusResponse> {
  const res = await fetch(`${API_BASE}/api/committee/status`);
  if (!res.ok) throw new Error(`Failed to get status: ${res.status}`);
  return res.json();
}

// ── Stats ────────────────────────────────────────────────────────────────────

/** Global Stats shape. */
export interface GlobalStats {
  hands_played: number;
  biggest_pot: number;
  total_players_joined: number;
}

/** Player Stats shape. */
export interface PlayerStats {
  address: string;
  hands_played: number;
  hands_won: number;
  biggest_pot_won: number;
}

/** Stats Response shape. */
export interface StatsResponse {
  global: GlobalStats;
  leaderboard: PlayerStats[];
  cached_at: number;
}

/** Loads Stats.
 * @returns Promise<StatsResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function getStats(): Promise<StatsResponse> {
  const res = await fetch(`${API_BASE}/api/stats`);
  if (!res.ok) throw new Error(`Failed to get stats: ${res.status}`);
  return res.json();
}

// ── MPC Node Status ──────────────────────────────────────────────────────────

/** Mpc Node Progress shape. */
export interface MpcNodeProgress {
  endpoint: string;
  phase: string;
  healthy: boolean;
  elapsed_secs: number;
}

/** Table Mpc Status Response shape. */
export interface TableMpcStatusResponse {
  table_id: number;
  phase: string;
  nodes: MpcNodeProgress[];
  active_sessions: number;
}

/** Loads Mpc Status.
 * @param tableId - table Id.
 * @returns Promise<TableMpcStatusResponse>.
 * @remarks Calls the coordinator REST API.
 */
export async function getMpcStatus(
  tableId: number
): Promise<TableMpcStatusResponse> {
  const res = await fetch(`${API_BASE}/api/table/${tableId}/mpc-status`);
  if (!res.ok) {
    throw new Error(await readApiError(res, `MPC status failed: ${res.status}`));
  }
  return res.json();
}

/** Wallet Challenge Response shape. */
export interface WalletChallengeResponse {
  challenge: string;
}

/** Wallet Verify Response shape. */
export interface WalletVerifyResponse {
  verified: boolean;
}

/** Loads Wallet Challenge.
 * @param address - address.
 * @returns Promise<WalletChallengeResponse>.
 * @remarks Calls the coordinator REST API.
 */
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

/** Verify Wallet Challenge.
 * @param address - address.
 * @param challenge - challenge.
 * @param signature - signature.
 * @returns Promise<WalletVerifyResponse>.
 * @remarks Calls the coordinator REST API.
 */
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
