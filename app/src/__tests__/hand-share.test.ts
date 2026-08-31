import { describe, it, expect } from "vitest";
import {
  buildHandShareText,
  twitterIntentUrl,
  renderHandSummaryCanvas,
  canvasToBlob,
  type HandShareEntry,
} from "../lib/hand-share";

const baseEntry: HandShareEntry = {
  tableId: 42,
  handNumber: 7,
  finalPot: 15000,
  handRankName: "Full House",
  txHash: "abc123",
};

describe("buildHandShareText (Issue #162)", () => {
  it("includes the hand rank, pot size, table/hand numbers, and a tx explorer link", () => {
    const text = buildHandShareText(baseEntry);
    expect(text).toContain("Full House");
    expect(text).toContain("15,000");
    expect(text).toContain("Table #42");
    expect(text).toContain("Hand #7");
    expect(text).toContain("stellar.expert");
    expect(text).toContain("abc123");
  });

  it("falls back to a generic label when handRankName is missing", () => {
    const text = buildHandShareText({ ...baseEntry, handRankName: undefined });
    expect(text).toContain("a strong hand");
  });

  it("omits the explorer link when txHash is missing", () => {
    const text = buildHandShareText({ ...baseEntry, txHash: undefined });
    expect(text).not.toContain("stellar.expert");
  });
});

describe("twitterIntentUrl", () => {
  it("URL-encodes the share text into a twitter.com intent link", () => {
    const url = twitterIntentUrl("hello world & poker");
    expect(url).toBe(
      "https://twitter.com/intent/tweet?text=hello%20world%20%26%20poker"
    );
  });
});

describe("renderHandSummaryCanvas", () => {
  it("returns a canvas with the expected fixed dimensions", () => {
    const canvas = renderHandSummaryCanvas(baseEntry);
    expect(canvas).toBeInstanceOf(HTMLCanvasElement);
    expect(canvas.width).toBe(600);
    expect(canvas.height).toBe(315);
  });

  it("does not throw even when the environment has no 2D canvas context (e.g. jsdom)", () => {
    // jsdom does not implement CanvasRenderingContext2D by default, so
    // getContext("2d") returns null here — this must degrade gracefully,
    // not throw, matching real low-capability-browser behavior too.
    expect(() => renderHandSummaryCanvas(baseEntry)).not.toThrow();
  });
});

describe("canvasToBlob", () => {
  it("resolves null when canvas.toBlob is unavailable", async () => {
    const canvas = document.createElement("canvas");
    // jsdom implements `toBlob` on the HTMLCanvasElement prototype, but its
    // stub logs "Not implemented" and never invokes its callback — unlike a
    // real browser without canvas support (which simply wouldn't define the
    // method at all). `delete` doesn't remove an inherited prototype
    // method, so shadow it with an own `undefined` property instead, to
    // exercise the intended "method truly absent" path without hanging.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (canvas as any).toBlob = undefined;
    const blob = await canvasToBlob(canvas);
    expect(blob).toBeNull();
  });

  it("resolves the value produced by canvas.toBlob when available", async () => {
    const canvas = document.createElement("canvas");
    const fakeBlob = new Blob(["fake"], { type: "image/png" });
    (canvas as unknown as { toBlob: (cb: (b: Blob | null) => void) => void }).toBlob = (cb) => {
      cb(fakeBlob);
    };
    const blob = await canvasToBlob(canvas);
    expect(blob).toBe(fakeBlob);
  });
});
