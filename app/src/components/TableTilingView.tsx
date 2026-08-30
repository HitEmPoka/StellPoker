"use client";

import { useState } from "react";
import type { OpenTable } from "@/lib/open-tables";

export type GridLayout = "auto" | "1x2" | "2x2" | "3x2";

interface TableTilingViewProps {
  tables: OpenTable[];
  activeTableId: number;
  onFocusTable: (tableId: number) => void;
  onCloseTable?: (tableId: number) => void;
  isOpen: boolean;
  onClose: () => void;
}

export function TableTilingView({
  tables,
  activeTableId,
  onFocusTable,
  onCloseTable,
  isOpen,
  onClose,
}: TableTilingViewProps) {
  const [layout, setLayout] = useState<GridLayout>("auto");

  if (!isOpen) return null;

  // Grid styling based on layout
  const getGridClass = () => {
    switch (layout) {
      case "1x2":
        return "grid grid-cols-1 md:grid-cols-2";
      case "2x2":
        return "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-2";
      case "3x2":
        return "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3";
      case "auto":
      default:
        if (tables.length <= 2) return "grid grid-cols-1 md:grid-cols-2";
        if (tables.length <= 4) return "grid grid-cols-1 md:grid-cols-2";
        return "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3";
    }
  };

  return (
    <div
      role="dialog"
      aria-label="Multi-Table Tiling Mode"
      className="fixed inset-0 z-50 flex flex-col bg-[#0c0a18]/95 p-4 backdrop-blur-md overflow-hidden"
    >
      {/* Header Toolbar */}
      <div className="flex justify-between items-center pb-3 border-b border-[#302b48] mb-4">
        <div className="flex items-center gap-3">
          <span className="text-[12px] font-bold text-[#f1c40f]" style={{ fontFamily: "'Press Start 2P', monospace" }}>
            MULTI-TABLE TILING MODE
          </span>
          <span className="text-[9px] text-[#95a5a6]">
            ({tables.length} {tables.length === 1 ? "table" : "tables"} active)
          </span>
        </div>

        {/* Layout Presets & Close */}
        <div className="flex items-center gap-2">
          <span className="text-[8px] text-[#95a5a6] mr-1">GRID:</span>
          {(["auto", "1x2", "2x2", "3x2"] as GridLayout[]).map((preset) => (
            <button
              key={preset}
              type="button"
              onClick={() => setLayout(preset)}
              className={`pixel-border-thin px-2 py-1 text-[8px] uppercase cursor-pointer ${
                layout === preset
                  ? "bg-[#27ae60] text-white border-[#2ecc71]"
                  : "bg-[#1e1b2e] text-[#c8e6ff] border-[#4a4568] hover:bg-[#2c2742]"
              }`}
            >
              {preset}
            </button>
          ))}
          <button
            type="button"
            onClick={onClose}
            aria-label="Close Tiling View"
            className="pixel-btn pixel-btn-red text-[8px] ml-4 px-2 py-1"
          >
            EXIT TILING
          </button>
        </div>
      </div>

      {/* Tiled Grid Container */}
      <div className={`flex-1 gap-4 overflow-y-auto ${getGridClass()}`} style={{ scrollbarWidth: "thin" }}>
        {tables.map((table) => {
          const isActive = table.tableId === activeTableId;

          return (
            <div
              key={table.tableId}
              data-testid={`tiled-table-${table.tableId}`}
              className={`relative flex flex-col rounded-lg p-3 pixel-border-thin transition-all ${
                isActive
                  ? "border-[#f1c40f] bg-[#1a1728] ring-2 ring-[#f1c40f]/40 shadow-xl"
                  : "border-[#3b3654] bg-[#12101e] hover:border-[#6c5ce7]"
              }`}
              style={{ minHeight: "220px" }}
            >
              {/* Tile Header */}
              <div className="flex justify-between items-center border-b border-[#2d2842] pb-2 mb-2">
                <div className="flex items-center gap-2">
                  <span className="text-[10px] font-bold text-[#f1c40f]">
                    TABLE #{table.tableId}
                  </span>
                  {table.mode && (
                    <span className="text-[7px] uppercase bg-[#2d2842] text-[#95a5a6] px-1.5 py-0.5 rounded">
                      {table.mode}
                    </span>
                  )}
                  {isActive && (
                    <span className="text-[7px] bg-[#27ae60] text-white px-1 py-0.5 rounded font-bold">
                      FOCUSED
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => {
                      onFocusTable(table.tableId);
                      onClose();
                    }}
                    className="pixel-btn pixel-btn-green text-[7px] px-2 py-0.5"
                    title="Focus and switch to this table"
                  >
                    FOCUS
                  </button>
                  {onCloseTable && (
                    <button
                      type="button"
                      onClick={() => onCloseTable(table.tableId)}
                      className="text-[#e74c3c] text-[8px] hover:text-white px-1.5 py-0.5"
                      title="Close tile"
                    >
                      ✕
                    </button>
                  )}
                </div>
              </div>

              {/* Miniaturized Table Canvas / Content */}
              <div
                className="flex-1 flex flex-col items-center justify-center rounded bg-[#091b10] border border-[#1b4329] p-3 relative cursor-pointer group"
                onClick={() => {
                  onFocusTable(table.tableId);
                  onClose();
                }}
              >
                {/* Felt Overlay details */}
                <div className="text-[9px] text-[#2ecc71] mb-1 font-bold">
                  POT: 0 XLM
                </div>
                <div className="flex gap-1 my-2">
                  <div className="w-6 h-8 bg-[#1e272e] rounded border border-gray-700 flex items-center justify-center text-[8px] text-gray-500">🂠</div>
                  <div className="w-6 h-8 bg-[#1e272e] rounded border border-gray-700 flex items-center justify-center text-[8px] text-gray-500">🂠</div>
                  <div className="w-6 h-8 bg-[#1e272e] rounded border border-gray-700 flex items-center justify-center text-[8px] text-gray-500">🂠</div>
                </div>
                <div className="text-[7px] text-[#95a5a6]">
                  Click to open full table view
                </div>
                <div className="absolute inset-0 bg-black/0 group-hover:bg-black/20 rounded transition-colors" />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
