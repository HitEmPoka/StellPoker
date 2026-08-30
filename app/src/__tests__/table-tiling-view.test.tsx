import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TableTilingView } from "@/components/TableTilingView";
import type { OpenTable } from "@/lib/open-tables";

describe("TableTilingView", () => {
  const mockTables: OpenTable[] = [
    { tableId: 1, mode: "single", lastVisited: 100 },
    { tableId: 2, mode: "headsup", lastVisited: 200 },
    { tableId: 3, mode: "multi", lastVisited: 300 },
  ];

  it("does not render when isOpen is false", () => {
    render(
      <TableTilingView
        tables={mockTables}
        activeTableId={1}
        onFocusTable={vi.fn()}
        isOpen={false}
        onClose={vi.fn()}
      />
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("renders all open tables as tiles when open", () => {
    render(
      <TableTilingView
        tables={mockTables}
        activeTableId={1}
        onFocusTable={vi.fn()}
        isOpen={true}
        onClose={vi.fn()}
      />
    );
    expect(screen.getByRole("dialog")).toBeDefined();
    expect(screen.getByText("TABLE #1")).toBeDefined();
    expect(screen.getByText("TABLE #2")).toBeDefined();
    expect(screen.getByText("TABLE #3")).toBeDefined();
    expect(screen.getByText("FOCUSED")).toBeDefined();
  });

  it("calls onFocusTable and onClose when FOCUS button is clicked", () => {
    const handleFocus = vi.fn();
    const handleClose = vi.fn();

    render(
      <TableTilingView
        tables={mockTables}
        activeTableId={1}
        onFocusTable={handleFocus}
        isOpen={true}
        onClose={handleClose}
      />
    );

    const focusBtns = screen.getAllByText("FOCUS");
    fireEvent.click(focusBtns[1]); // Focus table 2

    expect(handleFocus).toHaveBeenCalledWith(2);
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it("changes grid preset layout when grid buttons are clicked", () => {
    render(
      <TableTilingView
        tables={mockTables}
        activeTableId={1}
        onFocusTable={vi.fn()}
        isOpen={true}
        onClose={vi.fn()}
      />
    );

    const btn2x2 = screen.getByText("2x2");
    fireEvent.click(btn2x2);
    expect(btn2x2.className).toContain("bg-[#27ae60]");
  });
});
