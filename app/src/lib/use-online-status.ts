"use client";

import { useEffect, useState } from "react";
import { isBrowserOnline } from "./online";

/**
 * Tracks whether the browser is online (#632). Starts as `true` so the server
 * render and the first client render match, then syncs in an effect.
 */
export function useOnlineStatus(): boolean {
  const [online, setOnline] = useState(true);

  useEffect(() => {
    const sync = () => setOnline(isBrowserOnline());
    sync();
    window.addEventListener("online", sync);
    window.addEventListener("offline", sync);
    return () => {
      window.removeEventListener("online", sync);
      window.removeEventListener("offline", sync);
    };
  }, []);

  return online;
}
