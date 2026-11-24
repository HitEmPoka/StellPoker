import { Keypair } from "@stellar/stellar-sdk";

export interface AuthenticatedCardDelivery {
  table_id: number;
  recipient_address: string;
  ephemeral_pubkey: string;
  nonce: string;
  ciphertext: string;
  committee_signature: string;
  committee_pubkey: string;
}

export interface DecryptedHoleCards {
  card1: number;
  card2: number;
  salt1: string;
  salt2: string;
}

function getSubtle(): SubtleCrypto {
  if (typeof globalThis !== "undefined" && globalThis.crypto?.subtle) {
    return globalThis.crypto.subtle;
  }
  throw new Error("WebCrypto SubtleCrypto is unavailable in this environment");
}

export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(hex)) {
    throw new Error("Invalid hex string");
  }
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function u32ToLeBytes(n: number): Uint8Array {
  const buf = new ArrayBuffer(4);
  new DataView(buf).setUint32(0, n >>> 0, true);
  return new Uint8Array(buf);
}

function concatBytes(...arrays: Uint8Array[]): Uint8Array {
  const total = arrays.reduce((acc, a) => acc + a.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const a of arrays) {
    out.set(a, offset);
    offset += a.length;
  }
  return out;
}

export async function deriveSessionPubkey(secret: Uint8Array): Promise<Uint8Array> {
  const subtle = getSubtle();
  const encoder = new TextEncoder();
  const input = concatBytes(encoder.encode("STELLPOKER_SESSION_PK_V1:"), secret);
  const digest = await subtle.digest("SHA-256", input.buffer as ArrayBuffer);
  return new Uint8Array(digest);
}

export async function computeCardDeliverySigningBytes(
  tableId: number,
  recipientAddress: string,
  ephemeralPubkeyHex: string,
  nonceHex: string,
  ciphertextHex: string
): Promise<Uint8Array> {
  const subtle = getSubtle();
  const encoder = new TextEncoder();
  const payload = concatBytes(
    encoder.encode("STELLPOKER_CARD_DELIVERY_V1:"),
    u32ToLeBytes(tableId),
    encoder.encode(recipientAddress),
    encoder.encode(ephemeralPubkeyHex),
    encoder.encode(nonceHex),
    encoder.encode(ciphertextHex)
  );
  const digest = await subtle.digest("SHA-256", payload.buffer as ArrayBuffer);
  return new Uint8Array(digest);
}

async function deriveAesKey(
  recipientPubkey: Uint8Array,
  ephemeralBytes: Uint8Array,
  tableId: number,
  recipientAddress: string
): Promise<CryptoKey> {
  const subtle = getSubtle();
  const encoder = new TextEncoder();
  const keyMaterial = concatBytes(
    encoder.encode("STELLPOKER_CARD_ENC_KEY_V1:"),
    recipientPubkey,
    ephemeralBytes,
    u32ToLeBytes(tableId),
    encoder.encode(recipientAddress)
  );
  const keyHash = await subtle.digest("SHA-256", keyMaterial.buffer as ArrayBuffer);
  return subtle.importKey("raw", keyHash, { name: "AES-GCM", length: 256 }, false, [
    "encrypt",
    "decrypt",
  ]);
}

/**
 * Verifies the committee signature, checks recipient addressing, and decrypts
 * hole cards before rendering.
 *
 * Rejects:
 * - Unsigned or forged committee signatures
 * - Mis-addressed deliveries (recipient_address !== expectedAddress)
 * - Payloads encrypted for a different player's session key
 */
export async function verifyAndDecryptCardDelivery(
  delivery: AuthenticatedCardDelivery,
  expectedTableId: number,
  expectedAddress: string,
  playerSessionSecret: Uint8Array,
  expectedCommitteePubkey: Uint8Array
): Promise<DecryptedHoleCards> {
  if (!delivery || typeof delivery !== "object") {
    throw new Error("Invalid card delivery payload");
  }
  if (!delivery.committee_signature || !delivery.committee_pubkey) {
    throw new Error("Unsigned card delivery rejected");
  }
  if (delivery.table_id !== expectedTableId) {
    throw new Error(
      `Table ID mismatch: expected ${expectedTableId}, got ${delivery.table_id}`
    );
  }
  if (delivery.recipient_address !== expectedAddress) {
    throw new Error(
      `Mis-addressed card delivery: expected ${expectedAddress}, got ${delivery.recipient_address}`
    );
  }

  const deliveryPubkeyBytes = hexToBytes(delivery.committee_pubkey);
  if (
    deliveryPubkeyBytes.length !== expectedCommitteePubkey.length ||
    !deliveryPubkeyBytes.every((b, i) => b === expectedCommitteePubkey[i])
  ) {
    throw new Error("Committee public key mismatch");
  }

  const signingBytes = await computeCardDeliverySigningBytes(
    delivery.table_id,
    delivery.recipient_address,
    delivery.ephemeral_pubkey,
    delivery.nonce,
    delivery.ciphertext
  );

  const signatureBytes = hexToBytes(delivery.committee_signature);
  if (signatureBytes.length !== 64) {
    throw new Error("Invalid committee signature length");
  }

  const committeeKeypair = Keypair.fromRawEd25519PublicKey(
    Buffer.from(expectedCommitteePubkey)
  );
  const sigValid = committeeKeypair.verify(
    Buffer.from(signingBytes),
    Buffer.from(signatureBytes)
  );
  if (!sigValid) {
    throw new Error("Invalid committee signature on card delivery");
  }

  const recipientPub = await deriveSessionPubkey(playerSessionSecret);
  const ephemeralBytes = hexToBytes(delivery.ephemeral_pubkey);
  const nonceBytes = hexToBytes(delivery.nonce);
  const ciphertextBytes = hexToBytes(delivery.ciphertext);

  const aesKey = await deriveAesKey(
    recipientPub,
    ephemeralBytes,
    delivery.table_id,
    delivery.recipient_address
  );

  let plaintextBuf: ArrayBuffer;
  try {
    plaintextBuf = await getSubtle().decrypt(
      { name: "AES-GCM", iv: nonceBytes.buffer as ArrayBuffer },
      aesKey,
      ciphertextBytes.buffer as ArrayBuffer
    );
  } catch {
    throw new Error(
      "Failed to decrypt card delivery: invalid session key or tampered ciphertext"
    );
  }

  const decoded = new TextDecoder().decode(plaintextBuf);
  const parsed = JSON.parse(decoded) as DecryptedHoleCards;
  if (
    typeof parsed.card1 !== "number" ||
    typeof parsed.card2 !== "number" ||
    typeof parsed.salt1 !== "string" ||
    typeof parsed.salt2 !== "string"
  ) {
    throw new Error("Malformed decrypted card payload");
  }
  return parsed;
}
