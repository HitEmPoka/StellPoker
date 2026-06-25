import type { WalletSession } from "./wallet";

type LobstrAddressResponse =
  | string
  | {
      address?: string;
      publicKey?: string;
      error?: string;
    };

type LobstrSignResponse =
  | string
  | {
      signedMessage?: string;
      signature?: string;
      signed_message?: string;
      error?: string;
    };

type LobstrApi = {
  getAddress?: () => Promise<LobstrAddressResponse>;
  getPublicKey?: () => Promise<LobstrAddressResponse>;
  signMessage?: (message: string) => Promise<LobstrSignResponse>;
};

declare global {
  interface Window {
    lobstr?: LobstrApi;
  }
}

function errorMessage(raw: unknown, fallback: string): string {
  if (typeof raw === "string" && raw.trim()) {
    return raw;
  }
  if (
    typeof raw === "object" &&
    raw !== null &&
    "message" in raw &&
    typeof (raw as { message?: unknown }).message === "string"
  ) {
    return (raw as { message: string }).message;
  }
  return fallback;
}

function parseAddress(result: LobstrAddressResponse): string {
  if (typeof result === "string" && result.length > 0) {
    return result;
  }
  if (typeof result === "object" && result !== null) {
    if (result.error) {
      throw new Error(errorMessage(result.error, "Lobstr rejected address request"));
    }
    if (typeof result.address === "string" && result.address.length > 0) {
      return result.address;
    }
    if (typeof result.publicKey === "string" && result.publicKey.length > 0) {
      return result.publicKey;
    }
  }
  throw new Error("Lobstr returned an invalid address response");
}

function parseSignature(result: LobstrSignResponse): string {
  if (typeof result === "string" && result.length > 0) {
    return result;
  }
  if (typeof result === "object" && result !== null) {
    if (result.error) {
      throw new Error(errorMessage(result.error, "Lobstr rejected sign request"));
    }
    if (typeof result.signature === "string" && result.signature.length > 0) {
      return result.signature;
    }
    if (typeof result.signedMessage === "string" && result.signedMessage.length > 0) {
      return result.signedMessage;
    }
    if (typeof result.signed_message === "string" && result.signed_message.length > 0) {
      return result.signed_message;
    }
  }
  throw new Error("Lobstr returned an invalid signature response");
}

function getApi(): LobstrApi | null {
  if (typeof window === "undefined") return null;
  const api = window.lobstr;
  if (!api || typeof api !== "object") return null;
  if (
    typeof api.getAddress === "function" ||
    typeof api.getPublicKey === "function" ||
    typeof api.signMessage === "function"
  ) {
    return api;
  }
  return null;
}

export function isLobstrInstalled(): boolean {
  return getApi() !== null;
}

function saveWalletAddress(address: string): void {
  try {
    localStorage.setItem("stellar_poker_wallet_lobstr", address);
  } catch {
    // ignore
  }
}

function getSavedWalletAddress(): string | null {
  try {
    return localStorage.getItem("stellar_poker_wallet_lobstr");
  } catch {
    return null;
  }
}

export function clearSavedWallet(): void {
  try {
    localStorage.removeItem("stellar_poker_wallet_lobstr");
  } catch {
    // ignore
  }
}

export async function connectLobstrWallet(): Promise<WalletSession> {
  const api = getApi();
  if (!api) {
    throw new Error(
      "Lobstr wallet not found. Open Lobstr, unlock it, and allow this site."
    );
  }

  const getAddress = api.getAddress ?? api.getPublicKey;
  if (!getAddress) {
    throw new Error("Lobstr getAddress API is unavailable");
  }

  const address = parseAddress(await getAddress.call(api));

  if (!api.signMessage) {
    throw new Error("Lobstr signMessage API is unavailable");
  }

  saveWalletAddress(address);

  return {
    address,
    walletType: "lobstr",
    signMessage: async (message: string) => {
      const sig = await api.signMessage!(message);
      return parseSignature(sig);
    },
  };
}

export async function trySilentReconnectLobstr(): Promise<WalletSession | null> {
  const saved = getSavedWalletAddress();
  if (!saved) return null;

  try {
    const api = getApi();
    if (!api) {
      clearSavedWallet();
      return null;
    }

    const getAddress = api.getAddress ?? api.getPublicKey;
    if (!getAddress) {
      clearSavedWallet();
      return null;
    }

    const address = parseAddress(await getAddress.call(api));

    if (!api.signMessage) {
      clearSavedWallet();
      return null;
    }

    return {
      address,
      walletType: "lobstr",
      signMessage: async (message: string) => {
        const sig = await api.signMessage!(message);
        return parseSignature(sig);
      },
    };
  } catch {
    clearSavedWallet();
    return null;
  }
}
