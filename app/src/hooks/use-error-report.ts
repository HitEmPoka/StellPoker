"use client";

/** Shape of an error report emitted to the client error bus. */
export interface ErrorReport {
  message: string;
  stack?: string;
  url?: string;
  timestamp: string;
}

/**
 * Client-safe error report hook.
 *
 * Reports an error to the console and dispatches it on a custom window event
 * so the NotificationsCenter / error boundary UX can react without crashing
 * the server render. Never throws: if `window` is unavailable (SSR) it only
 * returns the report object.
 */
export function useErrorReport() {
  function reportError(error: unknown, context?: string): ErrorReport {
    const err = error instanceof Error ? error : new Error(String(error));
    const report: ErrorReport = {
      message: err.message,
      stack: err.stack,
      url: typeof window !== "undefined" ? window.location.href : undefined,
      timestamp: new Date().toISOString(),
      ...(context ? { message: `${context}: ${err.message}` } : {}),
    };

    if (typeof window !== "undefined") {
      try {
        console.error("[StellPoker error report]", report);
        window.dispatchEvent(new CustomEvent("stellpoker:error", { detail: report }));
      } catch {
        // Reporting must never throw, even if dispatching fails.
      }
    }

    return report;
  }

  return { reportError };
}
