import {
  getAddress as freighterGetAddress,
  isConnected as freighterIsConnected,
  requestAccess as freighterRequestAccess,
  signMessage as freighterSignMessage,
} from "@stellar/freighter-api";

export interface WalletSession {
  address: string;
  signMessage: (message: string) => Promise<string>;
}

type FreighterAddressResponse =
  | string
  | {
      address?: string;
      publicKey?: string;
      error?: string;
    };

type FreighterSignResponse =
  | string
  | {
      signature?: string;
      signedMessage?: string;
      signed_message?: string;
      error?: string;
    };

type FreighterApi = {
  requestAccess?: () => Promise<unknown>;
  setAllowed?: () => Promise<unknown>;
  getAddress?: () => Promise<FreighterAddressResponse>;
  getPublicKey?: () => Promise<FreighterAddressResponse>;
  signMessage?: (
    message: string,
    opts?: { address?: string }
  ) => Promise<FreighterSignResponse>;
};

declare global {
  interface Window {
    freighter?: unknown;
    freighterApi?: FreighterApi;
    stellar?: {
      freighterApi?: FreighterApi;
    };
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

function parseAddress(result: FreighterAddressResponse): string {
  if (typeof result === "string" && result.length > 0) {
    return result;
  }
  if (typeof result === "object" && result !== null) {
    if (result.error) {
      throw new Error(errorMessage(result.error, "Freighter rejected address request"));
    }
    if (typeof result.address === "string" && result.address.length > 0) {
      return result.address;
    }
    if (typeof result.publicKey === "string" && result.publicKey.length > 0) {
      return result.publicKey;
    }
  }
  throw new Error("Freighter returned an invalid address response");
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

function parseSignedPayload(payload: unknown): string {
  if (typeof payload === "string" && payload.length > 0) {
    return payload;
  }
  if (ArrayBuffer.isView(payload)) {
    const bytes = new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
    return bytesToBase64(bytes);
  }
  if (payload instanceof ArrayBuffer) {
    return bytesToBase64(new Uint8Array(payload));
  }
  if (
    typeof payload === "object" &&
    payload !== null &&
    "data" in payload &&
    Array.isArray((payload as { data?: unknown }).data)
  ) {
    return bytesToBase64(Uint8Array.from((payload as { data: number[] }).data));
  }
  throw new Error("Freighter returned an invalid signature payload");
}

function parseSignature(result: FreighterSignResponse): string {
  if (typeof result === "string" && result.length > 0) {
    return result;
  }
  if (typeof result === "object" && result !== null) {
    if (result.error) {
      throw new Error(errorMessage(result.error, "Freighter rejected sign request"));
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
  throw new Error("Freighter returned an invalid signature response");
}

function parseModernSignature(
  result:
    | {
        signedMessage: unknown;
        signerAddress: string;
        error?: unknown;
      }
    | {
        signedMessage: string | null;
        signerAddress: string;
        error?: unknown;
      }
): string {
  if (result.error) {
    throw new Error(errorMessage(result.error, "Freighter rejected sign request"));
  }
  return parseSignedPayload(result.signedMessage);
}

function getLegacyApiCandidate(): FreighterApi | null {
  if (typeof window === "undefined") {
    return null;
  }

  const candidates: unknown[] = [
    window.freighterApi,
    window.stellar?.freighterApi,
    typeof window.freighter === "object" ? window.freighter : null,
  ];

  for (const candidate of candidates) {
    if (!candidate || typeof candidate !== "object") {
      continue;
    }
    const api = candidate as FreighterApi;
    if (
      typeof api.requestAccess === "function" ||
      typeof api.setAllowed === "function" ||
      typeof api.getAddress === "function" ||
      typeof api.getPublicKey === "function" ||
      typeof api.signMessage === "function"
    ) {
      return api;
    }
  }
  return null;
}

async function waitForLegacyApi(timeoutMs = 3000): Promise<FreighterApi | null> {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const api = getLegacyApiCandidate();
    if (api) {
      return api;
    }
    await new Promise((resolve) => setTimeout(resolve, 120));
  }
  return getLegacyApiCandidate();
}

async function connectViaOfficialApi(): Promise<WalletSession | null> {
  if (typeof window === "undefined") {
    return null;
  }

  const connected = await freighterIsConnected();
  if (connected.error) {
    throw new Error(errorMessage(connected.error, "Failed to query Freighter connection state"));
  }
  if (!connected.isConnected) {
    return null;
  }

  const access = await freighterRequestAccess();
  if (access.error) {
    throw new Error(errorMessage(access.error, "Freighter access was denied"));
  }

  let address = access.address;
  if (!address) {
    const current = await freighterGetAddress();
    if (current.error) {
      throw new Error(errorMessage(current.error, "Failed to read Freighter address"));
    }
    address = current.address;
  }

  if (!address) {
    throw new Error("Freighter did not return an address");
  }

  return {
    address,
    signMessage: async (message: string) => {
      const result = await freighterSignMessage(message, { address });
      return parseModernSignature(result);
    },
  };
}

async function connectViaLegacyApi(): Promise<WalletSession | null> {
  const api = await waitForLegacyApi();
  if (!api) {
    return null;
  }

  if (api.requestAccess) {
    await api.requestAccess();
  } else if (api.setAllowed) {
    await api.setAllowed();
  }

  const getAddress = api.getAddress ?? api.getPublicKey;
  if (!getAddress) {
    throw new Error("Freighter getAddress API is unavailable");
  }
  const address = parseAddress(await getAddress.call(api));

  if (!api.signMessage) {
    throw new Error("Freighter signMessage API is unavailable");
  }

  return {
    address,
    signMessage: async (message: string): Promise<string> => {
      const sig = await api.signMessage!(message, { address });
      return parseSignature(sig);
    },
  };
}

// ── localStorage wallet persistence ──

const WALLET_KEY = "stellar_poker_wallet";

function saveWalletAddress(address: string): void {
  try {
    localStorage.setItem(WALLET_KEY, address);
  } catch {
    // Storage full or unavailable — ignore.
  }
}

export function getSavedWalletAddress(): string | null {
  try {
    return localStorage.getItem(WALLET_KEY);
  } catch {
    return null;
  }
}

export function clearSavedWallet(): void {
  try {
    localStorage.removeItem(WALLET_KEY);
  } catch {
    // ignore
  }
}

export async function connectFreighterWallet(): Promise<WalletSession> {
  try {
    const modern = await connectViaOfficialApi();
    if (modern) {
      saveWalletAddress(modern.address);
      return modern;
    }
  } catch (err) {
    const legacy = await connectViaLegacyApi();
    if (legacy) {
      saveWalletAddress(legacy.address);
      return legacy;
    }
    throw err;
  }

  const legacy = await connectViaLegacyApi();
  if (legacy) {
    saveWalletAddress(legacy.address);
    return legacy;
  }

  throw new Error(
    "Freighter wallet not found. Open Freighter, unlock it, and allow this site."
  );
}

/**
 * Silently reconnect if localStorage has a saved wallet address and
 * Freighter is already approved (no popup). Uses getAddress instead of
 * requestAccess to avoid triggering a Freighter approval popup.
 */
export async function trySilentReconnect(): Promise<WalletSession | null> {
  const saved = getSavedWalletAddress();
  if (!saved) return null;

  try {
    const connected = await freighterIsConnected();
    if (!connected.isConnected) {
      clearSavedWallet();
      return null;
    }

    // Use getAddress directly — avoids the requestAccess popup.
    // If the site was previously approved, this returns the address silently.
    const current = await freighterGetAddress();
    if (current.error || !current.address) {
      clearSavedWallet();
      return null;
    }

    const address = current.address;

    return {
      address,
      signMessage: async (message: string) => {
        const result = await freighterSignMessage(message, { address });
        return parseModernSignature(result);
      },
    };
  } catch {
    clearSavedWallet();
    return null;
  }
}

export async function getActiveAddress(): Promise<string | null> {
  try {
    const connected = await freighterIsConnected();
    if (!connected.isConnected) return null;
    const current = await freighterGetAddress();
    if (current.error || !current.address) return null;
    return current.address;
  } catch {
    return null;
  }
}
