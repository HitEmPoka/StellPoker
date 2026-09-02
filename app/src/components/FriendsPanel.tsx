"use client";

/**
 * Friend list and invite panel (Issue #168). Lets the local player add
 * friends by Stellar address (or alias), shows their online status, throws a
 * table invite, and surfaces which tables friends currently occupy.
 */

import { useEffect, useState } from "react";
import {
  loadFriends,
  addFriend,
  removeFriend,
  setFriendInvited,
  computeOnlineAddresses,
  tablesOccupiedBy,
  displayName,
  shortAddr,
  type Friend,
} from "@/lib/friends";
import { loadOpenTables, type OpenTable } from "@/lib/open-tables";
import { pushNotification } from "@/lib/notifications-center";
import type { WalletSession } from "@/lib/wallet";

const STELLAR_ADDR_RE = /^G[A-Z2-7]{55}$/;

interface Props {
  wallet: WalletSession | null;
  /** Live seat lists by table id, for computing friend presence (#168). */
  tables?: OpenTable[];
  seatedAt?: (tableId: number) => string[];
}

export function FriendsPanel({ wallet, tables, seatedAt }: Props) {
  const [friends, setFriends] = useState<Friend[]>([]);
  const [openTables, setOpenTables] = useState<OpenTable[]>([]);
  const [addressInput, setAddressInput] = useState("");
  const [aliasInput, setAliasInput] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setFriends(loadFriends());
    if (wallet) setOpenTables(loadOpenTables(wallet.address));
  }, [wallet]);

  const liveTables = tables ?? openTables;
  const seatedResolver = seatedAt ?? (() => [] as string[]);
  const online = computeOnlineAddresses(liveTables, seatedResolver);
  const occupied = tablesOccupiedBy(liveTables, seatedResolver);

  const handleAdd = () => {
    const addr = addressInput.trim().toUpperCase();
    if (!STELLAR_ADDR_RE.test(addr)) {
      setError("Enter a valid Stellar address (starts with G).");
      return;
    }
    setError(null);
    const next = addFriend(addr, aliasInput);
    setFriends(next);
    setAliasInput("");
    setAddressInput("");
  };

  const handleRemove = (addr: string) => {
    setFriends(removeFriend(addr));
  };

  const handleInvite = (addr: string, tableId: number) => {
    setFriends(setFriendInvited(addr, true));
    pushNotification({
      type: "table-invite",
      title: "Table invite sent",
      body: `Invited ${shortAddr(addr)} to table #${tableId}`,
      tableId,
      friend: addr,
    });
  };

  return (
    <div
      className="pixel-border p-4 flex flex-col gap-3"
      style={{ background: "rgba(12,10,24,0.92)", borderColor: "#2a2a4a" }}
      data-testid="friends-panel"
    >
      <div className="flex items-center justify-between">
        <span className="text-[10px]" style={{ color: "#f5e6c8" }}>
          FRIENDS
        </span>
        <span className="text-[8px]" style={{ color: "#95a5a6" }}>
          {friends.filter((f) => f.online).length}/{friends.length} ONLINE
        </span>
      </div>

      {/* Add friend */}
      <div className="flex flex-col gap-1">
        <label htmlFor="friend-address" className="text-[8px]" style={{ color: "#95a5a6" }}>
          ADD FRIEND BY STELLAR ADDRESS OR ALIAS
        </label>
        <div className="flex gap-1">
          <input
            id="friend-address"
            value={addressInput}
            onChange={(e) => setAddressInput(e.target.value)}
            placeholder="GABC…"
            className="pixel-border px-2 py-1 text-[8px] flex-1"
            style={{ background: "rgba(255,255,255,0.05)", color: "#f5e6c8", borderColor: "#4a4a6a" }}
          />
          <input
            value={aliasInput}
            onChange={(e) => setAliasInput(e.target.value)}
            placeholder="ALIAS"
            className="pixel-border px-2 py-1 text-[8px] w-20"
            style={{ background: "rgba(255,255,255,0.05)", color: "#f5e6c8", borderColor: "#4a4a6a" }}
          />
          <button
            onClick={handleAdd}
            className="pixel-btn text-[8px]"
            style={{ padding: "4px 10px", background: "#27ae60", color: "white" }}
            aria-label="Add friend"
          >
            ADD
          </button>
        </div>
        {error && (
          <div className="text-[7px]" style={{ color: "#e74c3c" }} role="alert">
            {error}
          </div>
        )}
      </div>

      {/* Friend list */}
      {friends.length === 0 ? (
        <div className="text-[8px] text-center py-2" style={{ color: "#7f8c8d" }}>
          NO FRIENDS YET. ADD SOME ABOVE.
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {friends.map((f) => {
            const isOnline = online.has(f.address);
            const occupiedTables = occupied[f.address] ?? [];
            return (
              <div
                key={f.address}
                className="flex items-center justify-between gap-1 px-2 py-1"
                style={{
                  background: isOnline ? "rgba(39,174,96,0.12)" : "rgba(0,0,0,0.2)",
                  borderLeft: isOnline ? "2px solid #27ae60" : "2px solid #4a4a6a",
                }}
                data-testid="friend-row"
                data-online={isOnline}
              >
                <div className="flex flex-col">
                  <span className="text-[8px]" style={{ color: "#f5e6c8" }} title={f.address}>
                    {displayName(f)}
                  </span>
                  <span className="text-[7px]" style={{ color: isOnline ? "#27ae60" : "#7f8c8d" }}>
                    {isOnline
                      ? occupiedTables.length > 0
                        ? `● AT TABLE #${occupiedTables.join(", #")}`
                        : "● ONLINE"
                      : "○ OFFLINE"}
                  </span>
                </div>
                {liveTables.length > 0 && (
                  <select
                    aria-label={`Invite ${displayName(f)} to a table`}
                    defaultValue=""
                    onChange={(e) => {
                      if (e.target.value) handleInvite(f.address, Number(e.target.value));
                      e.target.value = "";
                    }}
                    className="pixel-border px-1 py-0.5 text-[7px]"
                    style={{ background: "rgba(12,10,24,0.9)", color: "#f5e6c8", borderColor: "#4a4a6a" }}
                  >
                    <option value="">INVITE →</option>
                    {liveTables.map((t) => (
                      <option key={t.tableId} value={t.tableId}>
                        Table #{t.tableId}
                      </option>
                    ))}
                  </select>
                )}
                <button
                  onClick={() => handleRemove(f.address)}
                  className="pixel-btn text-[7px]"
                  style={{ padding: "2px 6px", background: "#2c3e50", color: "#e74c3c" }}
                  aria-label={`Remove friend ${displayName(f)}`}
                >
                  ✕
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Default export kept for potential dynamic import use. */
export default FriendsPanel;
