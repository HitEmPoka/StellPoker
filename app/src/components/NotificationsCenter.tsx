"use client";

/**
 * Notification dropdown for off-chain events (Issue #169): table invites,
 * friend requests, tournament reminders and achievement unlocks, grouped by
 * type. Raises a browser notification when the tab is backgrounded.
 */

import { useEffect, useRef, useState } from "react";
import {
  loadNotifications,
  pushNotification,
  markAllRead,
  markRead,
  clearAll,
  unreadCount,
  groupByType,
  groupLabel,
  fireBrowserNotificationIfHidden,
  NOTIFICATION_GROUPS,
  type AppNotification,
} from "@/lib/notifications-center";

export function NotificationsCenter() {
  const [open, setOpen] = useState(false);
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setNotifications(loadNotifications());
  }, []);

  // Close the dropdown when clicking outside.
  useEffect(() => {
    function handler(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  // Fire browser notifications for any new item while backgrounded.
  useEffect(() => {
    if (notifications.length === 0) return;
    const latest = notifications[0];
    fireBrowserNotificationIfHidden(latest);
  }, [notifications]);

  const count = unreadCount(notifications);
  const grouped = groupByType(notifications);

  return (
    <div ref={ref} className="relative" data-testid="notifications-center">
      <button
        onClick={() => setOpen((v) => !v)}
        aria-label={`Notifications${count ? `, ${count} unread` : ""}`}
        aria-expanded={open}
        className="relative pixel-btn text-[9px]"
        style={{ padding: "4px 10px", background: "#2c3e50", color: "white" }}
      >
        🔔
        {count > 0 && (
          <span
            className="absolute -top-1 -right-1 text-[7px] px-1 rounded-full"
            style={{ background: "#e74c3c", color: "white" }}
            data-testid="unread-badge"
          >
            {count}
          </span>
        )}
      </button>

      {open && (
        <div
          className="absolute right-0 top-full mt-2 w-80 pixel-border p-2 flex flex-col gap-2 z-50"
          style={{ background: "rgba(12,10,24,0.98)", borderColor: "#c47d2e" }}
          data-testid="notification-dropdown"
          role="region"
          aria-label="Notifications"
        >
          <div className="flex items-center justify-between">
            <span className="text-[8px]" style={{ color: "#95a5a6" }}>
              NOTIFICATIONS
            </span>
            <div className="flex gap-1">
              <button
                onClick={() => setNotifications(markAllRead())}
                className="pixel-btn text-[7px]"
                style={{ padding: "2px 6px", background: "#2c3e50", color: "#c8e6ff" }}
              >
                READ ALL
              </button>
              <button
                onClick={() => setNotifications(clearAll())}
                className="pixel-btn text-[7px]"
                style={{ padding: "2px 6px", background: "#2c3e50", color: "#e74c3c" }}
                aria-label="Clear all notifications"
              >
                CLEAR
              </button>
            </div>
          </div>

          {notifications.length === 0 ? (
            <div className="text-[8px] text-center py-4" style={{ color: "#7f8c8d" }}>
              NO NOTIFICATIONS
            </div>
          ) : (
            NOTIFICATION_GROUPS.map((type) => {
              const group = grouped[type];
              if (group.length === 0) return null;
              return (
                <div key={type} className="flex flex-col gap-1">
                  <div className="text-[7px] px-1" style={{ color: "#c47d2e" }}>
                    {groupLabel(type)} ({group.length})
                  </div>
                  <div className="flex flex-col gap-1">
                    {group.map((n) => (
                      <div
                        key={n.id}
                        onClick={() => setNotifications(markRead(n.id))}
                        className="pixel-border-thin px-2 py-1 cursor-pointer"
                        style={{
                          borderColor: n.read ? "#2a2a4a" : "#c47d2e",
                          background: n.read ? "rgba(0,0,0,0.2)" : "rgba(196,125,46,0.12)",
                        }}
                        data-testid="notification-item"
                        data-read={n.read}
                      >
                        <div className="text-[8px]" style={{ color: "#f5e6c8" }}>
                          {n.title}
                        </div>
                        <div className="text-[7px]" style={{ color: "#8a9ab0" }}>
                          {n.body}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}

/** Helper so a page can add an event and get the updated list back. */
export function addNotification(
  data: Omit<AppNotification, "id" | "createdAt" | "read">
): AppNotification[] {
  return pushNotification(data);
}
