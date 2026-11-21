import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ActionPanel } from "@/components/ActionPanel";

type Props = Parameters<typeof ActionPanel>[0];

function renderPanel(overrides: Partial<Props> = {}) {
  const props: Props = {
    phase: "preflop",
    isMyTurn: true,
    currentBet: 20,
    myBet: 10,
    myStack: 1000,
    pot: 30,
    onAction: vi.fn(),
    betAmount: 0,
    setBetAmount: vi.fn(),
    ...overrides,
  };
  const result = render(<ActionPanel {...props} />);
  return { props, ...result };
}

const button = (name: RegExp) => screen.getByRole("button", { name });

describe("ActionPanel", () => {
  it("shows DEAL CARDS while waiting and fires start", () => {
    const { props } = renderPanel({ phase: "waiting", statusHint: "Need 2 players" });
    fireEvent.click(button(/DEAL CARDS/));
    expect(props.onAction).toHaveBeenCalledWith("start");
    expect(screen.getByText("Need 2 players")).toBeTruthy();
  });

  it("disables DEAL CARDS when a hand cannot be started", () => {
    renderPanel({ phase: "waiting", canStartHand: false });
    expect((button(/DEAL CARDS/) as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows NEW HAND during settlement", () => {
    const { props } = renderPanel({ phase: "settlement" });
    fireEvent.click(button(/NEW HAND/));
    expect(props.onAction).toHaveBeenCalledWith("start");
  });

  it("renders nothing during showdown", () => {
    const { container } = renderPanel({ phase: "showdown" });
    expect(container.innerHTML).toBe("");
  });

  it("renders bet info and pot odds", () => {
    renderPanel();
    expect(screen.getByText(/TABLE BET: 20/)).toBeTruthy();
    expect(screen.getByText(/YOUR BET: 10/)).toBeTruthy();
    expect(screen.getByText(/STACK: 1,000/)).toBeTruthy();
    expect(screen.getByText(/POT ODDS: 10:30 \(25\.0%\)/)).toBeTruthy();
  });

  it("fires fold, call, raise and all-in actions", () => {
    const { props } = renderPanel();
    fireEvent.click(button(/FOLD/));
    fireEvent.click(button(/CALL 10/));
    fireEvent.click(button(/RAISE 40/));
    fireEvent.click(button(/ALL IN/));
    expect(props.onAction).toHaveBeenNthCalledWith(1, "fold");
    expect(props.onAction).toHaveBeenNthCalledWith(2, "call", 10);
    expect(props.onAction).toHaveBeenNthCalledWith(3, "raise", 40);
    expect(props.onAction).toHaveBeenNthCalledWith(4, "allin", 1000);
  });

  it("offers CHECK and BET when there is no bet to call", () => {
    const { props } = renderPanel({ currentBet: 0, myBet: 0, betAmount: 50 });
    fireEvent.click(button(/CHECK/));
    fireEvent.click(button(/BET 50/));
    expect(props.onAction).toHaveBeenNthCalledWith(1, "check", 0);
    expect(props.onAction).toHaveBeenNthCalledWith(2, "bet", 50);
  });

  it("updates the bet amount from presets and the slider", () => {
    const { props } = renderPanel();
    fireEvent.click(button(/^50%$/));
    expect(props.setBetAmount).toHaveBeenLastCalledWith(500);
    fireEvent.click(button(/^MAX$/));
    expect(props.setBetAmount).toHaveBeenLastCalledWith(1000);
    fireEvent.change(screen.getByRole("slider"), { target: { value: "300" } });
    expect(props.setBetAmount).toHaveBeenLastCalledWith(300);
  });

  it("sets pot sized bets from keyboard operable presets", () => {
    const { props } = renderPanel({ pot: 400 });
    fireEvent.click(button(/half the pot/));
    expect(props.setBetAmount).toHaveBeenLastCalledWith(200);
    fireEvent.click(button(/^Set bet to the pot$/));
    expect(props.setBetAmount).toHaveBeenLastCalledWith(400);
    expect(screen.getByRole("slider", { name: "Bet amount" })).toBeTruthy();
  });

  it("disables actions and shows waiting text when it is not my turn", () => {
    renderPanel({ isMyTurn: false });
    expect((button(/FOLD/) as HTMLButtonElement).disabled).toBe(true);
    expect((button(/ALL IN/) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/WAITING FOR OPPONENT/)).toBeTruthy();
    expect(screen.queryByRole("slider")).toBeNull();
  });

  it("shows the solo notice", () => {
    renderPanel({ isSolo: true });
    expect(screen.getByText(/SOLO VS AI/)).toBeTruthy();
  });
});
