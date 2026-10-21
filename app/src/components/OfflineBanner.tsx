"use client";

import { useOnlineStatus } from "@/lib/use-online-status";

/**
 * Tells the player that game actions are paused while the browser is offline
 * (#632). Shown only when offline. Announced politely so screen readers pick
 * it up without interrupting.
 */
export function OfflineBanner() {
  const online = useOnlineStatus();
  if (online) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="offline-banner"
      className="pixel-border-thin"
      style={{
        position: "fixed",
        left: 8,
        right: 8,
        bottom: 8,
        zIndex: 10000,
        padding: "8px 12px",
        background: "rgba(231, 76, 60, 0.95)",
        color: "#fff",
        fontSize: 9,
        textAlign: "center",
      }}
    >
      You&apos;re offline. Game actions are paused until you reconnect.
    </div>
  );
}
