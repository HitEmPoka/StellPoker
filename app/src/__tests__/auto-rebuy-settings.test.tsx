import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { AutoRebuySettings } from "../components/AutoRebuySettings";
import { getAutoRebuyPreference } from "../lib/auto-rebuy-store";

const ADDRESS = "GABC123";
const TABLE_ID = 5;

describe("AutoRebuySettings (Issue #164)", () => {
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
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders nothing when closed", () => {
    const { container } = render(
      <AutoRebuySettings open={false} onClose={() => {}} tableId={TABLE_ID} address={ADDRESS} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("defaults to 'Never auto-rebuy' selected", () => {
    render(<AutoRebuySettings open onClose={() => {}} tableId={TABLE_ID} address={ADDRESS} />);
    const neverRadio = screen.getByLabelText("Never auto-rebuy") as HTMLInputElement;
    expect(neverRadio.checked).toBe(true);
  });

  it("saves 'always_max' when selected and Save is clicked", () => {
    render(<AutoRebuySettings open onClose={() => {}} tableId={TABLE_ID} address={ADDRESS} />);
    fireEvent.click(screen.getByLabelText("Always rebuy to max"));
    fireEvent.click(screen.getByText("SAVE"));

    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({ mode: "always_max" });
  });

  it("shows the threshold input only for below_threshold, and saves the entered value", () => {
    render(<AutoRebuySettings open onClose={() => {}} tableId={TABLE_ID} address={ADDRESS} />);
    expect(screen.queryByLabelText("Threshold (big blinds):")).toBeNull();

    fireEvent.click(screen.getByLabelText("Rebuy when below N big blinds"));
    const input = screen.getByLabelText("Threshold (big blinds):") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "30" } });
    fireEvent.click(screen.getByText("SAVE"));

    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({
      mode: "below_threshold",
      thresholdBB: 30,
    });
  });

  it("calls onClose after saving", () => {
    let closed = false;
    render(
      <AutoRebuySettings open onClose={() => { closed = true; }} tableId={TABLE_ID} address={ADDRESS} />
    );
    fireEvent.click(screen.getByText("SAVE"));
    expect(closed).toBe(true);
  });
});
