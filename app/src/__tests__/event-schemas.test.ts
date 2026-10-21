import { describe, it, expect } from "vitest";
import { Keypair, nativeToScVal, xdr } from "@stellar/stellar-sdk";
import { decodePokerTableEvent, POKER_TABLE_EVENT_NAMES } from "@/lib/event-schemas";

const sym = (s: string) => xdr.ScVal.scvSymbol(s);
const u32 = (n: number) => nativeToScVal(n, { type: "u32" });
const i128 = (n: bigint) => nativeToScVal(n, { type: "i128" });
const player = Keypair.random().publicKey();
const addr = () => nativeToScVal(player, { type: "address" });

describe("decodePokerTableEvent", () => {
  it("decodes player_action into canonical JSON", () => {
    const event = decodePokerTableEvent(
      [sym("player_action"), u32(3), sym("raise")],
      xdr.ScVal.scvVec([addr(), i128(-42n)])
    );
    expect(event).toEqual({
      name: "player_action",
      topics: ["player_action", 3, "raise"],
      data: [player, "-42"],
    });
  });

  it("unwraps the phase enum", () => {
    const event = decodePokerTableEvent(
      [sym("phase_change"), u32(0)],
      xdr.ScVal.scvVec([sym("Flop")])
    );
    expect(event?.data).toBe("Flop");
  });

  it("hex encodes deal commitments", () => {
    const commitment = xdr.ScVal.scvBytes(Buffer.alloc(32, 0xab));
    const event = decodePokerTableEvent(
      [sym("deal_committed"), u32(0)],
      xdr.ScVal.scvVec([u32(1), xdr.ScVal.scvVec([commitment])])
    );
    expect(event?.data).toEqual([1, ["ab".repeat(32)]]);
  });

  it("accepts both rake_collected shapes and rejects others", () => {
    const topics = [sym("rake_collected"), u32(0)];
    const five = xdr.ScVal.scvVec([u32(1), i128(1n), i128(2n), i128(3n), i128(4n)]);
    const three = xdr.ScVal.scvVec([u32(1), i128(1n), i128(3n)]);
    const four = xdr.ScVal.scvVec([u32(1), i128(1n), i128(3n), i128(3n)]);
    expect(decodePokerTableEvent(topics, five)?.data).toEqual([1, "1", "2", "3", "4"]);
    expect(decodePokerTableEvent(topics, three)?.data).toEqual([1, "1", "3"]);
    expect(() => decodePokerTableEvent(topics, four)).toThrow();
  });

  it("reads base64 XDR and ignores events without a schema", () => {
    const topics = [sym("hand_started").toXDR("base64"), u32(7).toXDR("base64")];
    expect(decodePokerTableEvent(topics, u32(12).toXDR("base64"))?.data).toBe(12);
    expect(decodePokerTableEvent([sym("table_paused"), u32(0)], addr())).toBeNull();
  });

  it("lists one name per schema file", () => {
    expect(POKER_TABLE_EVENT_NAMES).toHaveLength(11);
  });
});
