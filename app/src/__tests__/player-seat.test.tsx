import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Player } from "@/lib/game-state";

vi.mock("@/lib/api", () => ({
  getPlayerHudStats: vi.fn(),
}));

import { PlayerSeat } from "@/components/PlayerSeat";
import { getPlayerHudStats } from "@/lib/api";

const ADDRESS = "GABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRSTUVW";

function makePlayer(overrides: Partial<Player> = {}): Player {
  return {
    address: ADDRESS,
    seat: 1,
    stack: 1500,
    betThisRound: 0,
    folded: false,
    allIn: false,
    ...overrides,
  };
}

describe("PlayerSeat", () => {
  beforeEach(() => {
    vi.mocked(getPlayerHudStats).mockReset();
    vi.mocked(getPlayerHudStats).mockResolvedValue({
      vpip: 25,
      pfr: 12.5,
      aggression_factor: 1.5,
      hands_played: 42,
    } as never);
  });

  it("shows a shortened address for opponents", () => {
    render(<PlayerSeat player={makePlayer()} isCurrentTurn={false} isDealer={false} isUser={false} showStatsTooltip={false} />);
    expect(screen.getByText(`${ADDRESS.slice(0, 4)}...${ADDRESS.slice(-4)}`)).toBeTruthy();
  });

  it("labels the user's own seat and alias", () => {
    const { rerender } = render(
      <PlayerSeat player={makePlayer()} isCurrentTurn={false} isDealer={false} isUser showStatsTooltip={false} />,
    );
    expect(screen.getByText("— YOU —")).toBeTruthy();
    rerender(
      <PlayerSeat player={makePlayer()} isCurrentTurn={false} isDealer={false} isUser alias="Ace" showStatsTooltip={false} />,
    );
    expect(screen.getByText("Ace (YOU)")).toBeTruthy();
  });

  it("renders turn, dealer, winner and status tags", () => {
    render(
      <PlayerSeat
        player={makePlayer({ allIn: true, betThisRound: 200 })}
        isCurrentTurn
        isDealer
        isUser={false}
        isWinner
        showStatsTooltip={false}
      />,
    );
    expect(screen.getByText("▼ THEIR TURN ▼")).toBeTruthy();
    expect(screen.getByText("[D]")).toBeTruthy();
    expect(screen.getByText("★ WINNER ★")).toBeTruthy();
    expect(screen.getByText("ALL IN!")).toBeTruthy();
    expect(screen.getByText(/BET:/)).toBeTruthy();
  });

  it("marks folded players and hides the turn indicator", () => {
    render(
      <PlayerSeat player={makePlayer({ folded: true })} isCurrentTurn isDealer={false} isUser={false} showStatsTooltip={false} />,
    );
    expect(screen.getByText("FOLDED")).toBeTruthy();
    expect(screen.queryByText("▼ THEIR TURN ▼")).toBeNull();
  });

  it("shows the user's hole cards face up and opponents' face down", () => {
    const cards: [number, number] = [51, 38]; // A♠, A♥
    const own = render(
      <PlayerSeat player={makePlayer({ cards })} isCurrentTurn={false} isDealer={false} isUser showStatsTooltip={false} />,
    );
    expect(own.container.textContent).toContain("♠");
    expect(own.container.textContent).toContain("♥");
    own.unmount();

    const opp = render(
      <PlayerSeat player={makePlayer({ cards })} isCurrentTurn={false} isDealer={false} isUser={false} showStatsTooltip={false} />,
    );
    expect(opp.container.textContent).not.toContain("♠");
  });

  it("invokes onEditAlias when the edit button is clicked", () => {
    const onEditAlias = vi.fn();
    render(
      <PlayerSeat player={makePlayer()} isCurrentTurn={false} isDealer={false} isUser onEditAlias={onEditAlias} showStatsTooltip={false} />,
    );
    fireEvent.click(screen.getByTitle("Change your alias"));
    expect(onEditAlias).toHaveBeenCalledTimes(1);
  });

  it("loads HUD stats into the tooltip", async () => {
    render(<PlayerSeat player={makePlayer()} isCurrentTurn={false} isDealer={false} isUser={false} />);
    expect(getPlayerHudStats).toHaveBeenCalledWith(ADDRESS);
    await waitFor(() => expect(screen.getByText("42")).toBeTruthy());
    expect(screen.getByText("25%")).toBeTruthy();
    expect(screen.getByText("1.50")).toBeTruthy();
  });

  it("does not fetch HUD stats for bots", () => {
    render(<PlayerSeat player={makePlayer()} isCurrentTurn={false} isDealer={false} isUser={false} isBot />);
    expect(getPlayerHudStats).not.toHaveBeenCalled();
    expect(screen.getByText("— AI BOT —")).toBeTruthy();
    expect(screen.getByAltText("AI Bot")).toBeTruthy();
  });
});
