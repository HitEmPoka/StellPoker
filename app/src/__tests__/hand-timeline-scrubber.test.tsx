import { render, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { HandTimeline } from "../components/HandTimeline";
import type { TimelineEvent } from "../lib/hand-timeline";

const T0 = 1_700_000_000_000;

const EVENTS: TimelineEvent[] = [
  { id: "1:deal:preflop", kind: "deal", label: "DEAL", timestamp: T0, street: "preflop", pot: 20, boardCards: [] },
  { id: "1:action:preflop:60", kind: "action", label: "+40", timestamp: T0 + 12_000, street: "preflop", pot: 60, boardCards: [], amount: 40 },
  { id: "1:street:flop", kind: "street", label: "FLOP", timestamp: T0 + 30_000, street: "flop", pot: 60, boardCards: [1, 2, 3] },
  { id: "1:street:turn", kind: "street", label: "TURN", timestamp: T0 + 61_000, street: "turn", pot: 140, boardCards: [1, 2, 3, 4] },
];

function renderTimeline(index = EVENTS.length - 1, isLive = index === EVENTS.length - 1) {
  const onSeek = vi.fn();
  const onReturnToLive = vi.fn();
  const utils = render(
    <HandTimeline
      events={EVENTS}
      index={index}
      onSeek={onSeek}
      isLive={isLive}
      onReturnToLive={onReturnToLive}
    />
  );
  return { ...utils, onSeek, onReturnToLive };
}

describe("HandTimeline (#176)", () => {
  it("renders nothing before the hand has produced a moment", () => {
    const { container } = render(
      <HandTimeline events={[]} index={0} onSeek={vi.fn()} isLive onReturnToLive={vi.fn()} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows one marker per moment of the hand", () => {
    const { getByTestId } = renderTimeline();
    const markers = getByTestId("hand-timeline").querySelectorAll(
      ".hand-timeline-marker"
    );
    expect(markers).toHaveLength(EVENTS.length);
    expect(markers[0].textContent).toContain("DEAL");
    expect(markers[2].textContent).toContain("FLOP");
  });

  it("stamps each marker with its elapsed time into the hand", () => {
    const { getByTestId } = renderTimeline();
    const markers = getByTestId("hand-timeline").querySelectorAll(
      ".hand-timeline-marker"
    );
    expect(markers[0].textContent).toContain("0:00");
    expect(markers[1].textContent).toContain("0:12");
    expect(markers[3].textContent).toContain("1:01");
  });

  it("exposes the strip as a slider carrying the current position", () => {
    const { getByRole } = renderTimeline(1, false);
    const slider = getByRole("slider");
    expect(slider.getAttribute("aria-valuemin")).toBe("0");
    expect(slider.getAttribute("aria-valuemax")).toBe("3");
    expect(slider.getAttribute("aria-valuenow")).toBe("1");
    expect(slider.getAttribute("aria-valuetext")).toContain("+40");
    expect(slider.getAttribute("aria-valuetext")).toContain("pot 60");
  });

  it("scrubs with the arrow keys", () => {
    const { getByRole, onSeek } = renderTimeline(2, false);
    const slider = getByRole("slider");

    fireEvent.keyDown(slider, { key: "ArrowLeft" });
    expect(onSeek).toHaveBeenLastCalledWith(1);

    fireEvent.keyDown(slider, { key: "ArrowRight" });
    expect(onSeek).toHaveBeenLastCalledWith(3);
  });

  it("jumps to the ends with Home and End", () => {
    const { getByRole, onSeek } = renderTimeline(2, false);
    const slider = getByRole("slider");

    fireEvent.keyDown(slider, { key: "Home" });
    expect(onSeek).toHaveBeenLastCalledWith(0);

    fireEvent.keyDown(slider, { key: "End" });
    expect(onSeek).toHaveBeenLastCalledWith(3);
  });

  it("clamps arrow keys at the ends rather than running off the timeline", () => {
    const first = renderTimeline(0, false);
    fireEvent.keyDown(first.getByRole("slider"), { key: "ArrowLeft" });
    expect(first.onSeek).toHaveBeenLastCalledWith(0);
    first.unmount();

    const last = renderTimeline(3, true);
    fireEvent.keyDown(last.getByRole("slider"), { key: "ArrowRight" });
    expect(last.onSeek).toHaveBeenLastCalledWith(3);
  });

  it("ignores keys that aren't scrub controls", () => {
    const { getByRole, onSeek } = renderTimeline(1, false);
    fireEvent.keyDown(getByRole("slider"), { key: "a" });
    expect(onSeek).not.toHaveBeenCalled();
  });

  it("seeks to a moment when its marker is clicked", () => {
    const { getByTestId, onSeek } = renderTimeline();
    const markers = getByTestId("hand-timeline").querySelectorAll(
      ".hand-timeline-marker"
    );
    fireEvent.click(markers[1]);
    expect(onSeek).toHaveBeenCalledWith(1);
  });

  it("says LIVE while pinned to the newest moment", () => {
    const { getByTestId } = renderTimeline();
    expect(getByTestId("hand-timeline").textContent).toContain("LIVE");
    expect(getByTestId("hand-timeline").textContent).not.toContain("REVIEWING");
  });

  it("says what is being reviewed once the player scrubs back", () => {
    const { getByTestId } = renderTimeline(2, false);
    const text = getByTestId("hand-timeline").textContent ?? "";
    expect(text).toContain("REVIEWING FLOP AT 0:30");
  });

  it("disables the LIVE control while already live and enables it otherwise", () => {
    const live = renderTimeline();
    expect(live.getByRole("button", { name: /LIVE/ })).toHaveProperty("disabled", true);
    live.unmount();

    const past = renderTimeline(0, false);
    const button = past.getByRole("button", { name: /LIVE/ });
    expect(button).toHaveProperty("disabled", false);
    fireEvent.click(button);
    expect(past.onReturnToLive).toHaveBeenCalled();
  });

  it("keeps the markers out of the tab order, since the strip is the control", () => {
    const { getByTestId } = renderTimeline();
    const markers = getByTestId("hand-timeline").querySelectorAll(
      ".hand-timeline-marker"
    );
    for (const marker of markers) {
      expect(marker.getAttribute("tabindex")).toBe("-1");
    }
    expect(getByTestId("hand-timeline").querySelector('[role="slider"]')
      ?.getAttribute("tabindex")).toBe("0");
  });
});
