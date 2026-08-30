"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import {
  loadOpenTables,
  trackOpenTable,
  untrackOpenTable,
  tableHref,
  type OpenTable,
  type PlayMode,
} from "@/lib/open-tables";
import { TableTilingView } from "./TableTilingView";

interface TableTabsProps {
  /** Table currently on screen — highlighted, and not a link. */
  activeTableId: number;
  /** Mode the active table was opened in, remembered for the return trip. */
  activeMode?: PlayMode;
  /** Wallet the strip is scoped to. Nothing renders without one. */
  address: string | null;
}

/**
 * Strip of the player's other open tables, so someone playing several at once
 * can jump between them without going back to the lobby (#72).
 *
 * Includes multi-table tiling grid view modal (#178).
 */
export function TableTabs({ activeTableId, activeMode, address }: TableTabsProps) {
  const router = useRouter();
  const [tables, setTables] = useState<OpenTable[]>([]);
  const [tilingOpen, setTilingOpen] = useState(false);

  useEffect(() => {
    if (!address) {
      setTables([]);
      return;
    }
    setTables(trackOpenTable(address, activeTableId, activeMode));
  }, [address, activeTableId, activeMode]);

  // Another tab may have opened or closed a table; pick that up on focus.
  useEffect(() => {
    if (!address) return;
    const refresh = () => setTables(loadOpenTables(address));
    window.addEventListener("focus", refresh);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      window.removeEventListener("storage", refresh);
    };
  }, [address]);

  if (!address || tables.length < 2) return null;

  return (
    <>
      <nav
        aria-label="Open tables"
        className="w-full max-w-3xl flex items-center gap-2 flex-wrap"
      >
        <span className="text-[8px]" style={{ color: "#95a5a6" }}>
          TABLES
        </span>
        {tables.map((table) => {
          const isActive = table.tableId === activeTableId;
          const label = `#${table.tableId}`;

          if (isActive) {
            return (
              <span
                key={table.tableId}
                aria-current="page"
                className="pixel-border-thin px-2 py-1 text-[9px]"
                style={{
                  background: "rgba(20, 90, 50, 0.5)",
                  borderColor: "#27ae60",
                  color: "#eafaf1",
                }}
              >
                {label}
              </span>
            );
          }

          return (
            <Link
              key={table.tableId}
              href={tableHref(table)}
              className="pixel-border-thin px-2 py-1 text-[9px]"
              style={{
                background: "rgba(20, 20, 40, 0.5)",
                borderColor: "#4a6a8a",
                color: "#c8e6ff",
                textDecoration: "none",
              }}
              title={`Switch to table ${table.tableId}`}
            >
              {label}
            </Link>
          );
        })}

        <button
          type="button"
          onClick={() => setTilingOpen(true)}
          className="pixel-btn pixel-btn-blue text-[8px] px-2 py-1 ml-auto"
          title="Open multi-table grid view"
        >
          ⊞ TILING VIEW
        </button>
      </nav>

      {/* Multi-table tiling grid view */}
      <TableTilingView
        isOpen={tilingOpen}
        onClose={() => setTilingOpen(false)}
        tables={tables}
        activeTableId={activeTableId}
        onFocusTable={(tableId) => {
          const target = tables.find((t) => t.tableId === tableId);
          if (target) {
            router.push(tableHref(target));
          }
        }}
        onCloseTable={(tableId) => {
          if (address) {
            const next = untrackOpenTable(address, tableId);
            setTables(next);
          }
        }}
      />
    </>
  );
}
