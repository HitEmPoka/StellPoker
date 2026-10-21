"use client";

import { useEffect, useState } from "react";
import { isReducedDataMode, setReducedDataMode } from "@/lib/reduced-data-mode";

/** Toggle reduced-data rendering for low-bandwidth devices. */
export function ReducedDataToggle() {
  const [reduced, setReduced] = useState(false);

  useEffect(() => {
    setReduced(isReducedDataMode());
  }, []);

  const toggle = () => {
    const next = !reduced;
    setReducedDataMode(next);
    setReduced(next);
    window.location.reload();
  };

  return (
    <button
      onClick={toggle}
      title="Toggle reduced-data mode"
      aria-pressed={reduced}
      className="pixel-btn pixel-btn-dark text-[8px]"
      style={{ padding: "6px 10px" }}
    >
      {reduced ? "DATA LOW" : "DATA FULL"}
    </button>
  );
}