"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import { isReducedDataMode } from "@/lib/reduced-data-mode";

/**
 * PixelWorld — immersive background with day/night toggle and ambient music.
 * Click the sun to transition to night (crescent moon, stars, dark sky).
 * Click the moon to return to day. Music crossfades with the visual transition.
 */
type SharedAudioState = {
  day: HTMLAudioElement;
  night: HTMLAudioElement;
  started: boolean;
  isNight: boolean;
  masterVolume: number; // 0-1, user-controlled via GameBoy settings
};

let sharedAudioState: SharedAudioState | null = null;

/** Expose audio elements so other components (e.g. GameBoyModal) can control volume. */
export function getSharedAudio(): { day: HTMLAudioElement; night: HTMLAudioElement } | null {
  if (!sharedAudioState) return null;
  return { day: sharedAudioState.day, night: sharedAudioState.night };
}

/** Get/set master volume (0-1). Used by GameBoyModal volume slider. */
export function getMasterVolume(): number {
  return sharedAudioState?.masterVolume ?? 1;
}

/** Persists Master Volume.
 * @param v - v.
 */
export function setMasterVolume(v: number): void {
  if (!sharedAudioState) return;
  sharedAudioState.masterVolume = Math.max(0, Math.min(1, v));
  // Apply immediately to whichever track is active
  const mv = sharedAudioState.masterVolume;
  if (!sharedAudioState.day.paused) sharedAudioState.day.volume = mv;
  if (!sharedAudioState.night.paused) sharedAudioState.night.volume = mv;
}

function ensureSharedAudioState(): SharedAudioState {
  if (sharedAudioState) {
    return sharedAudioState;
  }

  const day = new Audio("/music/day-music.mp3");
  day.loop = true;
  day.volume = 1;

  const night = new Audio("/music/night-music.mp3");
  night.loop = true;
  night.volume = 0;

  sharedAudioState = {
    day,
    night,
    started: false,
    isNight: false,
    masterVolume: 1,
  };
  return sharedAudioState;
}

/** Renders Pixel World.
 * @param props(object) - props(object).
 */
export function PixelWorld({ children }: { children: React.ReactNode }) {
  const [isNight, setIsNight] = useState(() => sharedAudioState?.isNight ?? false);
  const reducedData = isReducedDataMode();
  const dayAudioRef = useRef<HTMLAudioElement | null>(null);
  const nightAudioRef = useRef<HTMLAudioElement | null>(null);
  const fadeRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const musicStartedRef = useRef(false);

  const FADE_MS = 2000; // matches visual transition duration
  const FADE_STEP = 50; // ms per volume tick

  // Link this component instance to shared global audio so music persists
  // across route changes and does not restart.
  useEffect(() => {
    const shared = ensureSharedAudioState();
    dayAudioRef.current = shared.day;
    nightAudioRef.current = shared.night;
    musicStartedRef.current = shared.started;
    setIsNight(shared.isNight);

    return () => {
      if (fadeRef.current) {
        clearInterval(fadeRef.current);
        fadeRef.current = null;
      }
    };
  }, []);

  // Crossfade when isNight changes — respects master volume
  const crossfade = useCallback((toNight: boolean) => {
    if (reducedData) return;
    const fadeIn = toNight ? nightAudioRef.current : dayAudioRef.current;
    const fadeOut = toNight ? dayAudioRef.current : nightAudioRef.current;
    if (!fadeIn || !fadeOut) return;
    const mv = sharedAudioState?.masterVolume ?? 1;

    // Start the incoming track if paused
    fadeIn.play().catch(() => {});

    if (fadeRef.current) clearInterval(fadeRef.current);
    const steps = FADE_MS / FADE_STEP;
    let step = 0;

    fadeRef.current = setInterval(() => {
      step++;
      const progress = Math.min(step / steps, 1);
      fadeIn.volume = Math.min(progress * mv, mv);
      fadeOut.volume = Math.max((1 - progress) * mv, 0);

      if (step >= steps) {
        if (fadeRef.current) clearInterval(fadeRef.current);
        fadeRef.current = null;
        fadeOut.pause();
        fadeOut.currentTime = 0;
      }
    }, FADE_STEP);
  }, []);

  // Start music on first user click anywhere in the world
  const handleFirstInteraction = useCallback(() => {
    if (reducedData) return;
    const shared = ensureSharedAudioState();
    if (musicStartedRef.current || shared.started) return;
    musicStartedRef.current = true;
    shared.started = true;
    const mv = shared.masterVolume;
    const active = isNight ? nightAudioRef.current : dayAudioRef.current;
    const inactive = isNight ? dayAudioRef.current : nightAudioRef.current;
    if (active) {
      active.volume = mv;
      active.play().catch(() => {});
    }
    if (inactive) {
      inactive.volume = 0;
    }
  }, [isNight, reducedData]);

  const duration = '2s';

  return (
    <div className="relative min-h-screen overflow-hidden" onClick={handleFirstInteraction}>
      {/* Day sky */}
      <div className="absolute inset-0" style={{
        background: 'linear-gradient(180deg, #4a90d9 0%, #6bb3e0 40%, #87ceeb 70%, #a8dcf0 100%)',
        opacity: isNight ? 0 : 1,
        transition: `opacity ${duration} ease-in-out`,
      }} />
      {/* Night sky */}
      <div className="absolute inset-0" style={{
        background: 'linear-gradient(180deg, #070b1a 0%, #0f1530 30%, #1a1845 55%, #0d1225 100%)',
        opacity: isNight ? 1 : 0,
        transition: `opacity ${duration} ease-in-out`,
      }} />
      {/* Sun / Moon — click to toggle day/night */}
      <div
        className="absolute top-8 right-16 z-[15] cursor-pointer"
        onClick={(e) => {
          e.stopPropagation();
          handleFirstInteraction();
          const next = !isNight;
          setIsNight(next);
          ensureSharedAudioState().isNight = next;
          crossfade(next);
        }}
        title={isNight ? "Switch to day" : "Switch to night"}
        style={{
          width: '64px',
          height: '64px',
          borderRadius: '50%',
          overflow: 'hidden',
          transition: `transform ${duration} cubic-bezier(0.4, 0, 0.2, 1)`,
          transform: isNight ? 'scale(0.9) rotate(-15deg)' : 'scale(1) rotate(0deg)',
        }}
      >
        {/* Base circle (sun or moon glow) */}
        <div style={{
          width: '100%',
          height: '100%',
          borderRadius: '50%',
          background: isNight ? '#e8e8f0' : '#f1c40f',
          position: 'relative',
          overflow: 'hidden',
        }}>
          {/* Dark overlay circle that slides in to create crescent */}
          <div style={{
            position: 'absolute',
            top: '-6px',
            left: isNight ? '16px' : '70px',
            width: '64px',
            height: '76px',
            borderRadius: '50%',
            background: isNight ? '#0f1530' : '#0f1530',
            transition: `left ${duration} cubic-bezier(0.4, 0, 0.2, 1), opacity ${duration} ease-in-out`,
            opacity: isNight ? 1 : 0,
          }} />
        </div>
      </div>

      {/* Stars (fade in at night) */}
      <div className="absolute inset-0 z-[0]" style={{
        opacity: isNight ? 1 : 0,
        transition: `opacity ${duration} ease-in-out`,
        transitionDelay: isNight ? '0.8s' : '0s',
        pointerEvents: 'none',
      }}>
        {[
          { x: 8, y: 6, s: 2, d: 0 }, { x: 22, y: 12, s: 3, d: 0.3 },
          { x: 38, y: 4, s: 2, d: 0.7 }, { x: 52, y: 18, s: 2, d: 1.1 },
          { x: 65, y: 8, s: 3, d: 0.5 }, { x: 78, y: 22, s: 2, d: 1.4 },
          { x: 12, y: 28, s: 2, d: 0.9 }, { x: 32, y: 10, s: 2, d: 1.7 },
          { x: 48, y: 26, s: 3, d: 0.2 }, { x: 88, y: 6, s: 2, d: 1.0 },
          { x: 3, y: 18, s: 2, d: 1.3 }, { x: 72, y: 3, s: 2, d: 0.6 },
          { x: 58, y: 14, s: 2, d: 1.8 }, { x: 42, y: 22, s: 3, d: 0.4 },
          { x: 18, y: 3, s: 2, d: 1.5 }, { x: 95, y: 15, s: 2, d: 0.8 },
          { x: 28, y: 20, s: 2, d: 1.2 }, { x: 82, y: 30, s: 3, d: 0.1 },
        ].map((star, i) => (
          <div key={i} className="absolute" style={{
            left: `${star.x}%`,
            top: `${star.y}%`,
            width: `${star.s}px`,
            height: `${star.s}px`,
            background: '#fff',
            animation: `twinkle ${2 + (i % 3)}s ease-in-out ${star.d}s infinite`,
          }} />
        ))}
      </div>

      {/* Clouds layer */}
      <div style={{
        opacity: isNight ? 0.1 : 0.95,
        filter: isNight ? 'brightness(0.5)' : 'none',
        transition: `opacity ${duration} ease-in-out, filter ${duration} ease-in-out`,
      }}>
        <PixelCloud top={60} delay={0} speed={45} size={1.2} />
        <PixelCloud top={30} delay={12} speed={55} size={0.9} />
        <PixelCloud top={100} delay={25} speed={38} size={1.0} />
        <PixelCloud top={140} delay={8} speed={50} size={0.7} />
        <PixelCloud top={80} delay={35} speed={60} size={1.1} />
      </div>

      {/* Far hills */}
      <div className="absolute bottom-0 left-0 right-0 z-[1]" style={{
        height: '30%',
        filter: isNight ? 'brightness(0.2) saturate(0.3)' : 'none',
        transition: `filter ${duration} ease-in-out`,
      }}>
        <svg viewBox="0 0 1200 200" preserveAspectRatio="none" className="w-full h-full" shapeRendering="crispEdges">
          <defs>
            <pattern id="farGrass" width="128" height="128" patternUnits="userSpaceOnUse">
              {grassTiles(['#5cb85c','#4cae4c','#68c468','#489848','#55b055','#6ed66e','#3d8b3d'], 8, 16, 3)}
            </pattern>
          </defs>
          <path d={pixelHillPath(1200, 200, 120, 40, 16, 3)} fill="url(#farGrass)" />
        </svg>
      </div>

      {/* Mid hills */}
      <div className="absolute bottom-0 left-0 right-0 z-[2]" style={{
        height: '22%',
        filter: isNight ? 'brightness(0.18) saturate(0.3)' : 'none',
        transition: `filter ${duration} ease-in-out`,
      }}>
        <svg viewBox="0 0 1200 160" preserveAspectRatio="none" className="w-full h-full" shapeRendering="crispEdges">
          <defs>
            <pattern id="midGrass" width="128" height="128" patternUnits="userSpaceOnUse">
              {grassTiles(['#4cae4c','#3d8b3d','#5cb85c','#2d6b2d','#45a845','#6ed66e','#358435','#8bc34a'], 8, 16, 7)}
            </pattern>
          </defs>
          <path d={pixelHillPath(1200, 160, 80, 10, 16, 7)} fill="url(#midGrass)" />
        </svg>
      </div>

      {/* Foreground grass */}
      <div className="absolute bottom-0 left-0 right-0 z-[3]" style={{
        height: '12%',
        filter: isNight ? 'brightness(0.18) saturate(0.4)' : 'none',
        transition: `filter ${duration} ease-in-out`,
      }}>
        <svg viewBox="0 0 1200 100" preserveAspectRatio="none" className="w-full h-full" shapeRendering="crispEdges">
          <defs>
            <pattern id="fgGrass" width="128" height="128" patternUnits="userSpaceOnUse">
              {grassTiles(['#3d8b3d','#2d6b2d','#4cae4c','#27ae60','#358535','#5cb85c','#1e7a2e','#45a845'], 8, 16, 11)}
            </pattern>
          </defs>
          <rect width="1200" height="100" fill="url(#fgGrass)" />
        </svg>
      </div>

      {/* Content layer */}
      <div className="relative z-[10]">
        {children}
      </div>
    </div>
  );
}

/* Generate a pixelated (staircase-stepped) hill silhouette path.
 * Uses sine waves for shape, then quantises y to step increments
 * so the top edge looks like chunky pixel art. */
function pixelHillPath(w: number, h: number, baseY: number, minY: number, step: number, seed: number): string {
  const cols = Math.ceil(w / step);
  const heights: number[] = [];
  for (let i = 0; i <= cols; i++) {
    const t = i / cols;
    // Layered sine waves for organic hills
    const raw = baseY
      + (minY - baseY) * (
        0.5 * Math.sin(t * Math.PI * 2 + seed)
        + 0.3 * Math.sin(t * Math.PI * 4 + seed * 2.7)
        + 0.2 * Math.sin(t * Math.PI * 6 + seed * 5.1)
      );
    // Quantise to step grid
    heights.push(Math.round(raw / step) * step);
  }

  // Build staircase path: horizontal segment then vertical step
  let d = `M0,${heights[0]}`;
  for (let i = 1; i <= cols; i++) {
    const x = Math.min(i * step, w);
    d += ` H${x}`;
    if (i <= cols && heights[i] !== undefined && heights[i] !== heights[i - 1]) {
      d += ` V${heights[i]}`;
    }
  }
  d += ` V${h} H0 Z`;
  return d;
}

/* Deterministic mosaic tile generator for grass/hills.
 * Uses larger blocks and clumps adjacent tiles to the same color
 * so the result looks organic rather than noisy. */
function grassTiles(colors: string[], blockSize: number, gridSize: number, seed: number) {
  // Pre-compute a color grid with large organic patches.
  // High neighbor-copy probability creates natural-looking clumps.
  const grid: number[][] = [];
  for (let y = 0; y < gridSize; y++) {
    grid[y] = [];
    for (let x = 0; x < gridSize; x++) {
      const hash = ((x * 11 + y * 17 + x * y * 5 + seed) * 31 + seed * 7) & 0xffff;
      const roll = hash % 100;
      // ~60% copy left, ~22% copy above, ~8% copy diagonal — only ~10% picks a new color
      if (x > 0 && roll < 60) {
        grid[y][x] = grid[y][x - 1];
      } else if (y > 0 && roll < 82) {
        grid[y][x] = grid[y - 1][x];
      } else if (x > 0 && y > 0 && roll < 90) {
        grid[y][x] = grid[y - 1][x - 1];
      } else {
        grid[y][x] = ((hash >> 3) % colors.length + colors.length) % colors.length;
      }
    }
  }

  const rects = [];
  for (let y = 0; y < gridSize; y++) {
    for (let x = 0; x < gridSize; x++) {
      rects.push(
        <rect key={`${x}-${y}`} x={x * blockSize} y={y * blockSize}
              width={blockSize + 0.5} height={blockSize + 0.5} fill={colors[grid[y][x]]} shapeRendering="crispEdges" />
      );
    }
  }
  return rects;
}

function PixelCloud({ top, delay, speed, size }: { top: number; delay: number; speed: number; size: number }) {
  const p = 8;
  const c: Record<string, string> = { w: '#fff', l: '#dde8f0', s: '#b8ccdc' };
  const shape = [
    '          ww                         ',
    '        wwwwww          ww           ',
    '       wwwwwwww       wwwwww         ',
    '      wwwwwwwwwww   wwwwwwwww        ',
    '     wwwwwwwwwwwww wwwwwwwwwww       ',
    '    wwwwwwwwwwwwwwwwwwwwwwwwwww      ',
    '   wwwwwwwwwwwwwwwwwwwwwwwwwwwww     ',
    '  wwwwwwwwwwwwwwwwwwwwwwwwwwwwwww    ',
    '  lwwwwwwwwwwwwwwwwwwwwwwwwwwwwwl    ',
    '  lllllwwwwwwwwwwwwwwwwwwwwwllll     ',
    '   ssllllllllllllllllllllllss        ',
    '     sssssssssssssssssssss           ',
  ];

  const shadows: string[] = [];
  shape.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) {
      const ch = row[x];
      if (c[ch]) shadows.push(`${x * p}px ${y * p}px 0 0.5px ${c[ch]}`);
    }
  });

  return (
    <div className="absolute z-[0]" style={{
      top: `${top}px`,
      left: '-300px',
      animation: `cloudFloat2 ${speed}s linear ${delay}s infinite`,
    }}>
      <div style={{ transform: `scale(${size})` }}>
        <div style={{
          width: `${p}px`,
          height: `${p}px`,
          background: 'transparent',
          boxShadow: shadows.join(', '),
        }} />
      </div>
    </div>
  );
}

