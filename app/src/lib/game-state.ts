/** Game Phase shape. */
export type GamePhase =
  | "waiting"
  | "dealing"
  | "preflop"
  | "flop"
  | "turn"
  | "river"
  | "showdown"
  | "settlement"
  | "awaiting_run_it_twice"
  | "showdown_run1"
  | "showdown_run2"
  | "rit_settlement";

/** Player shape. */
export interface Player {
  address: string;
  seat: number;
  stack: number;
  betThisRound: number;
  folded: boolean;
  allIn: boolean;
  cards?: [number, number];
}

/** Rit State shape. */
export interface RitState {
  active: boolean;
  player1Seat: number;
  player2Seat: number;
  player1OptedIn: boolean;
  player2OptedIn: boolean;
  sharedBoardCount: number;
  currentRun: number;
  run1BoardIndices: number[];
  run2BoardIndices: number[];
  run1Winner: number;
  run2Winner: number;
}

/** A value per ZK proof phase, keyed the way the proof explorer names them. */
export type ProofPhaseRecord<T> = Partial<Record<"deal" | "flop" | "turn" | "river" | "showdown", T>>;

/** Game State shape. */
export interface GameState {
  tableId: number;
  phase: GamePhase;
  players: Player[];
  pot: number;
  boardCards: number[];
  boardCardsRun2?: number[];
  currentTurn: number;
  dealerSeat: number;
  handNumber: number;
  lastTxHash?: string;
  proofSize?: number;
  /** Tx hash per proof phase for the current hand, for the proof explorer (#625). */
  proofTxHashes?: ProofPhaseRecord<string>;
  /** Proof size in bytes per phase for the current hand (#625). */
  proofSizes?: ProofPhaseRecord<number>;
  /** Public inputs the current hand's deal proof was verified against (#625). */
  dealPublicInputs?: { deckRoot?: string; handCommitments?: string[] };
  onChainConfirmed: boolean;
  ritState?: RitState;
}

/** Creates Initial State.
 * @param tableId - table Id.
 * @returns GameState.
 */
export function createInitialState(tableId: number): GameState {
  return {
    tableId,
    phase: "waiting",
    players: [],
    pot: 0,
    boardCards: [],
    currentTurn: 0,
    dealerSeat: 0,
    handNumber: 0,
    onChainConfirmed: false,
  };
}
