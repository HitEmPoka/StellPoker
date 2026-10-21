import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ProofExplorerPanel } from "../components/ProofExplorerPanel";
import {
  PROOF_PANEL_DEFAULT_WIDTH,
  PROOF_PANEL_MAX_WIDTH,
  PROOF_PANEL_MIN_WIDTH,
  clampPanelWidth,
  loadProofPanelPrefs,
  saveProofPanelPrefs,
} from "../lib/proof-panel";

const STORAGE_KEY = "stellpoker:proof-panel";

beforeEach(() => {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => { store.set(key, value); },
    removeItem: (key: string) => { store.delete(key); },
    clear: () => store.clear(),
    length: 0,
    key: () => null,
  });
  window.localStorage.removeItem(STORAGE_KEY);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("proof panel preferences (Issue #160)", () => {
  it("clamps the width to its bounds", () => {
    expect(clampPanelWidth(10)).toBe(PROOF_PANEL_MIN_WIDTH);
    expect(clampPanelWidth(5000)).toBe(PROOF_PANEL_MAX_WIDTH);
    expect(clampPanelWidth(300.4)).toBe(300);
    expect(clampPanelWidth(Number.NaN)).toBe(PROOF_PANEL_DEFAULT_WIDTH);
  });

  it("round-trips the width and collapsed state", () => {
    saveProofPanelPrefs({ width: 400, collapsed: true });
    expect(loadProofPanelPrefs()).toEqual({ width: 400, collapsed: true });
  });

  it("clamps a stored width that is out of range", () => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ width: 9999, collapsed: false }));
    expect(loadProofPanelPrefs()).toEqual({ width: PROOF_PANEL_MAX_WIDTH, collapsed: false });
  });

  it("falls back to the defaults for corrupted storage", () => {
    window.localStorage.setItem(STORAGE_KEY, "{not json");
    expect(loadProofPanelPrefs()).toEqual({
      width: PROOF_PANEL_DEFAULT_WIDTH,
      collapsed: false,
    });
  });
});

describe("ProofExplorerPanel (Issue #160)", () => {
  it("renders nothing when closed", () => {
    const { container } = render(<ProofExplorerPanel open={false} onClose={() => {}} />);
    expect(container.firstChild).toBeNull();
  });

  it("collapses to its header and remembers it", () => {
    render(<ProofExplorerPanel open onClose={() => {}} />);
    const toggle = screen.getByTitle("Collapse");
    fireEvent.click(toggle);

    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect((document.getElementById("proof-panel-body") as HTMLElement).hidden).toBe(true);
    expect(loadProofPanelPrefs().collapsed).toBe(true);
  });

  it("resizes from the keyboard and remembers the width", () => {
    render(<ProofExplorerPanel open onClose={() => {}} />);
    const handle = screen.getByRole("separator");
    // ArrowLeft pulls the left edge outwards by one step, widening the panel.
    fireEvent.keyDown(handle, { key: "ArrowLeft" });

    const widened = PROOF_PANEL_DEFAULT_WIDTH + 16;
    expect(handle.getAttribute("aria-valuenow")).toBe(String(widened));
    expect(loadProofPanelPrefs().width).toBe(widened);
  });

  it("calls onClose from its close button", () => {
    const onClose = vi.fn();
    render(<ProofExplorerPanel open onClose={onClose} />);
    fireEvent.click(screen.getByLabelText("Close proof explorer"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
