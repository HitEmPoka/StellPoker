import { describe, it, expect } from "vitest";
import { Keypair } from "@stellar/stellar-sdk";
import {
  AuthenticatedCardDelivery,
  bytesToHex,
  computeCardDeliverySigningBytes,
  deriveSessionPubkey,
  verifyAndDecryptCardDelivery,
} from "../lib/authenticated-cards";

async function createTestDelivery(
  recipientSessionPubkey: Uint8Array,
  tableId: number,
  recipientAddress: string,
  card1: number,
  card2: number,
  salt1: string,
  salt2: string,
  committeeKeypair: Keypair
): Promise<AuthenticatedCardDelivery> {
  const ephemeral = globalThis.crypto.getRandomValues(new Uint8Array(32));
  const nonce = globalThis.crypto.getRandomValues(new Uint8Array(12));
  const encoder = new TextEncoder();

  const u32 = new Uint8Array(4);
  new DataView(u32.buffer).setUint32(0, tableId >>> 0, true);

  const prefix = encoder.encode("STELLPOKER_CARD_ENC_KEY_V1:");
  const addrBytes = encoder.encode(recipientAddress);
  const mat = new Uint8Array(
    prefix.length + recipientSessionPubkey.length + ephemeral.length + u32.length + addrBytes.length
  );
  let off = 0;
  for (const part of [prefix, recipientSessionPubkey, ephemeral, u32, addrBytes]) {
    mat.set(part, off);
    off += part.length;
  }
  const keyHash = await globalThis.crypto.subtle.digest("SHA-256", mat);
  const aesKey = await globalThis.crypto.subtle.importKey(
    "raw",
    keyHash,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt"]
  );

  const plaintext = encoder.encode(JSON.stringify({ card1, card2, salt1, salt2 }));
  const ciphertextBuf = await globalThis.crypto.subtle.encrypt(
    { name: "AES-GCM", iv: nonce },
    aesKey,
    plaintext
  );

  const ephemeralHex = bytesToHex(ephemeral);
  const nonceHex = bytesToHex(nonce);
  const ciphertextHex = bytesToHex(new Uint8Array(ciphertextBuf));

  const signingBytes = await computeCardDeliverySigningBytes(
    tableId,
    recipientAddress,
    ephemeralHex,
    nonceHex,
    ciphertextHex
  );
  const sig = committeeKeypair.sign(Buffer.from(signingBytes));

  return {
    table_id: tableId,
    recipient_address: recipientAddress,
    ephemeral_pubkey: ephemeralHex,
    nonce: nonceHex,
    ciphertext: ciphertextHex,
    committee_signature: bytesToHex(new Uint8Array(sig)),
    committee_pubkey: bytesToHex(new Uint8Array(committeeKeypair.rawPublicKey())),
  };
}

describe("authenticated private card delivery (#882)", () => {
  const committee = Keypair.random();
  const committeePub = new Uint8Array(committee.rawPublicKey());

  const secretA = new Uint8Array(32).fill(11);
  const secretB = new Uint8Array(32).fill(22);
  const addrA = "GA_PLAYER_A_TEST";
  const addrB = "GB_PLAYER_B_TEST";

  it("decrypts honest delivery addressed to the player", async () => {
    const pubA = await deriveSessionPubkey(secretA);
    const delivery = await createTestDelivery(pubA, 1, addrA, 14, 27, "s1", "s2", committee);

    const cards = await verifyAndDecryptCardDelivery(delivery, 1, addrA, secretA, committeePub);
    expect(cards).toEqual({ card1: 14, card2: 27, salt1: "s1", salt2: "s2" });
  });

  it("prevents a player from decrypting another player's delivery", async () => {
    const pubA = await deriveSessionPubkey(secretA);
    const deliveryA = await createTestDelivery(pubA, 1, addrA, 51, 50, "s1", "s2", committee);

    await expect(
      verifyAndDecryptCardDelivery(deliveryA, 1, addrA, secretB, committeePub)
    ).rejects.toThrow("Failed to decrypt card delivery");
  });

  it("rejects unsigned or forged deliveries", async () => {
    const pubA = await deriveSessionPubkey(secretA);
    const delivery = await createTestDelivery(pubA, 1, addrA, 10, 11, "s1", "s2", committee);

    const unsignedDelivery = { ...delivery, committee_signature: "" };
    await expect(
      verifyAndDecryptCardDelivery(unsignedDelivery, 1, addrA, secretA, committeePub)
    ).rejects.toThrow("Unsigned card delivery rejected");

    const forgedDelivery = { ...delivery, committee_signature: "00".repeat(64) };
    await expect(
      verifyAndDecryptCardDelivery(forgedDelivery, 1, addrA, secretA, committeePub)
    ).rejects.toThrow("Invalid committee signature");
  });

  it("rejects mis-addressed deliveries and malicious coordinator payload swaps", async () => {
    const pubA = await deriveSessionPubkey(secretA);
    const deliveryA = await createTestDelivery(pubA, 1, addrA, 12, 13, "sa1", "sa2", committee);

    // Attack 1: Malicious coordinator sends Player A's delivery directly to Player B
    await expect(
      verifyAndDecryptCardDelivery(deliveryA, 1, addrB, secretB, committeePub)
    ).rejects.toThrow("Mis-addressed card delivery");

    // Attack 2: Malicious coordinator rewrites recipient_address to Player B
    const swappedEnvelope = { ...deliveryA, recipient_address: addrB };
    await expect(
      verifyAndDecryptCardDelivery(swappedEnvelope, 1, addrB, secretB, committeePub)
    ).rejects.toThrow("Invalid committee signature");
  });
});
