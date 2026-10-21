"use client";

/**
 * PixelCat — renders cat sprite images from /cat_sprites/.
 * Available sprites: 17, 18, 19, 20, 21.
 * Sprite 18 is the user's cat (always shown with a golden glow).
 */

// Opponent sprites cycle through these
const OPPONENT_SPRITES = [17, 19, 20, 21];

/** Pick a non-18 sprite for an opponent seat index */
export function opponentSprite(seatIndex: number): number {
  return OPPONENT_SPRITES[seatIndex % OPPONENT_SPRITES.length];
}

interface PixelCatProps {
  /** Sprite number (17-21). Default 18. */
  sprite?: number;
  /** Width in pixels. Default 48. */
  size?: number;
  /** Idle bounce animation. Default true. */
  idle?: boolean;
  /** Mirror horizontally. */
  flipped?: boolean;
  /** Render the golden user glow. */
  isUser?: boolean;
}

/** Renders Pixel Cat.
 * @param props(object) - props(object).
 */
export function PixelCat({
  sprite = 18,
  size = 48,
  idle = true,
  flipped = false,
  isUser = false,
}: PixelCatProps) {
  return (
    <div
      style={{
        display: "inline-block",
        position: "relative",
        animation: idle ? "catIdle 2s ease-in-out infinite" : undefined,
        transform: flipped ? "scaleX(-1)" : undefined,
      }}
    >
      {/* User glow ring */}
      {isUser && (
        <div
          style={{
            position: "absolute",
            inset: `-${Math.round(size * 0.15)}px`,
            borderRadius: "50%",
            background:
              "radial-gradient(ellipse at center, rgba(241,196,15,0.35) 0%, rgba(241,196,15,0.12) 50%, transparent 70%)",
            animation: "userGlow 2s ease-in-out infinite",
            pointerEvents: "none",
            zIndex: 0,
          }}
        />
      )}
      <img
        src={`/cat_sprites/${sprite}.png`}
        alt={`Cat sprite ${sprite}`}
        width={size}
        height={size}
        style={{
          imageRendering: "pixelated",
          display: "block",
          position: "relative",
          zIndex: 1,
          filter: isUser
            ? "drop-shadow(0 0 6px rgba(241,196,15,0.7)) drop-shadow(0 0 12px rgba(241,196,15,0.35))"
            : undefined,
        }}
      />
    </div>
  );
}

/** Renders Pixel Heart.
 * @param props(object) - props(object).
 */
export function PixelHeart({ size = 4, beating = false }: { size?: number; beating?: boolean }) {
  const px = size;
  return (
    <div style={{
      display: 'inline-block',
      animation: beating ? 'heartBeat 1s ease-in-out infinite' : undefined,
    }}>
      <div style={{
        width: `${px}px`,
        height: `${px}px`,
        background: 'transparent',
        boxShadow: `
          ${1*px}px ${0}px 0 #e74c3c,
          ${2*px}px ${0}px 0 #e74c3c,
          ${4*px}px ${0}px 0 #e74c3c,
          ${5*px}px ${0}px 0 #e74c3c,
          ${0}px ${1*px}px 0 #e74c3c,
          ${1*px}px ${1*px}px 0 #ff6b6b,
          ${2*px}px ${1*px}px 0 #e74c3c,
          ${3*px}px ${1*px}px 0 #e74c3c,
          ${4*px}px ${1*px}px 0 #e74c3c,
          ${5*px}px ${1*px}px 0 #e74c3c,
          ${6*px}px ${1*px}px 0 #e74c3c,
          ${0}px ${2*px}px 0 #e74c3c,
          ${1*px}px ${2*px}px 0 #e74c3c,
          ${2*px}px ${2*px}px 0 #e74c3c,
          ${3*px}px ${2*px}px 0 #e74c3c,
          ${4*px}px ${2*px}px 0 #e74c3c,
          ${5*px}px ${2*px}px 0 #e74c3c,
          ${6*px}px ${2*px}px 0 #c0392b,
          ${1*px}px ${3*px}px 0 #e74c3c,
          ${2*px}px ${3*px}px 0 #e74c3c,
          ${3*px}px ${3*px}px 0 #e74c3c,
          ${4*px}px ${3*px}px 0 #e74c3c,
          ${5*px}px ${3*px}px 0 #c0392b,
          ${2*px}px ${4*px}px 0 #e74c3c,
          ${3*px}px ${4*px}px 0 #e74c3c,
          ${4*px}px ${4*px}px 0 #c0392b,
          ${3*px}px ${5*px}px 0 #c0392b
        `,
      }} />
    </div>
  );
}
