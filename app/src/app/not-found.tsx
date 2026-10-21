"use client";

import Link from "next/link";
import { PixelWorld } from "@/components/PixelWorld";

/** Default */
export default function NotFound() {
  return (
    <PixelWorld>
      <div className="min-h-screen flex items-center justify-center p-6">
        <div
          className="pixel-border p-5 flex flex-col items-center gap-3"
          style={{
            background: "rgba(12, 10, 24, 0.88)",
            borderColor: "#c47d2e",
            minWidth: "280px",
          }}
        >
          <div className="text-[11px]" style={{ color: "#ffc078" }}>
            TABLE NOT FOUND
          </div>
          <Link
            href="/"
            className="pixel-btn pixel-btn-blue text-[9px]"
            style={{ padding: "8px 14px", textDecoration: "none" }}
          >
            BACK TO LOBBY
          </Link>
        </div>
      </div>
    </PixelWorld>
  );
}
