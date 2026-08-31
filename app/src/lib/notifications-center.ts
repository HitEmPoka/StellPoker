/**
 * Off-chain notification center store (Issue #169).
 *
 * Notifications — table invites, friend requests, tournament reminders and
 * achievement unlocks — are queued in localStorage and surfaced through the
 * notification dropdown in the header. When the tab is backgrounded the app
 * raises a browser `Notification` (if permission is granted) so the player is
 * still alerted.
 *
 * This module keeps the pure grouping / persistence logic together so the
 * dropdown component and tests stay small and focused.
 */

export type NotificationType =
  | "table-invite"
  | "friend-request"
  | "tournament-reminder"
  | "achievement";

export interface AppNotification {
  id: string;
  type: NotificationType;
  title: string;
  body: string;
  createdAt: number;
  read: boolean;
  /** Optional related table id (for table invites / reminders). */
  tableId?: number;
  /** Optional related friend address. */
  friend?: string;
}

const STORAGE_KEY = "stellpoker:notifications";
const MAX_NOTIFICATIONS = 100;

export const NOTIFICATION_GROUPS: NotificationType[] = [
  "table-invite",
  "friend-request",
  "tournament-reminder",
  "achievement",
];

export function groupLabel(type: NotificationType): string {
  switch (type) {
    case "table-invite":
      return "TABLE INVITES";
    case "friend-request":
      return "FRIEND REQUESTS";
    case "tournament-reminder":
      return "TOURNAMENTS";
    case "achievement":
      return "ACHIEVEMENTS";
  }
}

export function loadNotifications(): AppNotification[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as AppNotification[]) : [];
  } catch {
    return [];
  }
}

function persist(items: AppNotification[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
  } catch {
    // Storage unavailable — notifications just won't persist.
  }
}

function uid(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export function pushNotification(
  n: Omit<AppNotification, "id" | "createdAt" | "read">
): AppNotification[] {
  const full: AppNotification = { ...n, id: uid(), createdAt: Date.now(), read: false };
  const next = [full, ...loadNotifications()].slice(0, MAX_NOTIFICATIONS);
  persist(next);
  return next;
}

export function markAllRead(): AppNotification[] {
  const next = loadNotifications().map((n) => ({ ...n, read: true }));
  persist(next);
  return next;
}

export function markRead(id: string): AppNotification[] {
  const next = loadNotifications().map((n) =>
    n.id === id ? { ...n, read: true } : n
  );
  persist(next);
  return next;
}

export function clearNotification(id: string): AppNotification[] {
  const next = loadNotifications().filter((n) => n.id !== id);
  persist(next);
  return next;
}

export function clearAll(): AppNotification[] {
  persist([]);
  return [];
}

export function unreadCount(items: AppNotification[]): number {
  return items.filter((n) => !n.read).length;
}

/** Group an (already newest-first) list of notifications by type. */
export function groupByType(
  items: AppNotification[]
): Record<NotificationType, AppNotification[]> {
  const grouped: Record<NotificationType, AppNotification[]> = {
    "table-invite": [],
    "friend-request": [],
    "tournament-reminder": [],
    achievement: [],
  };
  for (const n of items) {
    grouped[n.type].push(n);
  }
  return grouped;
}

/** Raise a browser notification *only* when the tab is backgrounded. */
export function fireBrowserNotificationIfHidden(n: AppNotification): void {
  if (typeof document === "undefined") return;
  // Only surface system notifications when the page is hidden/backgrounded.
  if (!document.hidden) return;
  if (typeof window === "undefined" || !("Notification" in window)) return;
  if (Notification.permission !== "granted") return;
  try {
    new Notification(`StellPoker — ${n.title}`, {
      body: n.body,
      icon: "/icon.svg",
      tag: `stellpoker-${n.type}`,
      renotify: true,
    } as NotificationOptions);
  } catch {
    // Notification API unavailable in this context.
  }
}
