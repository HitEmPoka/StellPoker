/**
 * Screen-reader announcements for the turn countdown (#630).
 *
 * The visible countdown ticks every second, which is far too chatty for a
 * live region. Instead the countdown is spoken only when it crosses a few
 * fixed thresholds, each at most once per turn.
 */

/** Seconds-left values the countdown is announced at. */
export const ANNOUNCE_THRESHOLDS = [10, 5, 0] as const;

/** Spoken when the viewer's turn starts. */
export function turnStartAnnouncement(durationSeconds: number): string {
  return `It's your turn. ${durationSeconds} seconds to act.`;
}

/** Spoken at a threshold. `autoFoldOn` decides what happens at zero. */
export function turnThresholdAnnouncement(threshold: number, autoFoldOn: boolean): string {
  if (threshold <= 0) {
    return autoFoldOn ? "Time is up. Folding automatically." : "Time is up.";
  }
  return `${threshold} seconds left to act.`;
}

/**
 * The most urgent threshold the countdown has reached that has not been
 * spoken yet this turn, or `null` if there is nothing new to say. When the
 * clock jumps past several thresholds at once only the lowest one is spoken.
 */
export function nextTurnThreshold(
  timeLeftSeconds: number,
  alreadySpoken: ReadonlySet<number>
): number | null {
  const pending = ANNOUNCE_THRESHOLDS.filter(
    (threshold) => timeLeftSeconds <= threshold && !alreadySpoken.has(threshold)
  );
  return pending.length > 0 ? Math.min(...pending) : null;
}
