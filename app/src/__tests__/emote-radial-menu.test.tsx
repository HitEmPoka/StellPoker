import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { EmoteRadialMenu, RADIAL_EMOTES } from "@/components/EmoteRadialMenu";

describe("EmoteRadialMenu", () => {
  it("does not render when isOpen is false", () => {
    render(
      <EmoteRadialMenu
        isOpen={false}
        onClose={vi.fn()}
        onSelectEmote={vi.fn()}
      />
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("renders all radial emotes when isOpen is true", () => {
    render(
      <EmoteRadialMenu
        isOpen={true}
        onClose={vi.fn()}
        onSelectEmote={vi.fn()}
      />
    );
    expect(screen.getByRole("dialog")).toBeDefined();
    for (const emote of RADIAL_EMOTES) {
      expect(screen.getByText(emote.label)).toBeDefined();
    }
  });

  it("calls onSelectEmote and onClose when an emote button is clicked", () => {
    const handleSelect = vi.fn();
    const handleClose = vi.fn();

    render(
      <EmoteRadialMenu
        isOpen={true}
        onClose={handleClose}
        onSelectEmote={handleSelect}
      />
    );

    const niceHandBtn = screen.getByTitle("[1] Nice hand!");
    fireEvent.click(niceHandBtn);

    expect(handleSelect).toHaveBeenCalledWith("👏 Nice hand!");
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it("triggers emote selection via keyboard number shortcut", () => {
    const handleSelect = vi.fn();
    const handleClose = vi.fn();

    render(
      <EmoteRadialMenu
        isOpen={true}
        onClose={handleClose}
        onSelectEmote={handleSelect}
      />
    );

    fireEvent.keyDown(window, { key: "3" });
    expect(handleSelect).toHaveBeenCalledWith("😲 Wow");
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it("closes when Escape key is pressed", () => {
    const handleClose = vi.fn();

    render(
      <EmoteRadialMenu
        isOpen={true}
        onClose={handleClose}
        onSelectEmote={vi.fn()}
      />
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(handleClose).toHaveBeenCalledTimes(1);
  });
});
