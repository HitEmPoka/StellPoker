import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { SpectatorCount } from "@/components/SpectatorCount";

describe("SpectatorCount", () => {
  it("renders the live count", () => {
    render(<SpectatorCount count={4} />);
    expect(screen.getByTestId("spectator-count").textContent).toContain("4 WATCHING");
    expect(screen.getByLabelText("4 spectators watching")).toBeTruthy();
  });

  it("uses singular wording for one spectator", () => {
    render(<SpectatorCount count={1} />);
    expect(screen.getByLabelText("1 spectator watching")).toBeTruthy();
  });

  it("can hide itself when nobody is watching", () => {
    const { container } = render(<SpectatorCount count={0} hideWhenZero />);
    expect(container.innerHTML).toBe("");
  });
});
