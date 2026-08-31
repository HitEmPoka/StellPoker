import { render, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { MobileActionBar } from "../components/MobileActionBar";

function renderBar(overrides: Partial<Parameters<typeof MobileActionBar>[0]> = {}) {
  const onAction = vi.fn();
  const setBetAmount = vi.fn();
  const utils = render(
    <MobileActionBar
      visible
      isMyTurn
      currentBet={0}
      myBet={0}
      myStack={1000}
      pot={100}
      betAmount={0}
      onAction={onAction}
      setBetAmount={setBetAmount}
      {...overrides}
    />
  );
  return { ...utils, onAction, setBetAmount };
}

describe("MobileActionBar (#175)", () => {
  it("renders nothing outside a betting round", () => {
    const { container } = renderBar({ visible: false });
    expect(container.firstChild).toBeNull();
  });

  it("shows the four decisions a player actually makes", () => {
    const { getByTestId } = renderBar();
    const labels = [...getByTestId("mobile-action-bar").querySelectorAll(
      ".mobile-action-btn"
    )].map((b) => b.textContent);
    expect(labels).toHaveLength(4);
    expect(labels[0]).toContain("FOLD");
    expect(labels[1]).toContain("CHECK");
    expect(labels[3]).toContain("ALL IN");
  });

  it("offers CHECK when nothing is owed and CALL with the amount otherwise", () => {
    const checking = renderBar();
    expect(checking.getByRole("button", { name: "CHECK" })).toBeTruthy();
    checking.unmount();

    const calling = renderBar({ currentBet: 250, myBet: 50 });
    expect(calling.getByRole("button", { name: "CALL 200" })).toBeTruthy();
  });

  it("sends the action a tap represents", () => {
    const { getByRole, onAction } = renderBar({ currentBet: 40, myBet: 0 });

    fireEvent.click(getByRole("button", { name: "FOLD" }));
    expect(onAction).toHaveBeenCalledWith("fold");

    fireEvent.click(getByRole("button", { name: "CALL 40" }));
    expect(onAction).toHaveBeenCalledWith("call", 40);

    fireEvent.click(getByRole("button", { name: "ALL IN" }));
    expect(onAction).toHaveBeenCalledWith("allin", 1000);
  });

  it("disables every action when it is not the player's turn", () => {
    const { getByTestId } = renderBar({ isMyTurn: false });
    const buttons = getByTestId("mobile-action-bar").querySelectorAll(
      ".mobile-action-btn"
    );
    for (const button of buttons) {
      expect(button).toHaveProperty("disabled", true);
    }
  });

  it("explains why the bar is inert while waiting", () => {
    const { getByTestId } = renderBar({ isMyTurn: false });
    expect(getByTestId("mobile-action-bar").textContent).toContain("WAITING");
  });

  it("keeps raise behind an expandable sheet rather than in the bar", () => {
    const { getByRole, queryByRole } = renderBar();
    expect(queryByRole("group", { name: "Raise presets" })).toBeNull();

    const raise = getByRole("button", { name: /BET options/ });
    expect(raise.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(raise);
    expect(queryByRole("group", { name: "Raise presets" })).not.toBeNull();
    expect(getByRole("button", { name: /BET options/ }).getAttribute("aria-expanded")).toBe(
      "true"
    );
  });

  it("offers pot-relative raise presets in the sheet", () => {
    const { getByRole } = renderBar({ pot: 400, myStack: 5000 });
    fireEvent.click(getByRole("button", { name: /BET options/ }));

    const presets = getByRole("group", { name: "Raise presets" });
    const labels = [...presets.querySelectorAll("button")].map((b) => b.textContent ?? "");
    expect(labels.some((l) => l.startsWith("MIN"))).toBe(true);
    expect(labels.some((l) => l.startsWith("POT"))).toBe(true);
    expect(labels.some((l) => l.startsWith("MAX"))).toBe(true);
    // More sizings than fit a phone, which is why the row scrolls.
    expect(labels.length).toBeGreaterThanOrEqual(4);
  });

  it("collapses presets that clamp to the same amount on a short stack", () => {
    const { getByRole } = renderBar({ pot: 400, myStack: 30, currentBet: 0 });
    fireEvent.click(getByRole("button", { name: /BET options/ }));
    const presets = getByRole("group", { name: "Raise presets" });
    const values = [...presets.querySelectorAll("button")].map(
      (b) => b.querySelector(".mobile-raise-preset-value")?.textContent
    );
    expect(new Set(values).size).toBe(values.length);
  });

  it("picks up a preset as the raise target", () => {
    const { getByRole, setBetAmount } = renderBar({ pot: 400, myStack: 5000 });
    fireEvent.click(getByRole("button", { name: /BET options/ }));
    fireEvent.click(getByRole("button", { name: /^POT/ }));
    expect(setBetAmount).toHaveBeenCalledWith(400);
  });

  it("submits the chosen size and closes the sheet", () => {
    const { getByRole, queryByRole, onAction } = renderBar({
      pot: 400,
      myStack: 5000,
      currentBet: 100,
      betAmount: 350,
    });
    fireEvent.click(getByRole("button", { name: /RAISE options/ }));
    fireEvent.click(getByRole("button", { name: /CONFIRM RAISE/ }));

    expect(onAction).toHaveBeenCalledWith("raise", 350);
    expect(queryByRole("group", { name: "Raise presets" })).toBeNull();
  });

  it("never lets the sheet target more than the stack", () => {
    const { getByRole } = renderBar({ myStack: 80, myBet: 20, pot: 5000 });
    fireEvent.click(getByRole("button", { name: /BET options/ }));
    const slider = getByRole("slider", { name: "Raise amount" }) as HTMLInputElement;
    expect(Number(slider.max)).toBe(100);
  });

  it("closes the sheet on Escape", () => {
    const { getByRole, queryByRole } = renderBar();
    fireEvent.click(getByRole("button", { name: /BET options/ }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(queryByRole("group", { name: "Raise presets" })).toBeNull();
  });

  it("closes the sheet when the turn passes, so it can't show stale numbers", () => {
    const { getByRole, queryByRole, rerender } = renderBar();
    fireEvent.click(getByRole("button", { name: /BET options/ }));
    expect(queryByRole("group", { name: "Raise presets" })).not.toBeNull();

    rerender(
      <MobileActionBar
        visible
        isMyTurn={false}
        currentBet={0}
        myBet={0}
        myStack={1000}
        pot={100}
        betAmount={0}
        onAction={vi.fn()}
        setBetAmount={vi.fn()}
      />
    );
    expect(queryByRole("group", { name: "Raise presets" })).toBeNull();
  });

  it("cannot open the sheet when the stack is already committed to the call", () => {
    const { getByRole } = renderBar({ currentBet: 1000, myBet: 0, myStack: 1000 });
    expect(getByRole("button", { name: /RAISE options/ })).toHaveProperty(
      "disabled",
      true
    );
  });
});
