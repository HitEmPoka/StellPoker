/**
 * Off-chain avatar cache (Issue #153).
 *
 * Reads from the `player-avatar` Soroban contract and caches results
 * aggressively so rendering a table full of players doesn't hammer the RPC
 * node on every tick. Two-layer cache:
 *   1. In-memory Map keyed by address — lives for the tab session.
 *   2. localStorage — survives reloads, invalidated when the on-chain
 *      `updated_at` timestamp advances.
 *
 * Callers get a synchronous read path via `getCachedAvatar()` plus an async
 * refresh path via `refreshAvatar()`. The `<Avatar />` component wires these
 * together automatically.
 */

import { Address, Contract, rpc, scValToNative } from "@stellar/stellar-sdk";
import { getChainConfig } from "./api";

export type AvatarKind = "none" | "svg" | "nft";

export interface SvgAvatarData {
  kind: "svg";
  templateId: number;
  params: Record<string, string>;
  updatedAt: number;
}

export interface NftAvatarData {
  kind: "nft";
  nftContract: string;
  tokenId: number;
  /** Resolved image URL (filled in by the NFT metadata lookup). */
  imageUrl?: string;
  updatedAt: number;
}

export interface NoAvatarData {
  kind: "none";
  updatedAt: number;
}

export type CachedAvatar = SvgAvatarData | NftAvatarData | NoAvatarData;

const STORAGE_PREFIX = "stellpoker:avatar:";
/** Age (ms) after which we proactively re-fetch even if we have a cached copy. */
const STALE_MS = 5 * 60 * 1000;

const memoryCache = new Map<string, { entry: CachedAvatar; fetchedAt: number }>();

function storageKey(address: string): string {
  return `${STORAGE_PREFIX}${address}`;
}

function parseParamsString(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  if (!raw) return out;
  for (const part of raw.split(",")) {
    const eq = part.indexOf("=");
    if (eq > 0) {
      const k = part.slice(0, eq).trim();
      const v = part.slice(eq + 1).trim();
      if (k) out[k] = v;
    }
  }
  return out;
}

function serializeParams(params: Record<string, string>): string {
  return Object.entries(params)
    .map(([k, v]) => `${k}=${v}`)
    .join(",");
}

/**
 * Synchronous read: returns the cached avatar (memory or disk) if available,
 * otherwise `null`. Does NOT trigger a network fetch.
 */
export function getCachedAvatar(address: string): CachedAvatar | null {
  if (!address) return null;
  const fromMem = memoryCache.get(address);
  if (fromMem) return fromMem.entry;
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(storageKey(address));
    if (!raw) return null;
    const parsed = JSON.parse(raw) as CachedAvatar;
    memoryCache.set(address, { entry: parsed, fetchedAt: Date.now() });
    return parsed;
  } catch {
    return null;
  }
}

function persist(address: string, entry: CachedAvatar) {
  memoryCache.set(address, { entry, fetchedAt: Date.now() });
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(storageKey(address), JSON.stringify(entry));
  } catch {
    // quota / private mode — keep mem-only cache, that's fine.
  }
}

/**
 * True if we have a cache entry younger than STALE_MS.
 * Used by callers to decide whether to schedule a background refresh.
 */
export function isAvatarFresh(address: string): boolean {
  const fromMem = memoryCache.get(address);
  if (fromMem && Date.now() - fromMem.fetchedAt < STALE_MS) return true;
  // Disk cache has no explicit fetchedAt timestamp; consider it fresh for
  // the current tick (caller will still check updatedAt against chain).
  if (typeof window !== "undefined") {
    try {
      if (window.localStorage.getItem(storageKey(address))) return true;
    } catch {
      // noop
    }
  }
  return false;
}

let cachedRpc:
  | { server: rpc.Server; contract: Contract; networkPassphrase: string }
  | null = null;

async function getRpc() {
  if (cachedRpc) return cachedRpc;
  const cfg = await getChainConfig();
  const server = new rpc.Server(cfg.rpc_url, {
    allowHttp: cfg.rpc_url.startsWith("http://"),
  });
  const contract = new Contract(cfg.player_avatar_contract ?? "");
  cachedRpc = { server, contract, networkPassphrase: cfg.network_passphrase };
  return cachedRpc;
}

/**
 * Parse a raw on-chain avatar ScVal (from `get_avatar`) into our typed form.
 * Returns `null` when nothing is on-chain (frontend uses Identicon fallback).
 */
function parseOnChain(raw: unknown): CachedAvatar | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as { kind?: unknown; updated_at?: unknown };
  const updatedAt =
    typeof r.updated_at === "number"
      ? r.updated_at
      : typeof r.updated_at === "string"
        ? parseInt(r.updated_at, 10) || Date.now()
        : Math.floor(Date.now() / 1000);

  if (!r.kind || !Array.isArray(r.kind)) {
    return { kind: "none", updatedAt };
  }
  const tag = r.kind[0];
  const payload = r.kind[1] as Record<string, unknown> | undefined;

  if (tag === "None") return { kind: "none", updatedAt };
  if (tag === "Svg" && payload) {
    const templateId =
      typeof payload.template_id === "number"
        ? payload.template_id
        : parseInt(String(payload.template_id ?? "0"), 10);
    const paramsRaw = String(payload.params ?? "");
    return {
      kind: "svg",
      templateId,
      params: parseParamsString(paramsRaw),
      updatedAt,
    };
  }
  if (tag === "Nft" && payload) {
    const nftContract = String(payload.nft_contract ?? "");
    const tokenId =
      typeof payload.token_id === "number"
        ? payload.token_id
        : parseInt(String(payload.token_id ?? "0"), 10);
    return {
      kind: "nft",
      nftContract,
      tokenId,
      updatedAt,
    };
  }
  return { kind: "none", updatedAt };
}

/**
 * Fetch the avatar from the player-avatar contract and update caches.
 * Returns the fresh entry, or `null` if the contract isn't configured.
 */
export async function refreshAvatar(
  address: string,
  opts: { force?: boolean } = {}
): Promise<CachedAvatar | null> {
  if (!address) return null;
  const cached = memoryCache.get(address);
  if (!opts.force && cached && Date.now() - cached.fetchedAt < STALE_MS) {
    return cached.entry;
  }
  try {
    const rpc = await getRpc();
    if (!rpc.contract.address()) return null;
    const addrArg = new Address(address).toScVal();
    const res = await rpc.server.callContract(
      rpc.contract.call("get_avatar", addrArg).toXDR("base64"),
      rpc.networkPassphrase
    );
    const parsed = scValToNative(
      rpc.Server.xdr.ScVal.fromXDR(res.resultXdr, "base64")
    );
    const avatar = parseOnChain(parsed as unknown);
    if (!avatar) return null;
    persist(address, avatar);
    return avatar;
  } catch {
    // Contract not deployed, network down, etc. — keep existing cache if any.
    return memoryCache.get(address)?.entry ?? null;
  }
}

/**
 * Batch-refresh a list of addresses using the contract's `get_avatars` helper.
 * Caches each entry individually. Missing / failed entries are skipped.
 */
export async function refreshAvatars(
  addresses: string[]
): Promise<Map<string, CachedAvatar>> {
  const out = new Map<string, CachedAvatar>();
  const unique = Array.from(new Set(addresses.filter(Boolean)));
  if (unique.length === 0) return out;
  try {
    const rpc = await getRpc();
    if (!rpc.contract.address()) return out;
    const addrsArg = (unique as unknown[]).map((a) => new Address(a as string).toScVal());
    const vecXdr = rpc.Server.xdr.ScVal.scvVec(addrsArg);
    const tx = rpc.contract
      .address()
      ? rpc.contract.call("get_avatars", (unique as unknown[]).map((a) => new Address(a as string).toScVal()) as unknown as never)
      : null;
    if (!tx) return out;
    const res = await rpc.server.callContract(
      tx.toXDR("base64"),
      rpc.networkPassphrase
    );
    const parsed = scValToNative(
      rpc.Server.xdr.ScVal.fromXDR(res.resultXdr, "base64")
    ) as unknown[];
    parsed.forEach((entry, i) => {
      const av = parseOnChain(entry);
      if (av) {
        persist(unique[i], av);
        out.set(unique[i], av);
      }
    });
    return out;
  } catch {
    // Fall back to per-address refreshes (slower but works when
    // `get_avatars` isn't available on an older contract deploy).
    for (const a of unique) {
      const av = await refreshAvatar(a, opts_force_false());
      if (av) out.set(a, av);
    }
    return out;
  }
}

function opts_force_false() {
  return { force: false } as const;
}

/**
 * Invalidate a cache entry — call it after the user submits a new avatar
 * tx so the next read picks up the chain state.
 */
export function invalidateAvatar(address: string) {
  memoryCache.delete(address);
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(storageKey(address));
  } catch {
    // noop
  }
}

/**
 * Build the on-chain `params` string that `set_svg_avatar` expects from a
 * structured dict. Used by the avatar picker before submitting the tx.
 */
export function buildSvgParams(params: Record<string, string>): string {
  return serializeParams(params);
}
