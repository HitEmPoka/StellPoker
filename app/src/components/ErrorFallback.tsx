"use client";

import { useEffect } from "react";
import { useErrorReport } from "@/hooks/use-error-report";

interface ErrorFallbackProps {
  error: Error;
  reset: () => void;
}

/** Recoverable error UI with a retry button and a client-safe error report. */
export function ErrorFallback({ error, reset }: ErrorFallbackProps) {
  const { reportError } = useErrorReport();

  useEffect(() => {
    reportError(error);
  }, [error, reportError]);

  return (
    <div className="min-h-screen flex items-center justify-center p-6">
      <div
        className="pixel-border p-5 flex flex-col items-center gap-3"
        style={{
          background: "rgba(12, 10, 24, 0.88)",
          borderColor: "#e74c3c",
          minWidth: "280px",
        }}
      >
        <div className="text-[11px]" style={{ color: "#ffc078" }}>
          TABLE ERROR
        </div>
        <div className="text-[9px] text-center" style={{ color: "#f5e6c8" }}>
          {error.message}
        </div>
        <button
          onClick={() => {
            reportError(error);
            reset();
          }}
          className="pixel-btn pixel-btn-blue text-[9px]"
          style={{ padding: "8px 14px", textDecoration: "none" }}
        >
          RETRY
        </button>
      </div>
    </div>
  );
}
