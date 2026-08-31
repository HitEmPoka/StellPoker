import { describe, it, expect, beforeEach, afterAll } from "vitest";
import {
  appendEvent,
  eventId,
  observeEvent,
  snapshotAt,
  formatElapsed,
  formatClock,
  loadTimeline,
  saveTimeline,
  type TimelineEvent,
  type TimelineObservation,
} from "@/lib/hand-timeline";

const T0 = 1_700_000_000_000;

function event(overrides: Partial<TimelineEvent> = {}): TimelineEvent {
  return {
    id: "1:street:flop",
    kind: "street",
    label: "FLOP",
    timestamp: T0,
    street: "flop",
    pot: 40,
    boardCards: [1, 2, 3],
    ...overrides,
  };
}

function observation(
  overrides: Partial<TimelineObservation> = {}
): TimelineObservation {
  return {
    handNumber: 1,
    phase: "preflop",
    pot: 20,
    boardCards: [],
    ...overrides,
  };
}

/** Storage that actually remembers, unlike the shared no-op stub. */
function inMemoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    key: (i: number) => [...data.keys()][i] ?? null,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, String(value)),
    removeItem: (key: string) => void data.delete(key),
    clear: () => data.clear(),
  } as Storage;
}

describe("appending events", () => {
  it("adds an event to an empty timeline", () => {
    expect(appendEvent([], event())).toHaveLength(1);
  });

  it("ignores a repeat of an event already recorded", () => {
    const first = appendEvent([], event());
    const again = appendEvent(first, event());
    // Same array back, so a React setter can bail out on identity.
    expect(again).toBe(first);
  });

  it("keeps distinct events in the order they happened", () => {
    let events = appendEvent([], event({ id: "a", label: "DEAL" }));
    events = appendEvent(events, event({ id: "b", label: "FLOP" }));
    expect(events.map((e) => e.label)).toEqual(["DEAL", "FLOP"]);
  });

  it("caps a runaway timeline rather than growing without bound", () => {
    let events: TimelineEvent[] = [];
    for (let i = 0; i < 200; i += 1) {
      events = appendEvent(events, event({ id: `e${i}` }));
    }
    expect(events.length).toBeLessThanOrEqual(120);
    // The most recent moments are the ones kept.
    expect(events[events.length - 1].id).toBe("e199");
  });

  it("builds ids from the moment, not the observation time", () => {
    expect(eventId(3, "action", "flop:120")).toBe("3:action:flop:120");
  });
});

describe("observing table state", () => {
  it("records nothing while the table is waiting for a deal", () => {
    expect(observeEvent(observation({ phase: "waiting" }), undefined, T0)).toBeNull();
  });

  it("opens the timeline with the deal", () => {
    const observed = observeEvent(observation(), undefined, T0);
    expect(observed).toMatchObject({ kind: "deal", label: "DEAL", street: "preflop" });
  });

  it("marks each new street", () => {
    const preflop = observeEvent(observation(), undefined, T0)!;
    const flop = observeEvent(
      observation({ phase: "flop", pot: 40, boardCards: [1, 2, 3] }),
      preflop,
      T0 + 5000
    );
    expect(flop).toMatchObject({ kind: "street", label: "FLOP" });
    expect(flop!.boardCards).toEqual([1, 2, 3]);
  });

  it("records a pot increase on the same street as an action", () => {
    const previous = event({ street: "flop", pot: 40 });
    const observed = observeEvent(
      observation({ phase: "flop", pot: 100, boardCards: [1, 2, 3] }),
      previous,
      T0 + 1000
    );
    expect(observed).toMatchObject({ kind: "action", label: "+60", amount: 60 });
  });

  it("attributes an action to whoever was on the clock", () => {
    const previous = event({ street: "flop", pot: 40 });
    const observed = observeEvent(
      observation({ phase: "flop", pot: 60, turnAddress: "GPLAYER" }),
      previous,
      T0
    );
    expect(observed!.actor).toBe("GPLAYER");
  });

  it("records nothing when neither street nor pot moved", () => {
    const previous = event({ street: "flop", pot: 40 });
    expect(
      observeEvent(observation({ phase: "flop", pot: 40 }), previous, T0)
    ).toBeNull();
  });

  it("gives the same moment the same id however often it is observed", () => {
    const previous = event({ street: "flop", pot: 40 });
    const seenOnce = observeEvent(observation({ phase: "flop", pot: 90 }), previous, T0)!;
    const seenAgain = observeEvent(
      observation({ phase: "flop", pot: 90 }),
      previous,
      T0 + 3000
    )!;
    expect(seenAgain.id).toBe(seenOnce.id);
    expect(appendEvent(appendEvent([], seenOnce), seenAgain)).toHaveLength(1);
  });

  it("labels settlement as the payout", () => {
    const previous = event({ street: "river", pot: 200 });
    const observed = observeEvent(
      observation({ phase: "settlement", pot: 200 }),
      previous,
      T0
    );
    expect(observed).toMatchObject({ kind: "settlement", label: "PAYOUT" });
  });
});

describe("reading a snapshot", () => {
  const events = [
    event({ id: "a", label: "DEAL", street: "preflop", pot: 20, boardCards: [], timestamp: T0 }),
    event({ id: "b", label: "FLOP", street: "flop", pot: 60, boardCards: [1, 2, 3], timestamp: T0 + 30_000 }),
    event({ id: "c", label: "TURN", street: "turn", pot: 120, boardCards: [1, 2, 3, 4], timestamp: T0 + 95_000 }),
  ];

  it("returns nothing for an empty timeline", () => {
    expect(snapshotAt([], 0)).toBeNull();
  });

  it("returns the table as it stood at that moment", () => {
    const snapshot = snapshotAt(events, 1)!;
    expect(snapshot.pot).toBe(60);
    expect(snapshot.boardCards).toEqual([1, 2, 3]);
    expect(snapshot.isLive).toBe(false);
  });

  it("flags the newest moment as live", () => {
    expect(snapshotAt(events, 2)!.isLive).toBe(true);
  });

  it("clamps an out-of-range index instead of returning nothing", () => {
    expect(snapshotAt(events, -5)!.index).toBe(0);
    expect(snapshotAt(events, 99)!.index).toBe(2);
  });

  it("reports elapsed time from the start of the hand", () => {
    expect(formatElapsed(events, 0)).toBe("0:00");
    expect(formatElapsed(events, 1)).toBe("0:30");
    expect(formatElapsed(events, 2)).toBe("1:35");
  });

  it("falls back to zero for an index that isn't there", () => {
    expect(formatElapsed([], 0)).toBe("0:00");
  });

  it("formats a wall clock for the marker tooltip", () => {
    expect(formatClock(T0)).toBe(new Date(T0).toLocaleTimeString());
  });
});

describe("surviving a reload mid-hand", () => {
  const original = Object.getOwnPropertyDescriptor(window, "localStorage");

  beforeEach(() => {
    Object.defineProperty(window, "localStorage", {
      value: inMemoryStorage(),
      writable: true,
      configurable: true,
    });
  });

  afterAll(() => {
    if (original) Object.defineProperty(window, "localStorage", original);
  });

  it("restores the timeline for the hand still being played", () => {
    const events = [event({ id: "a" }), event({ id: "b" })];
    saveTimeline(4, 7, events);
    expect(loadTimeline(4, 7).map((e) => e.id)).toEqual(["a", "b"]);
  });

  it("discards a timeline left over from a previous hand", () => {
    saveTimeline(4, 7, [event()]);
    expect(loadTimeline(4, 8)).toEqual([]);
  });

  it("scopes timelines per table", () => {
    saveTimeline(4, 1, [event({ id: "a" })]);
    saveTimeline(5, 1, [event({ id: "b" })]);
    expect(loadTimeline(4, 1).map((e) => e.id)).toEqual(["a"]);
    expect(loadTimeline(5, 1).map((e) => e.id)).toEqual(["b"]);
  });

  it("returns nothing for a table that has no stored timeline", () => {
    expect(loadTimeline(99, 1)).toEqual([]);
  });

  it("survives corrupt stored data", () => {
    window.localStorage.setItem("stellpoker:hand-timeline:4", "not json");
    expect(loadTimeline(4, 1)).toEqual([]);

    window.localStorage.setItem(
      "stellpoker:hand-timeline:4",
      JSON.stringify({ handNumber: 1, events: "nope" })
    );
    expect(loadTimeline(4, 1)).toEqual([]);
  });
});
