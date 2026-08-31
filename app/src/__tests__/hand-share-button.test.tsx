import { render, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { HandShareButton } from "../components/HandShareButton";
import type { HandShareEntry } from "../lib/hand-share";

const entry: HandShareEntry = {
  tableId: 3,
  handNumber: 12,
  finalPot: 5000,
  handRankName: "Flush",
  txHash: "deadbeef",
};

describe("HandShareButton (Issue #162)", () => {
  beforeEach(() => {
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
      share: undefined,
      canShare: undefined,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders a closed share button initially", () => {
    const { getByText, queryByText } = render(<HandShareButton entry={entry} />);
    expect(getByText("↗ SHARE")).toBeTruthy();
    expect(queryByText(/SHARE HAND #/)).toBeNull();
  });

  it("opens the share modal with the hand number on click", () => {
    const { getByText } = render(<HandShareButton entry={entry} />);
    fireEvent.click(getByText("↗ SHARE"));
    expect(getByText("SHARE HAND #12")).toBeTruthy();
    expect(getByText("SHARE TO X")).toBeTruthy();
    expect(getByText("COPY AS TEXT")).toBeTruthy();
    expect(getByText("DOWNLOAD IMAGE")).toBeTruthy();
  });

  it("copies the hand summary text to the clipboard and shows feedback", async () => {
    const { getByText } = render(<HandShareButton entry={entry} />);
    fireEvent.click(getByText("↗ SHARE"));
    fireEvent.click(getByText("COPY AS TEXT"));

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        expect.stringContaining("Flush")
      );
    });
    await waitFor(() => {
      expect(getByText("COPIED!")).toBeTruthy();
    });
  });

  it("closes the modal when the close button is clicked", () => {
    const { getByText, queryByText } = render(<HandShareButton entry={entry} />);
    fireEvent.click(getByText("↗ SHARE"));
    expect(getByText("SHARE HAND #12")).toBeTruthy();

    fireEvent.click(getByText("✕"));
    expect(queryByText("SHARE HAND #12")).toBeNull();
  });
});
