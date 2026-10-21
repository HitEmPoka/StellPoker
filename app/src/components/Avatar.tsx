"use client";

import { useEffect, useMemo, useState } from "react";
import {
  getCachedAvatar,
  isAvatarFresh,
  refreshAvatar,
  type CachedAvatar,
} from "@/lib/avatar-store";
import { svgDataUrl } from "@/lib/svg-avatars";
import { Identicon } from "./Identicon";

interface AvatarProps {
  /** Stellar address of the player. Used as cache key + Identicon fallback seed. */
  address: string;
  /** Size in pixels. Width == height (always square). Default 48. */
  size?: number;
  /** Show the tiny Identicon badge in the corner (legacy behavior). Default true. */
  showIdenticonBadge?: boolean;
  /** Pixel-art pixelated rendering? Default true (matches the site aesthetic). */
  pixelated?: boolean;
  /** Optional click handler (for opening the avatar picker on own seat). */
  onClick?: () => void;
  /** Extra CSS class. */
  className?: string;
  /** Style overrides. */
  style?: React.CSSProperties;
}

/**
 * Unified Avatar component (Issue #153).
 *
 * Rendering strategy, in priority order:
 *   1. Cached on-chain SVG avatar (`AvatarKind::Svg`) — rendered as an
 *      inline `<img>` via data URL so the browser can cache it deeply.
 *   2. Cached NFT avatar (`AvatarKind::Nft`) — uses the resolved imageUrl
 *      if present; otherwise falls through to Identicon while the
 *      metadata lookup resolves in the background.
 *   3. Deterministic Identicon (fallback — what the app always did).
 *
 * A background refresh fires on mount if the cache is stale.
 */
export function Avatar({
  address,
  size = 48,
  showIdenticonBadge = true,
  pixelated = true,
  onClick,
  className,
  style,
}: AvatarProps) {
  const [avatar, setAvatar] = useState<CachedAvatar | null>(() =>
    address ? getCachedAvatar(address) : null
  );

  useEffect(() => {
    if (!address) return;
    // Fire a background refresh if we don't have it or if it's stale.
    if (!avatar || !isAvatarFresh(address)) {
      refreshAvatar(address).then((a) => {
        if (a) setAvatar(a);
      });
    }
  }, [address]);

  const content = useMemo(() => {
    if (avatar && avatar.kind === "svg") {
      const url = svgDataUrl(avatar.templateId, avatar.params, size * 2);
      return (
        <img
          src={url}
          alt=""
          width={size}
          height={size}
          draggable={false}
          style={{
            width: size,
            height: size,
            display: "block",
            imageRendering: pixelated ? "pixelated" : undefined,
          }}
        />
      );
    }
    if (avatar && avatar.kind === "nft" && avatar.imageUrl) {
      return (
        <img
          src={avatar.imageUrl}
          alt=""
          width={size}
          height={size}
          draggable={false}
          style={{
            width: size,
            height: size,
            display: "block",
            objectFit: "cover",
            borderRadius: 2,
            imageRendering: pixelated ? "pixelated" : undefined,
          }}
          onError={(e) => {
            (e.currentTarget as HTMLImageElement).style.display = "none";
          }}
        />
      );
    }
    // Fallback: full-size Identicon
    return (
      <div
        style={{
          width: size,
          height: size,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Identicon
          seed={address}
          size={7}
          cellSize={Math.max(2, Math.floor(size / 10))}
        />
      </div>
    );
  }, [avatar, address, size, pixelated]);

  const cellSize = Math.max(2, Math.floor(size / 18));

  return (
    <div
      className={className}
      onClick={onClick}
      style={{
        position: "relative",
        width: size,
        height: size,
        display: "inline-block",
        cursor: onClick ? "pointer" : undefined,
        ...style,
      }}
    >
      {content}
      {showIdenticonBadge && avatar && avatar.kind !== "none" && (
        <div
          style={{
            position: "absolute",
            bottom: -Math.round(cellSize * 0.4),
            right: -Math.round(cellSize * 0.4),
          }}
        >
          <Identicon seed={address} size={4} cellSize={cellSize} />
        </div>
      )}
    </div>
  );
}
