"use client";

import { useEffect, useRef, useCallback } from "react";

export type NotificationPrefs = {
  enabled: boolean;
  sound: boolean;
};

const PREFS_KEY = "stellpoker:notification_prefs";

export function loadNotificationPrefs(): NotificationPrefs {
  if (typeof window === "undefined") return { enabled: true, sound: true };
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) return JSON.parse(raw) as NotificationPrefs;
  } catch {
    // ignore
  }
  return { enabled: true, sound: true };
}

export function saveNotificationPrefs(prefs: NotificationPrefs): void {
  if (typeof window === "undefined") return;
  localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
}

/** Request browser notification permission if not already granted. */
export async function requestNotificationPermission(): Promise<boolean> {
  if (typeof window === "undefined" || !("Notification" in window)) return false;
  if (Notification.permission === "granted") return true;
  if (Notification.permission === "denied") return false;
  const result = await Notification.requestPermission();
  return result === "granted";
}

interface UseTurnNotificationOptions {
  isMyTurn: boolean;
  tableName: string;
  timeRemainingSecs?: number;
}

/**
 * Fires a browser notification and plays a subtle sound when it becomes
 * the local player's turn. Re-fires only when `isMyTurn` transitions to true.
 */
export function useTurnNotification({
  isMyTurn,
  tableName,
  timeRemainingSecs,
}: UseTurnNotificationOptions): void {
  const prevTurnRef = useRef(false);
  const audioRef = useRef<AudioContext | null>(null);

  const playTurnSound = useCallback(() => {
    const prefs = loadNotificationPrefs();
    if (!prefs.sound) return;
    try {
      if (!audioRef.current) {
        audioRef.current = new AudioContext();
      }
      const ctx = audioRef.current;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.type = "sine";
      osc.frequency.setValueAtTime(880, ctx.currentTime);
      osc.frequency.exponentialRampToValueAtTime(440, ctx.currentTime + 0.15);
      gain.gain.setValueAtTime(0.18, ctx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.35);
      osc.start(ctx.currentTime);
      osc.stop(ctx.currentTime + 0.35);
    } catch {
      // AudioContext not available
    }
  }, []);

  useEffect(() => {
    // Only fire on the rising edge (false → true).
    if (!isMyTurn || prevTurnRef.current === isMyTurn) {
      prevTurnRef.current = isMyTurn;
      return;
    }
    prevTurnRef.current = true;

    const prefs = loadNotificationPrefs();
    if (!prefs.enabled) return;

    const timeText =
      timeRemainingSecs !== undefined ? ` — ${timeRemainingSecs}s remaining` : "";
    const body = `It's your turn at ${tableName}${timeText}`;

    // Browser notification (only if permission already granted — we request on join).
    if (typeof window !== "undefined" && "Notification" in window && Notification.permission === "granted") {
      try {
        new Notification("StellPoker — Your Turn!", {
          body,
          icon: "/icon.svg",
          tag: "stellpoker-turn",
          renotify: true,
        } as NotificationOptions);
      } catch {
        // Notification API unavailable in this context
      }
    }

    playTurnSound();
  }, [isMyTurn, tableName, timeRemainingSecs, playTurnSound]);
}

/** Call once on table join to prompt for permission. */
export async function requestPermissionOnJoin(): Promise<void> {
  await requestNotificationPermission();
}
