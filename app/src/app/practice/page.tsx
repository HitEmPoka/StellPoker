import type { Metadata } from "next";
import { PracticeTable } from "@/components/PracticeTable";

export const metadata: Metadata = {
  title: "Practice — Poker on Stellar",
  description:
    "Play Texas Hold'em against heuristic bots in your browser. No wallet, no XLM, nothing on chain.",
};

/**
 * Practice mode (#174).
 *
 * Deliberately outside the `/table/[id]` route: there is no table, no
 * coordinator session, and no chain state behind this page — it is the local
 * engine and nothing else, so a visitor with no wallet can open it directly.
 */
export default function PracticePage() {
  return <PracticeTable />;
}
