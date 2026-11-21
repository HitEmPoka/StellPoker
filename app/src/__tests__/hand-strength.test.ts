import { describe, expect, it } from "vitest";
import {
  classifyHandStrength,
  getHandStrengthColor,
  getHandStrengthLabel,
} from "@/lib/hand-strength";

function card(suitIndex: number, rankIndex: number): number {
  return suitIndex * 13 + rankIndex;
}

// rankIndex: 0="2" ... 12="A"
const A = 12;
const K = 11;
const Q = 10;

describe("classifyHandStrength (pre-flop, empty board)", () => {
  it("pocket aces are strong", () => {
    expect(classifyHandStrength([card(0, A), card(1, A)], [])).toBe("strong");
  });

  it("pocket kings are strong", () => {
    expect(classifyHandStrength([card(0, K), card(2, K)], [])).toBe("strong");
  });

  it("a low pair is marginal", () => {
    expect(classifyHandStrength([card(0, 5), card(1, 5)], [])).toBe("marginal");
  });

  it("A-K suited is marginal", () => {
    expect(classifyHandStrength([card(0, A), card(0, K)], [])).toBe("marginal");
  });

  it("A-2 offsuit is weak", () => {
    expect(classifyHandStrength([card(0, A), card(1, 0)], [])).toBe("weak");
  });

  it("7-2 offsuit is weak", () => {
    expect(classifyHandStrength([card(0, 5), card(1, 0)], [])).toBe("weak");
  });
});

describe("classifyHandStrength (flop, 5 total cards)", () => {
  it("a hole card pairing the board is strong", () => {
    expect(classifyHandStrength([card(0, A)], [card(1, A), card(2, 5), card(3, 8)])).toBe(
      "strong",
    );
  });

  it("both hole cards above the board is marginal", () => {
    expect(classifyHandStrength([card(0, A), card(1, K)], [card(2, 5), card(3, 4), card(0, 3)])).toBe(
      "marginal",
    );
  });

  it("a weak unconnected hand is weak", () => {
    expect(classifyHandStrength([card(0, 2), card(1, 6)], [card(2, 10), card(3, 8), card(0, 4)])).toBe(
      "weak",
    );
  });
});

describe("getHandStrengthColor", () => {
  it("maps strong", () => {
    expect(getHandStrengthColor("strong")).toBe("#27ae60");
  });
  it("maps marginal", () => {
    expect(getHandStrengthColor("marginal")).toBe("#f39c12");
  });
  it("maps weak", () => {
    expect(getHandStrengthColor("weak")).toBe("#e74c3c");
  });
  it("maps drawing-dead", () => {
    expect(getHandStrengthColor("drawing-dead")).toBe("#7f8c8d");
  });
});

describe("getHandStrengthLabel", () => {
  it("maps strong", () => {
    expect(getHandStrengthLabel("strong")).toBe("STRONG");
  });
  it("maps marginal", () => {
    expect(getHandStrengthLabel("marginal")).toBe("MARGINAL");
  });
  it("maps weak", () => {
    expect(getHandStrengthLabel("weak")).toBe("WEAK");
  });
  it("maps drawing-dead", () => {
    expect(getHandStrengthLabel("drawing-dead")).toBe("DEAD");
  });
});
