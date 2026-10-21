/**
 * Offline guard for game actions (#632).
 *
 * The app shell works offline, but anything that changes table state needs
 * the coordinator and the chain. Such writes are refused up front with a clear
 * message, rather than failing with a generic network error after the player
 * has pressed the button.
 */

/** Thrown instead of sending a state-changing request while offline. */
export class OfflineError extends Error {
  constructor() {
    super("You're offline. This action was not sent. Reconnect and try again.");
    this.name = "OfflineError";
  }
}

/** True unless the browser reports that it is offline. */
export function isBrowserOnline(): boolean {
  return typeof navigator === "undefined" || navigator.onLine !== false;
}

/** Throws `OfflineError` before a write is attempted while offline. */
export function assertOnline(): void {
  if (!isBrowserOnline()) {
    throw new OfflineError();
  }
}
