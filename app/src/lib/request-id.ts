/**
 * Client-generated correlation ids (#633).
 *
 * Every coordinator request carries an `X-Request-Id` header. When a call
 * fails, the id is appended to the error copy the player sees, so a support
 * report can be matched to the coordinator's logs for that exact call.
 */

/** Header name the coordinator reads the correlation id from. */
export const REQUEST_ID_HEADER = "X-Request-Id";

/**
 * Creates a random RFC 4122 v4 id. Uses `crypto.randomUUID` when available
 * and falls back to `getRandomValues` (or `Math.random` as a last resort) on
 * insecure origins where `randomUUID` is not exposed.
 */
export function createRequestId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") {
    return c.randomUUID();
  }

  const bytes = new Uint8Array(16);
  if (c && typeof c.getRandomValues === "function") {
    c.getRandomValues(bytes);
  } else {
    for (let i = 0; i < bytes.length; i++) {
      bytes[i] = Math.floor(Math.random() * 256);
    }
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join("-");
}

/** Appends the support reference to an error message, e.g. `"Deal failed (ref: …)"`. */
export function withSupportRef(message: string, requestId: string): string {
  return `${message} (ref: ${requestId})`;
}
