import { describe, expect, it } from "vitest";
import { displaySeatName } from "@/lib/seat-privacy";

describe("seat privacy modes", () => {
  const player = { address: "GABCDEF123456789", alias: "Ace", seat: 1 };

  it("renders the configured alias", () => {
    expect(displaySeatName({ ...player, isUser: false, mode: "alias" })).toBe("Ace");
  });

  it("renders a shortened address when aliases are disabled", () => {
    expect(displaySeatName({ ...player, isUser: false, mode: "address" })).toBe("GABCDE...456789");
  });

  it("does not expose another player's identity in anonymous mode", () => {
    expect(displaySeatName({ ...player, isUser: false, mode: "anonymous" })).toBe("Player 2");
  });
});
