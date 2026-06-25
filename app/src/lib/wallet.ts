export type WalletType = "freighter" | "lobstr";

export interface WalletSession {
  address: string;
  walletType: WalletType;
  signMessage: (message: string) => Promise<string>;
}

export interface WalletInfo {
  type: WalletType;
  name: string;
  isInstalled: boolean;
}

import {
  connectFreighterWallet as connectFreighter,
  trySilentReconnect as tryReconnectFreighter,
  isFreighterInstalled,
} from "./freighter";

import {
  connectLobstrWallet as connectLobstr,
  trySilentReconnectLobstr as tryReconnectLobstr,
  isLobstrInstalled,
} from "./lobstr";

const WALLET_META: Record<WalletType, { name: string }> = {
  freighter: { name: "Freighter" },
  lobstr: { name: "Lobstr" },
};

export function detectInstalledWallets(): WalletInfo[] {
  const results: WalletInfo[] = [];

  if (typeof window === "undefined") return results;

  if (isFreighterInstalled()) {
    results.push({
      type: "freighter",
      name: WALLET_META.freighter.name,
      isInstalled: true,
    });
  }

  if (isLobstrInstalled()) {
    results.push({
      type: "lobstr",
      name: WALLET_META.lobstr.name,
      isInstalled: true,
    });
  }

  if (results.length === 0) {
    results.push(
      { type: "freighter", name: WALLET_META.freighter.name, isInstalled: false },
      { type: "lobstr", name: WALLET_META.lobstr.name, isInstalled: false }
    );
  }

  return results;
}

export async function connectWallet(type: WalletType): Promise<WalletSession> {
  switch (type) {
    case "freighter":
      return connectFreighter();
    case "lobstr":
      return connectLobstr();
  }
}

export async function trySilentReconnect(): Promise<WalletSession | null> {
  const freighterSession = await tryReconnectFreighter();
  if (freighterSession) return freighterSession;

  const lobstrSession = await tryReconnectLobstr();
  if (lobstrSession) return lobstrSession;

  return null;
}

export function getWalletDisplayName(session: WalletSession): string {
  const meta = WALLET_META[session.walletType];
  return meta ? meta.name : session.walletType;
}
