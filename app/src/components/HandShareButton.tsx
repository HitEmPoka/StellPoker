"use client";

import { useEffect, useRef, useState } from "react";
import {
  buildHandShareText,
  canvasToBlob,
  renderHandSummaryCanvas,
  twitterIntentUrl,
  type HandShareEntry,
} from "@/lib/hand-share";

interface HandShareButtonProps {
  entry: HandShareEntry;
}

/** Whether the current browser can share files (canvas image) via the Web Share API. */
function canShareFiles(file: File): boolean {
  return (
    typeof navigator !== "undefined" &&
    typeof navigator.share === "function" &&
    typeof navigator.canShare === "function" &&
    navigator.canShare({ files: [file] })
  );
}

export function HandShareButton({ entry }: HandShareButtonProps) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [imageDataUrl, setImageDataUrl] = useState<string | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const canvas = renderHandSummaryCanvas(entry);
    canvasRef.current = canvas;
    try {
      setImageDataUrl(canvas.toDataURL("image/png"));
    } catch {
      setImageDataUrl(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const shareText = buildHandShareText(entry);

  const handleCopyText = async () => {
    try {
      await navigator.clipboard.writeText(shareText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API unavailable/denied — nothing more we can do silently.
    }
  };

  const handleDownloadImage = () => {
    if (!imageDataUrl) return;
    const link = document.createElement("a");
    link.href = imageDataUrl;
    link.download = `stellpoker-hand-${entry.tableId}-${entry.handNumber}.png`;
    link.click();
  };

  const handleShareToTwitter = async () => {
    const canvas = canvasRef.current;
    if (canvas) {
      const blob = await canvasToBlob(canvas);
      if (blob) {
        const file = new File([blob], `stellpoker-hand-${entry.handNumber}.png`, {
          type: "image/png",
        });
        if (canShareFiles(file)) {
          try {
            await navigator.share({ text: shareText, files: [file] });
            return;
          } catch {
            // User cancelled the share sheet, or it failed — fall through to
            // the web-intent fallback below rather than leaving them stuck.
          }
        }
      }
    }
    // Twitter/X's web intent URL cannot attach an image directly — open the
    // intent with the text pre-filled, and also trigger a download so the
    // user can manually attach the image in the compose window.
    window.open(twitterIntentUrl(shareText), "_blank", "noopener,noreferrer");
    handleDownloadImage();
  };

  if (!open) {
    return (
      <button
        onClick={() => setOpen(true)}
        title="Share this hand"
        style={{
          fontFamily: "'Press Start 2P', monospace",
          fontSize: "7px",
          background: "rgba(196,125,46,0.15)",
          border: "1px solid #c47d2e",
          color: "#ffc078",
          cursor: "pointer",
          padding: "2px 6px",
          lineHeight: 1.4,
        }}
      >
        ↗ SHARE
      </button>
    );
  }

  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.7)" }}
      onClick={(e) => {
        if (e.target === e.currentTarget) setOpen(false);
      }}
    >
      <div
        className="pixel-border"
        style={{
          background: "rgba(12, 10, 24, 0.98)",
          borderColor: "#c47d2e",
          width: "340px",
          padding: "16px",
        }}
      >
        <div className="flex items-center justify-between mb-3">
          <span className="text-[10px]" style={{ color: "#f5e6c8" }}>
            SHARE HAND #{entry.handNumber}
          </span>
          <button
            onClick={() => setOpen(false)}
            style={{ background: "none", border: "none", color: "#e74c3c", cursor: "pointer" }}
          >
            ✕
          </button>
        </div>

        {imageDataUrl && (
          <img
            src={imageDataUrl}
            alt={`Hand #${entry.handNumber} summary`}
            style={{ width: "100%", borderRadius: "4px", marginBottom: "12px" }}
          />
        )}

        <div className="flex flex-col gap-2">
          <button
            onClick={handleShareToTwitter}
            style={{
              fontFamily: "'Press Start 2P', monospace",
              fontSize: "8px",
              background: "#1d9bf0",
              border: "none",
              color: "#fff",
              cursor: "pointer",
              padding: "8px",
            }}
          >
            SHARE TO X
          </button>
          <button
            onClick={handleCopyText}
            style={{
              fontFamily: "'Press Start 2P', monospace",
              fontSize: "8px",
              background: "rgba(196,125,46,0.2)",
              border: "1px solid #c47d2e",
              color: "#ffc078",
              cursor: "pointer",
              padding: "8px",
            }}
          >
            {copied ? "COPIED!" : "COPY AS TEXT"}
          </button>
          <button
            onClick={handleDownloadImage}
            style={{
              fontFamily: "'Press Start 2P', monospace",
              fontSize: "8px",
              background: "none",
              border: "1px solid #7f8c8d",
              color: "#95a5a6",
              cursor: "pointer",
              padding: "8px",
            }}
          >
            DOWNLOAD IMAGE
          </button>
        </div>
      </div>
    </div>
  );
}
