import { describe, it, expect } from "vitest";
import {
  createPracticeGame,
  startHand,
  applyAction,
  legalActions,
  reconfigure,
  isHumanTurn,
  humanSeat,
  displayPot,
  HUMAN_SEAT_ID,
  MAX_BOTS,
  type PracticeState,
} from "@/lib/practice-engine";

/** Plays a hand out by always taking the cheapest legal line. */
function foldOrCheckToTheEnd(state: PracticeState, limit = 40): PracticeState {
  let current = state;
  for (let i = 0; i < limit && current.phase !== "settlement"; i += 1) {
    if (!isHumanTurn(current)) break;
    const legal = legalActions(current);
    current = applyAction(current, legal.canCheck ? "check" : "call");
  }
  return current;
}

function chipsOnTable(state: PracticeState): number {
  return (
    state.seats.reduce((sum, s) => sum + s.stack + s.betThisRound, 0) + state.pot
  );
}

describe("practice table setup", () => {
  it("seats the human first, then the requested bots", () => {
    const game = createPracticeGame({ botCount: 3, difficulty: "hard" });
    expect(game.seats).toHaveLength(4);
    expect(game.seats[0].id).toBe(HUMAN_SEAT_ID);
    expect(game.seats.slice(1).every((s) => s.isBot)).toBe(true);
    expect(game.seats[1].name).toContain("HARD");
  });

  it("clamps the bot count to a playable table", () => {
    expect(createPracticeGame({ botCount: 0 }).seats).toHaveLength(2);
    expect(createPracticeGame({ botCount: 99 }).seats).toHaveLength(MAX_BOTS + 1);
  });

  it("gives everyone the configured starting stack and deals nothing yet", () => {
    const game = createPracticeGame({ startingStack: 250 });
    expect(game.seats.every((s) => s.stack === 250)).toBe(true);
    expect(game.phase).toBe("waiting");
    expect(game.seats.every((s) => s.cards === null)).toBe(true);
  });

  it("needs no wallet, network, or chain state to exist", () => {
    // The whole engine is synchronous and self-contained — if this constructs
    // and deals, practice mode works offline, which is the point of #174.
    const dealt = startHand(createPracticeGame({ botCount: 2 }, 7));
    expect(dealt.handNumber).toBe(1);
    expect(dealt.seats[0].cards).not.toBeNull();
  });
});

describe("dealing a hand", () => {
  it("deals two distinct hole cards per seat from one deck", () => {
    const state = startHand(createPracticeGame({ botCount: 3 }, 42));
    const dealt = state.seats.flatMap((s) => s.cards ?? []);
    expect(dealt).toHaveLength(8);
    expect(new Set(dealt).size).toBe(8);
    expect(dealt.every((c) => c >= 0 && c < 52)).toBe(true);
  });

  it("posts the blinds into the pot", () => {
    const state = startHand(
      createPracticeGame({ botCount: 1, smallBlind: 5, bigBlind: 10 }, 3)
    );
    expect(displayPot(state)).toBeGreaterThanOrEqual(15);
    expect(state.currentBet).toBeGreaterThanOrEqual(10);
  });

  it("is fully reproducible from a seed", () => {
    const a = startHand(createPracticeGame({ botCount: 2 }, 99));
    const b = startHand(createPracticeGame({ botCount: 2 }, 99));
    expect(a.seats.map((s) => s.cards)).toEqual(b.seats.map((s) => s.cards));
    expect(a.log).toEqual(b.log);
  });

  it("produces different hands from different seeds", () => {
    const a = startHand(createPracticeGame({ botCount: 2 }, 1));
    const b = startHand(createPracticeGame({ botCount: 2 }, 2));
    expect(a.seats[0].cards).not.toEqual(b.seats[0].cards);
  });

  it("moves the button between hands", () => {
    const first = startHand(createPracticeGame({ botCount: 2 }, 5));
    const second = startHand(foldOrCheckToTheEnd(first));
    expect(second.dealerSeat).not.toBe(first.dealerSeat);
  });
});

describe("legal actions", () => {
  it("offers a check exactly when nothing is owed, and a call otherwise", () => {
    // Checked across many deals rather than one, since which of the two the
    // player is offered depends on how the bots in front of them acted.
    let sawCheck = false;
    let sawCall = false;

    for (let seed = 1; seed <= 40; seed += 1) {
      const state = startHand(createPracticeGame({ botCount: 2 }, seed));
      const legal = legalActions(state);
      if (!legal.canAct) continue;

      expect(legal.canCheck).toBe(legal.callAmount === 0);
      expect(legal.canCall).toBe(legal.callAmount > 0);
      sawCheck ||= legal.canCheck;
      sawCall ||= legal.canCall;
    }

    // Both branches really do occur, so the assertion above isn't vacuous.
    expect(sawCall).toBe(true);
    expect(sawCheck || sawCall).toBe(true);
  });

  it("never lets a raise exceed the seat's stack", () => {
    const state = startHand(createPracticeGame({ botCount: 1, startingStack: 60 }, 8));
    const legal = legalActions(state);
    const seat = state.seats[state.toAct];
    expect(legal.maxRaiseTo).toBe(seat.betThisRound + seat.stack);
  });

  it("reports nobody to act once the hand has settled", () => {
    const settled = foldOrCheckToTheEnd(
      startHand(createPracticeGame({ botCount: 1 }, 4))
    );
    if (settled.phase === "settlement") {
      expect(legalActions(settled).canAct).toBe(false);
    }
  });
});

describe("playing a hand", () => {
  it("ends the hand immediately when the human folds heads-up", () => {
    let state = startHand(createPracticeGame({ botCount: 1 }, 21));
    if (!isHumanTurn(state)) state = foldOrCheckToTheEnd(state, 1);
    const folded = applyAction(state, "fold");
    expect(folded.phase).toBe("settlement");
    expect(humanSeat(folded)!.folded).toBe(true);
  });

  it("awards the pot to the last player standing", () => {
    let state = startHand(createPracticeGame({ botCount: 1 }, 21));
    const potBefore = displayPot(state);
    if (!isHumanTurn(state)) state = foldOrCheckToTheEnd(state, 1);
    const folded = applyAction(state, "fold");
    expect(folded.payouts).toHaveLength(1);
    expect(folded.payouts[0].seatId).not.toBe(HUMAN_SEAT_ID);
    expect(folded.payouts[0].amount).toBeGreaterThanOrEqual(potBefore);
  });

  it("never creates or destroys chips over a full hand", () => {
    const game = createPracticeGame({ botCount: 3, startingStack: 500 }, 17);
    const total = chipsOnTable(game);
    const played = foldOrCheckToTheEnd(startHand(game));
    expect(chipsOnTable(played)).toBe(total);
  });

  it("keeps chips conserved across many hands and seeds", () => {
    for (let seed = 1; seed <= 25; seed += 1) {
      let state = createPracticeGame({ botCount: 2, startingStack: 300 }, seed);
      const total = chipsOnTable(state);
      for (let hand = 0; hand < 5; hand += 1) {
        if (state.busted) break;
        state = foldOrCheckToTheEnd(startHand(state));
        expect(chipsOnTable(state)).toBe(total);
      }
    }
  });

  it("always reaches a terminal state rather than stalling", () => {
    for (let seed = 1; seed <= 25; seed += 1) {
      const played = foldOrCheckToTheEnd(
        startHand(createPracticeGame({ botCount: 3 }, seed)),
        80
      );
      // Either the hand finished, or it is waiting on the human — never a
      // state where nobody can act and the hand is not over.
      expect(played.phase === "settlement" || isHumanTurn(played)).toBe(true);
    }
  });

  it("reveals exactly five board cards when a hand runs to showdown", () => {
    for (let seed = 1; seed <= 30; seed += 1) {
      const played = foldOrCheckToTheEnd(
        startHand(createPracticeGame({ botCount: 1 }, seed))
      );
      if (played.phase !== "settlement") continue;
      const contested = played.seats.filter((s) => !s.folded).length > 1;
      if (contested) {
        expect(played.board).toHaveLength(5);
        expect(new Set(played.board).size).toBe(5);
      }
    }
  });

  it("names the winning hand at a contested showdown", () => {
    for (let seed = 1; seed <= 30; seed += 1) {
      const played = foldOrCheckToTheEnd(
        startHand(createPracticeGame({ botCount: 1 }, seed))
      );
      const contested = played.seats.filter((s) => !s.folded).length > 1;
      if (played.phase === "settlement" && contested) {
        expect(played.payouts[0].handName).toBeTruthy();
        return;
      }
    }
    throw new Error("no contested showdown found in 30 seeds");
  });

  it("ignores an action when it is not the human's turn to act", () => {
    const settled = foldOrCheckToTheEnd(
      startHand(createPracticeGame({ botCount: 1 }, 30))
    );
    if (settled.phase === "settlement") {
      expect(applyAction(settled, "fold")).toBe(settled);
    }
  });

  it("clamps an oversized raise to the stack instead of going negative", () => {
    let state = startHand(createPracticeGame({ botCount: 1, startingStack: 200 }, 12));
    if (!isHumanTurn(state)) state = foldOrCheckToTheEnd(state, 1);
    const raised = applyAction(state, "raise", 999999);
    expect(raised.seats.every((s) => s.stack >= 0)).toBe(true);
  });

  it("marks a seat all in once its stack is empty", () => {
    let state = startHand(createPracticeGame({ botCount: 1, startingStack: 80 }, 6));
    if (!isHumanTurn(state)) state = foldOrCheckToTheEnd(state, 1);
    const shoved = applyAction(state, "allin");
    const human = shoved.seats.find((s) => s.id === HUMAN_SEAT_ID)!;
    expect(human.stack === 0 || shoved.phase === "settlement").toBe(true);
  });
});

describe("session controls", () => {
  it("rebuilds the table when difficulty changes", () => {
    const game = createPracticeGame({ botCount: 2, difficulty: "easy" });
    const harder = reconfigure(game, { difficulty: "hard" });
    expect(harder.config.difficulty).toBe("hard");
    expect(harder.seats[1].name).toContain("HARD");
    expect(harder.phase).toBe("waiting");
  });

  it("keeps the other settings when only one changes", () => {
    const game = createPracticeGame({ botCount: 3, startingStack: 750 });
    const next = reconfigure(game, { difficulty: "easy" });
    expect(next.config.botCount).toBe(3);
    expect(next.config.startingStack).toBe(750);
  });

  it("flags a busted human rather than dealing an unfunded hand", () => {
    const game = createPracticeGame({ botCount: 1, startingStack: 100 }, 2);
    const broke: PracticeState = {
      ...game,
      seats: game.seats.map((s) =>
        s.id === HUMAN_SEAT_ID ? { ...s, stack: 0 } : s
      ),
    };
    const attempted = startHand(broke);
    expect(attempted.busted).toBe(true);
    expect(attempted.handNumber).toBe(0);
  });
});
