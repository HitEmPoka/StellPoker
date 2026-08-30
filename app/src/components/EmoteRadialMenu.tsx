"use client";

import { useEffect, useRef } from "react";

export interface EmoteItem {
  id: string;
  icon: string;
  label: string;
  keyNumber: string;
}

export const RADIAL_EMOTES: EmoteItem[] = [
  { id: "nice-hand", icon: "👏", label: "Nice hand!", keyNumber: "1" },
  { id: "unlucky", icon: "💔", label: "Unlucky", keyNumber: "2" },
  { id: "wow", icon: "😲", label: "Wow", keyNumber: "3" },
  { id: "good-game", icon: "🤝", label: "Good game", keyNumber: "4" },
  { id: "on-fire", icon: "🔥", label: "On fire", keyNumber: "5" },
  { id: "thinking", icon: "🤔", label: "Thinking", keyNumber: "6" },
  { id: "ez", icon: "😎", label: "EZ", keyNumber: "7" },
  { id: "rip", icon: "💀", label: "RIP", keyNumber: "8" },
];

interface EmoteRadialMenuProps {
  isOpen: boolean;
  onClose: () => void;
  onSelectEmote: (emoteText: string) => void;
  /** Optional custom position style (e.g. fixed or absolute near seat/drawer) */
  anchorPosition?: { x: number; y: number };
}

export function EmoteRadialMenu({
  isOpen,
  onClose,
  onSelectEmote,
  anchorPosition,
}: EmoteRadialMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
        return;
      }
      const matched = RADIAL_EMOTES.find((item) => item.keyNumber === e.key);
      if (matched) {
        onSelectEmote(`${matched.icon} ${matched.label}`);
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose, onSelectEmote]);

  if (!isOpen) return null;

  const radius = 90; // Distance in px from center
  const total = RADIAL_EMOTES.length;

  return (
    <div
      role="dialog"
      aria-label="Emote Radial Menu"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-[2px] animate-fade-in"
      onClick={onClose}
    >
      <div
        ref={menuRef}
        className="relative w-64 h-64 flex items-center justify-center select-none"
        style={
          anchorPosition
            ? {
                position: "absolute",
                left: anchorPosition.x,
                top: anchorPosition.y,
                transform: "translate(-50%, -50%)",
              }
            : undefined
        }
        onClick={(e) => e.stopPropagation()}
      >
        {/* Center Hub Button */}
        <button
          type="button"
          onClick={onClose}
          aria-label="Close Emote Menu"
          className="z-20 w-14 h-14 rounded-full flex flex-col items-center justify-center pixel-border-thin cursor-pointer transition-transform hover:scale-105 active:scale-95 shadow-lg"
          style={{
            background: "rgba(30, 18, 12, 0.95)",
            borderColor: "#f1c40f",
            color: "#f1c40f",
            boxShadow: "0 0 12px rgba(241, 196, 15, 0.35)",
          }}
        >
          <span className="text-[14px]">💬</span>
          <span className="text-[7px] font-bold mt-[-2px]">EMOTE</span>
        </button>

        {/* Outer radial decorative circle */}
        <div
          className="absolute inset-2 rounded-full border border-dashed pointer-events-none"
          style={{ borderColor: "rgba(241, 196, 15, 0.25)" }}
        />

        {/* Radial Emote Buttons */}
        {RADIAL_EMOTES.map((item, index) => {
          // Angle starting from top (-PI/2) and distributing evenly
          const angle = (index / total) * 2 * Math.PI - Math.PI / 2;
          const x = Math.cos(angle) * radius;
          const y = Math.sin(angle) * radius;

          return (
            <button
              key={item.id}
              type="button"
              onClick={() => {
                onSelectEmote(`${item.icon} ${item.label}`);
                onClose();
              }}
              className="absolute z-30 group flex flex-col items-center justify-center p-2 rounded-lg pixel-border-thin cursor-pointer transition-all duration-150 hover:scale-115 hover:z-40"
              style={{
                transform: `translate(${x}px, ${y}px)`,
                background: "rgba(24, 14, 10, 0.95)",
                borderColor: "#8b6914",
                minWidth: "60px",
                boxShadow: "0 4px 6px rgba(0,0,0,0.5)",
              }}
              title={`[${item.keyNumber}] ${item.label}`}
            >
              <span className="text-[18px] transition-transform group-hover:scale-125">
                {item.icon}
              </span>
              <span
                className="text-[7px] text-[#f1c40f] whitespace-nowrap mt-0.5"
                style={{ fontFamily: "'Press Start 2P', monospace" }}
              >
                {item.label}
              </span>
              <span
                className="absolute -top-1.5 -right-1.5 bg-[#8b6914] text-[#1a120c] font-bold text-[6px] px-1 rounded-full border border-[#f1c40f]"
              >
                {item.keyNumber}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
