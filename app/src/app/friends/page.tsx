"use client";

/**
 * Friends list and invite page (Issue #168).
 */

import { useEffect, useState } from "react";
import Link from "next/link";
import { PixelWorld } from "@/components/PixelWorld";
import { FriendsPanel } from "@/components/FriendsPanel";
import { trySilentReconnect, type WalletSession } from "@/lib/wallet";

export default function FriendsPage() {
  const [wallet, setWallet] = useState<WalletSession | null>(null);

  useEffect(() => {
    trySilentReconnect().then((s) => setWallet(s));
  }, []);

  return (
    <PixelWorld>
      <main className="min-h-screen flex flex-col items-center p-6 gap-6">
        <div className="w-full" style={{ maxWidth: 560 }}>
          <div className="flex items-center justify-between">
            <Link
              href="/"
              className="text-[9px]"
              style={{ color: "#c47d2e", textDecoration: "none", fontFamily: "'Press Start 2P', monospace" }}
            >
              ← HOME
            </Link>
            <div className="text-[10px]" style={{ color: "#f5e6c8" }}>
              FRIENDS
            </div>
            <div style={{ width: 40 }} />
          </div>
        </div>

        {!wallet ? (
          <div
            className="pixel-border px-6 py-6 text-center text-[9px]"
            style={{ borderColor: "#2a2a4a", background: "rgba(12,10,24,0.9)", color: "#95a5a6" }}
          >
            CONNECT A WALLET TO MANAGE FRIENDS AND TABLE INVITES
          </div>
        ) : (
          <FriendsPanel wallet={wallet} />
        )}
      </main>
    </PixelWorld>
  );
}
