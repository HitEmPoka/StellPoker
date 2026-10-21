import { rpc, xdr, scValToNative } from "@stellar/stellar-sdk";
import { getChainConfig } from "./api";

/**
 * Contract event subscription for the poker table.
 *
 * The Soroban contracts now emit an event for every meaningful state
 * transition (table created, player joined/left, hand started, deal committed,
 * player action, board revealed, hand settled, fold win, phase change). This
 * module consumes those events via the RPC `getEvents` endpoint so the frontend
 * can react to on-chain changes promptly instead of relying solely on a fixed
 * polling interval.
 *
 * RPC has no push/websocket channel, so we still issue periodic `getEvents`
 * requests, but each request only returns *new* events since the last cursor.
 * When a relevant event arrives we invoke `onEvent`, which the UI uses to
 * trigger an immediate state refresh — turning the 4s blind poll into an
 * event-driven refresh that fires as soon as something actually happens.
 */

export interface PokerTableEvent {
  /** Event topic name, e.g. "player_action", "hand_settled". */
  topic: string;
  /** Decoded data payload (best-effort native conversion). */
  data: unknown;
  /** Ledger the event was emitted in. */
  ledger: number;
}

/** Event Subscription shape. */
export interface EventSubscription {
  stop: () => void;
}

/**
 * Decode the first topic of an event to its symbol/string name. The RPC client
 * may hand topics back either as decoded `xdr.ScVal` objects (stellar-sdk v13)
 * or as base64 XDR strings, so handle both.
 */
function decodeTopic(topic: unknown): string | null {
  try {
    const val =
      typeof topic === "string"
        ? xdr.ScVal.fromXDR(topic, "base64")
        : (topic as xdr.ScVal);
    if (val.switch() === xdr.ScValType.scvSymbol()) {
      return val.sym().toString();
    }
    const native = scValToNative(val);
    return typeof native === "string" ? native : null;
  } catch {
    return null;
  }
}

/**
 * Subscribe to events emitted by the poker table contract.
 *
 * @param onEvent  called for each new contract event (after the cursor)
 * @param pollMs   how often to ask the RPC for new events (default 2s)
 * @returns an object whose `stop()` ends the subscription
 */
export async function subscribePokerTableEvents(
  onEvent: (event: PokerTableEvent) => void,
  pollMs = 2000
): Promise<EventSubscription> {
  const cfg = await getChainConfig();
  const server = new rpc.Server(cfg.rpc_url, {
    allowHttp: cfg.rpc_url.startsWith("http://"),
  });

  let cancelled = false;
  let cursor: string | undefined;
  let startLedger: number | undefined;

  // Seed the cursor at the current ledger so we only see future events.
  try {
    const latest = await server.getLatestLedger();
    startLedger = latest.sequence;
  } catch {
    startLedger = undefined;
  }

  async function tick() {
    if (cancelled) return;
    try {
      const res = await server.getEvents({
        ...(cursor
          ? { cursor }
          : startLedger
            ? { startLedger }
            : {}),
        filters: [
          {
            type: "contract",
            contractIds: [cfg.poker_table_contract],
          },
        ],
      });

      for (const raw of res.events ?? []) {
        // The SDK's event type varies across versions; read fields defensively.
        const ev = raw as {
          topic?: unknown[];
          value?: unknown;
          ledger?: number | string;
        };
        const topics = ev.topic ?? [];
        const topic = topics.length > 0 ? decodeTopic(topics[0]) : null;
        let data: unknown = null;
        try {
          data = ev.value ? scValToNative(ev.value as xdr.ScVal) : null;
        } catch {
          data = null;
        }
        onEvent({
          topic: topic ?? "unknown",
          data,
          ledger: Number(ev.ledger ?? 0),
        });
      }

      // Advance the cursor so the next poll only returns newer events.
      if (res.cursor) {
        cursor = res.cursor;
        startLedger = undefined;
      }
    } catch {
      // Network hiccup — keep polling; the next tick retries from the cursor.
    } finally {
      if (!cancelled) {
        setTimeout(tick, pollMs);
      }
    }
  }

  void tick();

  return {
    stop: () => {
      cancelled = true;
    },
  };
}
