import { describe, expect, it } from "vitest";
import { cardBack, cardLabel, decodeCard } from "@/lib/cards";

describe("decodeCard", () => {
  it("decodes the first card (2 of clubs)", () => {
    const card = decodeCard(0);
    expect(card).toEqual({
      value: 0,
      suit: "clubs",
      rank: "2",
      display: "2\u2663",
      suitSymbol: "\u2663",
      color: "black",
    });
  });

  it("decodes the last card (Ace of spades)", () => {
    const card = decodeCard(51);
    expect(card).toEqual({
      value: 51,
      suit: "spades",
      rank: "A",
      display: "A\u2660",
      suitSymbol: "\u2660",
      color: "black",
    });
  });

  it("decodes a red diamond", () => {
    // 13 = suit 1 (diamonds), rank 0 ("2")
    const card = decodeCard(13);
    expect(card.suit).toBe("diamonds");
    expect(card.rank).toBe("2");
    expect(card.color).toBe("red");
    expect(card.display).toBe("2\u2666");
  });

  it("decodes a red heart", () => {
    // 26 = suit 2 (hearts), rank 0 ("2")
    const card = decodeCard(26);
    expect(card.suit).toBe("hearts");
    expect(card.color).toBe("red");
  });

  it("decodes a black club", () => {
    const card = decodeCard(1); // 2 of clubs? no: 1 = suit 0, rank 1 = 3 of clubs
    expect(card.suit).toBe("clubs");
    expect(card.color).toBe("black");
  });

  it("round-trips through cardLabel for each suit", () => {
    expect(cardLabel(0)).toBe("Two of clubs");
    expect(cardLabel(13)).toBe("Two of diamonds");
    expect(cardLabel(26)).toBe("Two of hearts");
    expect(cardLabel(39)).toBe("Two of spades");
    expect(cardLabel(51)).toBe("Ace of spades");
  });
});

describe("cardBack", () => {
  it("returns the concealed card marker", () => {
    expect(cardBack()).toBe("?");
  });
});
