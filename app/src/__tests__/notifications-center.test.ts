import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadNotifications,
  pushNotification,
  markAllRead,
  markRead,
  clearNotification,
  clearAll,
  unreadCount,
  groupByType,
  groupLabel,
  NOTIFICATION_GROUPS,
  fireBrowserNotificationIfHidden,
  type AppNotification,
} from "@/lib/notifications-center";

function setupStorage() {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => { store.set(key, value); },
    removeItem: (key: string) => { store.delete(key); },
    clear: () => store.clear(),
    length: 0,
    key: () => null,
  });
  return store;
}

describe("notifications store", () => {
  beforeEach(setupStorage);
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("pushes a notification with defaults", () => {
    const next = pushNotification({ type: "friend-request", title: "Hi", body: "b" });
    expect(next).toHaveLength(1);
    expect(next[0].read).toBe(false);
    expect(next[0].id).toBeTruthy();
    expect(next[0].createdAt).toBeTruthy();
  });

  it("returns empty for no notifications", () => {
    expect(loadNotifications()).toEqual([]);
  });

  it("marks all read", () => {
    pushNotification({ type: "table-invite", title: "t", body: "b" });
    pushNotification({ type: "achievement", title: "a", body: "b" });
    const next = markAllRead();
    expect(unreadCount(next)).toBe(0);
  });

  it("marks a single notification read by id", () => {
    const [n] = pushNotification({ type: "tournament-reminder", title: "t", body: "b" });
    const next = markRead(n.id);
    expect(next.find((x) => x.id === n.id)?.read).toBe(true);
  });

  it("clears a single notification", () => {
    const [n] = pushNotification({ type: "friend-request", title: "t", body: "b" });
    const next = clearNotification(n.id);
    expect(next).toHaveLength(0);
  });

  it("clears all", () => {
    pushNotification({ type: "table-invite", title: "t", body: "b" });
    expect(clearAll()).toEqual([]);
  });

  it("computes unread count", () => {
    pushNotification({ type: "table-invite", title: "t", body: "b" });
    pushNotification({ type: "friend-request", title: "t", body: "b" });
    const items = loadNotifications();
    expect(unreadCount(items)).toBe(2);
    expect(unreadCount(markAllRead())).toBe(0);
  });
});

describe("groupByType", () => {
  it("groups notifications by type", () => {
    const items: AppNotification[] = [
      { id: "1", type: "table-invite", title: "a", body: "b", createdAt: 1, read: false },
      { id: "2", type: "table-invite", title: "c", body: "d", createdAt: 2, read: false },
      { id: "3", type: "achievement", title: "e", body: "f", createdAt: 3, read: true },
    ];
    const grouped = groupByType(items);
    expect(grouped["table-invite"]).toHaveLength(2);
    expect(grouped.achievement).toHaveLength(1);
    expect(grouped["friend-request"]).toHaveLength(0);
    expect(grouped["tournament-reminder"]).toHaveLength(0);
  });

  it("has an entry per group", () => {
    expect(NOTIFICATION_GROUPS).toHaveLength(4);
  });
});

describe("groupLabel", () => {
  it("labels every type", () => {
    for (const t of NOTIFICATION_GROUPS) {
      expect(groupLabel(t).length).toBeGreaterThan(0);
    }
  });
});

describe("fireBrowserNotificationIfHidden", () => {
  it("does nothing when the tab is visible", () => {
    let fired = false;
    (globalThis as Record<string, unknown>).document = {
      hidden: false,
    } as Document;
    const mockCtor = vi.fn();
    (globalThis as Record<string, unknown>).Notification = mockCtor as unknown as typeof Notification;
    fireBrowserNotificationIfHidden({
      id: "1", type: "achievement", title: "t", body: "b", createdAt: 1, read: false,
    });
    expect(mockCtor).not.toHaveBeenCalled();
  });
});
