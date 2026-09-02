/**
 * Client-side friend list and invite store (Issue #168).
 *
 * Friends are keyed by Stellar address and stored locally (persisted via
 * localStorage) plus an optional display alias. Online presence is derived
 * from the open-tables store: a friend is "online" when this browser has any
 * open table that seats them, or when their address appears in the lobby's
 * open tables. Table invites are queued in the notifications center.
 *
 * This is intentionally a thin, testable layer: it keeps the friend data and
 * pure helpers in one place so the UI and the notification center can share
 * them without duplication.
 */

import type { OpenTable } from "./open-tables";

const STORAGE_PREFIX = "stellpoker:friends:";
const MAX_ALIAS_LENGTH = 16;

export interface Friend {
  /** The friend's Stellar public key. */
  address: string;
  /** Optional display alias; falls back to the short address. */
  alias: string | null;
  /** True when the friend is currently present/online. */
  online: boolean;
  /** When ``true``, a pending table invite was sent to this friend. */
  invited: boolean;
}

/** An invite the local player has extended to a friend for a table. */
export interface TableInvite {
  address: string;
  tableId: number;
  sentAt: number;
}

function storageKey(): string {
  return STORAGE_PREFIX.slice(0, -1);
}

export function loadFriends(): Friend[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(storageKey());
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as Friend[]) : [];
  } catch {
    return [];
  }
}

function persist(friends: Friend[]): void {
  try {
    window.localStorage.setItem(storageKey(), JSON.stringify(friends));
  } catch {
    // Storage unavailable — friend list just won't persist.
  }
}

export function addFriend(address: string, alias?: string): Friend[] {
  const trimmed = (alias ?? "").trim().slice(0, MAX_ALIAS_LENGTH);
  const next = loadFriends();
  if (!next.some((f) => f.address === address)) {
    next.push({
      address,
      alias: trimmed.length ? trimmed : null,
      online: false,
      invited: false,
    });
  }
  persist(next);
  return next;
}

export function removeFriend(address: string): Friend[] {
  const next = loadFriends().filter((f) => f.address !== address);
  persist(next);
  return next;
}

export function setFriendAlias(address: string, alias: string): Friend[] {
  const next = loadFriends().map((f) =>
    f.address === address
      ? { ...f, alias: alias.trim().slice(0, MAX_ALIAS_LENGTH) || null }
      : f
  );
  persist(next);
  return next;
}

export function clearInvites(address: string, tableId: number): Friend[] {
  const next = loadFriends().map((f) =>
    f.address === address ? { ...f, invited: false } : f
  );
  persist(next);
  return next;
}

/**
 * Mark a friend as present/online or offline based on whether any currently
 * open table seats them.
 */
export function markFriendPresence(
  address: string,
  online: boolean
): Friend[] {
  const next = loadFriends().map((f) =>
    f.address === address ? { ...f, online } : f
  );
  persist(next);
  return next;
}

export function setFriendInvited(address: string, invited: boolean): Friend[] {
  const next = loadFriends().map((f) =>
    f.address === address ? { ...f, invited } : f
  );
  persist(next);
  return next;
}

/** Which of the given friends are seated at the currently open tables. */
export function computeOnlineAddresses(
  openTables: OpenTable[],
  seatedAt: (tableId: number) => string[]
): Set<string> {
  const online = new Set<string>();
  for (const table of openTables) {
    for (const addr of seatedAt(table.tableId)) {
      online.add(addr);
    }
  }
  return online;
}

/** The set of tables each online friend currently occupies. */
export function tablesOccupiedBy(
  openTables: OpenTable[],
  seatedAt: (tableId: number) => string[]
): Record<string, number[]> {
  const map: Record<string, number[]> = {};
  for (const table of openTables) {
    const seated = seatedAt(table.tableId);
    for (const addr of seated) {
      (map[addr] ??= []).push(table.tableId);
    }
  }
  return map;
}

export function shortAddr(addr: string): string {
  if (!addr || addr.length < 12) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

export function displayName(friend: Friend): string {
  return friend.alias ?? shortAddr(friend.address);
}
