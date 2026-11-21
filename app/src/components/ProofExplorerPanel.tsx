"use client";

import { useRef, useState } from "react";
import { ProofExplorer, type ProofExplorerData } from "./ProofExplorer";
import {
  PROOF_PANEL_MAX_WIDTH,
  PROOF_PANEL_MIN_WIDTH,
  clampPanelWidth,
  loadProofPanelPrefs,
  saveProofPanelPrefs,
  type ProofPanelPrefs,
} from "@/lib/proof-panel";

interface ProofExplorerPanelProps {
  open: boolean;
  onClose: () => void;
  /** Tx hashes, sizes and public inputs for the current hand (#625). */
  data?: ProofExplorerData;
}

/** Must match the mobile breakpoint in globals.css, where the panel is a bottom sheet. */
const BOTTOM_SHEET_QUERY = "(max-width: 640px)";
/** Width change per arrow-key press on the resize handle. */
const RESIZE_STEP = 16;
/** How much of the header must stay on screen while the panel is dragged. */
const HEADER_HEIGHT = 32;

function isBottomSheet(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.(BOTTOM_SHEET_QUERY).matches === true;
}

interface DragState {
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

interface ResizeState {
  startX: number;
  startWidth: number;
}

/**
 * The ZK proof explorer as a side panel beside the table (#160). On desktop it
 * is docked to the right edge, can be dragged by its header, resized from its
 * left edge, and collapsed to just its header; the width and collapsed state
 * are remembered in localStorage. Below the mobile breakpoint CSS turns it
 * into a bottom sheet instead (see `.proof-panel` in globals.css), so no
 * viewport-width state lives in React.
 */
export function ProofExplorerPanel({ open, onClose, data }: ProofExplorerPanelProps) {
  const [prefs, setPrefs] = useState<ProofPanelPrefs>(() => loadProofPanelPrefs());
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const panelRef = useRef<HTMLElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const resizeRef = useRef<ResizeState | null>(null);

  if (!open) return null;

  const updatePrefs = (next: ProofPanelPrefs) => {
    setPrefs(next);
    saveProofPanelPrefs(next);
  };

  // ── Drag by the header ──────────────────────────────────────────────────────

  const handleHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    // The bottom sheet stays pinned, and the header's buttons stay clickable.
    if (isBottomSheet() || (e.target as HTMLElement).closest("button")) return;
    const rect = panelRef.current?.getBoundingClientRect();
    if (!rect) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    // Keep the panel inside the viewport horizontally, and its header reachable.
    dragRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      originX: offset.x,
      originY: offset.y,
      minX: offset.x - rect.left,
      maxX: offset.x + (window.innerWidth - rect.right),
      minY: offset.y - rect.top,
      maxY: offset.y + (window.innerHeight - rect.top - HEADER_HEIGHT),
    };
  };

  const handleHeaderPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const x = drag.originX + e.clientX - drag.startX;
    const y = drag.originY + e.clientY - drag.startY;
    setOffset({
      x: Math.min(drag.maxX, Math.max(drag.minX, x)),
      y: Math.min(drag.maxY, Math.max(drag.minY, y)),
    });
  };

  const endDrag = () => {
    dragRef.current = null;
  };

  // ── Resize from the left edge ───────────────────────────────────────────────

  // The panel is anchored by its right edge, so pulling the left edge leftwards
  // widens it.
  const widthAt = (resize: ResizeState, clientX: number) =>
    clampPanelWidth(resize.startWidth + resize.startX - clientX);

  const handleResizePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (isBottomSheet()) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    resizeRef.current = { startX: e.clientX, startWidth: prefs.width };
  };

  const handleResizePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const resize = resizeRef.current;
    if (!resize) return;
    const width = widthAt(resize, e.clientX);
    setPrefs((current) => ({ ...current, width }));
  };

  const handleResizePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const resize = resizeRef.current;
    if (!resize) return;
    resizeRef.current = null;
    updatePrefs({ ...prefs, width: widthAt(resize, e.clientX) });
  };

  const handleResizePointerCancel = () => {
    if (!resizeRef.current) return;
    resizeRef.current = null;
    saveProofPanelPrefs(prefs);
  };

  const handleResizeKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const delta =
      e.key === "ArrowLeft" ? RESIZE_STEP : e.key === "ArrowRight" ? -RESIZE_STEP : 0;
    if (delta === 0) return;
    e.preventDefault();
    updatePrefs({ ...prefs, width: clampPanelWidth(prefs.width + delta) });
  };

  return (
    <aside
      ref={panelRef}
      className={`proof-panel pixel-border${prefs.collapsed ? " proof-panel-collapsed" : ""}`}
      aria-label="ZK proof explorer"
      style={{
        width: prefs.width,
        transform: `translate(${offset.x}px, ${offset.y}px)`,
      }}
    >
      {!prefs.collapsed && (
        <div
          className="proof-panel-resize"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize proof explorer"
          aria-valuenow={prefs.width}
          aria-valuemin={PROOF_PANEL_MIN_WIDTH}
          aria-valuemax={PROOF_PANEL_MAX_WIDTH}
          tabIndex={0}
          onPointerDown={handleResizePointerDown}
          onPointerMove={handleResizePointerMove}
          onPointerUp={handleResizePointerUp}
          onPointerCancel={handleResizePointerCancel}
          onKeyDown={handleResizeKeyDown}
        />
      )}

      <div
        className="proof-panel-header"
        onPointerDown={handleHeaderPointerDown}
        onPointerMove={handleHeaderPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <span>ZK PROOFS</span>
        <div className="flex items-center gap-1">
          <button
            className="proof-panel-btn"
            onClick={() => updatePrefs({ ...prefs, collapsed: !prefs.collapsed })}
            aria-expanded={!prefs.collapsed}
            aria-controls="proof-panel-body"
            title={prefs.collapsed ? "Expand" : "Collapse"}
          >
            {prefs.collapsed ? "▶" : "▼"}
          </button>
          <button
            className="proof-panel-btn"
            onClick={onClose}
            aria-label="Close proof explorer"
            title="Close"
          >
            ✕
          </button>
        </div>
      </div>

      <div id="proof-panel-body" className="proof-panel-body" hidden={prefs.collapsed}>
        <ProofExplorer data={data} />
      </div>
    </aside>
  );
}
