"use client";

import { useEffect, useState } from "react";
import {
  getSfxMuted,
  getSfxVolume,
  setSfxMuted,
  setSfxVolume,
  playSound,
  getSoundGroupMuted,
  setSoundGroupMuted,
  type SoundGroup,
} from "@/lib/sound-engine";

/** Renders Audio Controls */
export function AudioControls() {
  const [muted, setMutedState] = useState<boolean>(false);
  const [volume, setVolumeState] = useState<number>(0.55);
  const [groupMuted, setGroupMuted] = useState<Record<SoundGroup, boolean>>({ cards: false, chips: false, alerts: false });

  useEffect(() => {
    setMutedState(getSfxMuted());
    setVolumeState(getSfxVolume());
    setGroupMuted({ cards: getSoundGroupMuted("cards"), chips: getSoundGroupMuted("chips"), alerts: getSoundGroupMuted("alerts") });
  }, []);

  const handleToggleMute = () => {
    const next = !muted;
    setMutedState(next);
    setSfxMuted(next);
    if (!next) {
      playSound("chip");
    }
  };

  const toggleGroup = (group: SoundGroup) => {
    const next = !groupMuted[group];
    setGroupMuted((current) => ({ ...current, [group]: next }));
    setSoundGroupMuted(group, next);
  };

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = parseFloat(e.target.value);
    setVolumeState(val);
    setSfxVolume(val);
    if (muted && val > 0) {
      setMutedState(false);
      setSfxMuted(false);
    }
  };

  return (
    <div
      className="pixel-panel flex items-center gap-3 p-2 bg-neutral-900 border border-neutral-700 rounded-md text-white text-xs"
      data-testid="audio-controls"
    >
      <button
        onClick={handleToggleMute}
        title={muted ? "Unmute Audio" : "Mute Audio"}
        className="pixel-btn px-2 py-1 bg-neutral-800 hover:bg-neutral-700 font-bold rounded"
        data-testid="audio-mute-toggle"
      >
        {muted ? "🔇 MUTED" : "🔊 SFX"}
      </button>

      <div className="flex items-center gap-1.5">
        <label htmlFor="volume-slider" className="sr-only">
          Audio Volume
        </label>
        <input
          id="volume-slider"
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={volume}
          onChange={handleVolumeChange}
          className="w-20 accent-emerald-500 cursor-pointer"
          data-testid="audio-volume-slider"
        />
        <span className="w-8 text-[10px] text-neutral-400 font-mono">
          {Math.round(volume * 100)}%
        </span>
      </div>
      <div className="flex gap-2 text-[9px]" aria-label="Sound groups">
        {(["cards", "chips", "alerts"] as const).map((group) => (
          <label key={group} className="flex items-center gap-1 capitalize">
            <input type="checkbox" checked={!groupMuted[group]} onChange={() => toggleGroup(group)} />
            {group}
          </label>
        ))}
      </div>
    </div>
  );
}
