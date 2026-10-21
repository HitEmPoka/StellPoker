import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { HandHistoryPanel, HISTORY_PAGE_SIZE } from "@/components/HandHistoryPanel";
import type { HandHistoryEntry } from "@/lib/hand-history";

function makeEntries(count: number): HandHistoryEntry[] {
  return Array.from({ length: count }, (_, i) => ({
    tableId: 1,
    handNumber: i + 1,
    timestamp: 1_700_000_000_000 + i,
    streets: [],
    finalPot: 100,
    boardCards: [],
  }));
}

describe("HandHistoryPanel", () => {
  it("renders only the first page of a long session", () => {
    render(<HandHistoryPanel open onClose={vi.fn()} entries={makeEntries(120)} />);
    expect(screen.getAllByText(/^HAND #/)).toHaveLength(HISTORY_PAGE_SIZE);
    expect(screen.getByRole("button", { name: /SHOW 50 MORE \(70 LEFT\)/ })).toBeTruthy();
  });

  it("loads more pages on demand until every hand is shown", () => {
    render(<HandHistoryPanel open onClose={vi.fn()} entries={makeEntries(120)} />);
    fireEvent.click(screen.getByRole("button", { name: /SHOW 50 MORE/ }));
    fireEvent.click(screen.getByRole("button", { name: /SHOW 20 MORE/ }));
    expect(screen.getAllByText(/^HAND #/)).toHaveLength(120);
    expect(screen.queryByRole("button", { name: /MORE/ })).toBeNull();
  });

  it("labels the close button", () => {
    const onClose = vi.fn();
    render(<HandHistoryPanel open onClose={onClose} entries={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Close hand history" }));
    expect(onClose).toHaveBeenCalled();
  });
});
