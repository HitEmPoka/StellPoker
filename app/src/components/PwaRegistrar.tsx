"use client";

import { useEffect } from "react";

/**
 * Registers the offline service worker (#632). Only in production builds, so
 * development never serves stale cached code.
 */
export function PwaRegistrar() {
  useEffect(() => {
    if (process.env.NODE_ENV !== "production") return;
    if (typeof navigator === "undefined" || !("serviceWorker" in navigator)) return;
    navigator.serviceWorker.register("/sw.js").catch(() => {
      // Registration is best-effort. The app works fine without it.
    });
  }, []);

  return null;
}
