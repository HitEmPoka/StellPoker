import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { TurnCountdownBanner } from "@/components/TurnCountdownBanner";

function renderBanner(overrides: Partial<Parameters<typeof TurnCountdownBanner>[0]> = {}) {
  const onAutoFoldChange = vi.fn();
  const utils = render(
    <TurnCountdownBanner
      active={true}
      timeLeftSeconds={30}
      durationSeconds={30}
      autoFoldOn={true}
      onAutoFoldChange={onAutoFoldChange}
      {...overrides}
    />
  );
  return { ...utils, onAutoFoldChange };
}

describe("TurnCountdownBanner", () => {
  it("shows the banner and an auto-fold toggle while it is the viewer's turn", () => {
    renderBanner();
    expect(screen.getByTestId("turn-countdown-banner")).toBeTruthy();
    const toggle = screen.getByLabelText("Auto-fold when time runs out") as HTMLInputElement;
    expect(toggle.checked).toBe(true);
  });

  it("hides the banner when it is not the viewer's turn", () => {
    renderBanner({ active: false });
    expect(screen.queryByTestId("turn-countdown-banner")).toBeNull();
  });

  it("keeps the live region mounted so announcements are not missed", () => {
    renderBanner({ active: false });
    const live = screen.getByTestId("turn-countdown-live");
    expect(live.getAttribute("aria-live")).toBe("polite");
  });

  it("announces the turn start, then speaks at the 10 second threshold", () => {
    const { rerender, onAutoFoldChange } = renderBanner({ timeLeftSeconds: 30 });
    const live = () => screen.getByTestId("turn-countdown-live").textContent;
    expect(live()).toBe("It's your turn. 30 seconds to act.");

    rerender(
      <TurnCountdownBanner
        active={true}
        timeLeftSeconds={10}
        durationSeconds={30}
        autoFoldOn={true}
        onAutoFoldChange={onAutoFoldChange}
      />
    );
    expect(live()).toBe("10 seconds left to act.");
  });

  it("does not re-announce a threshold on every tick", () => {
    const { rerender, onAutoFoldChange } = renderBanner({ timeLeftSeconds: 10 });
    const live = () => screen.getByTestId("turn-countdown-live").textContent;
    const before = live();

    rerender(
      <TurnCountdownBanner
        active={true}
        timeLeftSeconds={9}
        durationSeconds={30}
        autoFoldOn={true}
        onAutoFoldChange={onAutoFoldChange}
      />
    );
    expect(live()).toBe(before);
  });

  it("reports auto-fold changes to the caller", () => {
    const { onAutoFoldChange } = renderBanner({ autoFoldOn: true });
    fireEvent.click(screen.getByLabelText("Auto-fold when time runs out"));
    expect(onAutoFoldChange).toHaveBeenCalledWith(false);
  });
});
