"use client";

import { useEffect, useMemo, useState } from "react";
import {
  AVATAR_TEMPLATES,
  renderSvgAvatar,
  svgDataUrl,
  type AvatarTemplateInfo,
} from "@/lib/svg-avatars";
import { buildSvgParams, invalidateAvatar } from "@/lib/avatar-store";
import { Identicon } from "./Identicon";
import { trySilentReconnect, type WalletSession } from "@/lib/wallet";
import { useT } from "@/lib/i18n/context";
import { submitSvgAvatarTx, submitClearAvatarTx } from "@/lib/onchain";

type Tab = "svg" | "nft" | "none";

interface AvatarSelectorProps {
  /** Close selector callback. */
  onClose: () => void;
  /** Fired after a successful tx submit so the caller can refresh views. */
  onUpdated?: () => void;
}

/**
 * Modal for selecting / customizing a player avatar (Issue #153).
 *
 * Three tabs:
 *   - SVG:  browse template library, tweak colors/features, submit to chain.
 *   - NFT:  link an NFT from an approved contract (placeholder UI; admin
 *           populates the approved list on-chain).
 *   - None: clear custom avatar back to Identicon fallback.
 */
export function AvatarSelector({ onClose, onUpdated }: AvatarSelectorProps) {
  const t = useT();
  const [wallet, setWallet] = useState<WalletSession | null>(null);
  const [tab, setTab] = useState<Tab>("svg");
  const [tplId, setTplId] = useState<number>(0);
  const [params, setParams] = useState<Record<string, string>>(() => {
    const out: Record<string, string> = {};
    for (const p of AVATAR_TEMPLATES[0].params) out[p.key] = p.default;
    return out;
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nftContract, setNftContract] = useState("");
  const [nftTokenId, setNftTokenId] = useState("");

  useEffect(() => {
    trySilentReconnect().then((w) => setWallet(w));
  }, []);

  const template: AvatarTemplateInfo | undefined = AVATAR_TEMPLATES.find(
    (x) => x.id === tplId
  );

  useEffect(() => {
    if (!template) return;
    setParams((prev) => {
      const next: Record<string, string> = {};
      for (const p of template.params) {
        next[p.key] = prev[p.key] ?? p.default;
      }
      return next;
    });
  }, [tplId]);

  const previewSize = 128;
  const previewSvg = useMemo(() => {
    if (tab !== "svg" || !template) return null;
    const url = svgDataUrl(tplId, params, previewSize);
    return url;
  }, [tab, tplId, params, template]);

  const previewRaw = useMemo(() => {
    if (tab !== "svg" || !template) return null;
    return renderSvgAvatar(tplId, params, previewSize);
  }, [tab, tplId, params, template]);

  async function submitSvg() {
    if (!wallet || !template) return;
    setBusy(true);
    setError(null);
    try {
      await submitSvgAvatarTx(wallet, tplId, buildSvgParams(params));
      invalidateAvatar(wallet.address);
      onUpdated?.();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to submit");
    } finally {
      setBusy(false);
    }
  }

  async function submitNone() {
    if (!wallet) return;
    setBusy(true);
    setError(null);
    try {
      await submitClearAvatarTx(wallet);
      invalidateAvatar(wallet.address);
      onUpdated?.();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to submit");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.75)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
        padding: 16,
      }}
      onClick={onClose}
    >
      <div
        className="pixel-border p-4"
        style={{
          background: "#0e0c1a",
          borderColor: "#2a2a4a",
          width: "100%",
          maxWidth: 560,
          maxHeight: "90vh",
          overflowY: "auto",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex justify-between items-center mb-3">
          <div className="text-[11px]" style={{ color: "#f1c40f" }}>
            {t("avatar.title") ?? "CUSTOMIZE AVATAR"}
          </div>
          <button
            className="text-[9px]"
            style={{
              background: "none",
              border: "none",
              color: "#95a5a6",
              cursor: "pointer",
            }}
            onClick={onClose}
          >
            ✕
          </button>
        </div>

        {!wallet && (
          <div className="text-[9px] py-6 text-center" style={{ color: "#95a5a6" }}>
            {t("avatar.connectWallet") ?? "Connect a wallet first."}
          </div>
        )}

        {wallet && (
          <>
            <div className="flex gap-2 mb-3">
              {(
                [
                  ["svg", "SVG"],
                  ["nft", "NFT"],
                  ["none", "RESET"],
                ] as [Tab, string][]
              ).map(([key, label]) => (
                <button
                  key={key}
                  onClick={() => setTab(key)}
                  className="text-[9px] px-3 py-1"
                  style={{
                    background: tab === key ? "#2a2a4a" : "transparent",
                    color: tab === key ? "#f1c40f" : "#7f8c8d",
                    border: `1px solid ${tab === key ? "#f1c40f" : "#2a2a4a"}`,
                    cursor: "pointer",
                  }}
                >
                  {label}
                </button>
              ))}
            </div>

            {/* Preview */}
            <div className="flex gap-4 items-start mb-4">
              <div
                className="pixel-border-thin p-2"
                style={{ borderColor: "#2a2a4a", background: "#0a0914" }}
              >
                {tab === "svg" && previewSvg ? (
                  <img
                    src={previewSvg}
                    width={previewSize}
                    height={previewSize}
                    alt="preview"
                    style={{ imageRendering: "pixelated", display: "block" }}
                  />
                ) : tab === "nft" ? (
                  <div
                    style={{
                      width: previewSize,
                      height: previewSize,
                      background: "#1a1a2e",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      color: "#7f8c8d",
                      fontSize: 9,
                    }}
                  >
                    {nftContract && nftTokenId ? (
                      <span>NFT #{nftTokenId}</span>
                    ) : (
                      <span>NFT preview</span>
                    )}
                  </div>
                ) : (
                  <div style={{ padding: 8 }}>
                    <Identicon
                      seed={wallet.address}
                      size={7}
                      cellSize={8}
                    />
                  </div>
                )}
              </div>
              <div className="flex-1 text-[8px]" style={{ color: "#95a5a6", lineHeight: 1.6 }}>
                <div style={{ color: "#f5e6c8", fontSize: 9, marginBottom: 4 }}>
                  {template?.name ?? "Identicon"}
                </div>
                <div style={{ whiteSpace: "pre-wrap" }}>
                  {template?.description ??
                    (t("avatar.resetDesc") ??
                      "Clear custom avatar and fall back to the deterministic pixel Identicon.")}
                </div>
                {previewRaw && (
                  <details style={{ marginTop: 6 }}>
                    <summary style={{ cursor: "pointer", color: "#7f8c8d" }}>
                      {t("avatar.rawSvg") ?? "Raw SVG"}
                    </summary>
                    <pre
                      style={{
                        marginTop: 4,
                        padding: 6,
                        background: "#050410",
                        border: "1px solid #2a2a4a",
                        fontSize: 7,
                        maxHeight: 120,
                        overflow: "auto",
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-all",
                      }}
                    >
                      {previewRaw}
                    </pre>
                  </details>
                )}
              </div>
            </div>

            {/* Tab bodies */}
            {tab === "svg" && template && (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-4">
                {template.params.map((p) => (
                  <div key={p.key} className="flex flex-col gap-1">
                    <label className="text-[8px]" style={{ color: "#95a5a6" }}>
                      {p.label}
                    </label>
                    {p.type === "color" ? (
                      <div className="flex gap-2 items-center">
                        <input
                          type="color"
                          value={params[p.key] ?? p.default}
                          onChange={(e) =>
                            setParams((prev) => ({ ...prev, [p.key]: e.target.value }))
                          }
                          style={{
                            width: 28,
                            height: 22,
                            border: "none",
                            background: "transparent",
                            padding: 0,
                            cursor: "pointer",
                          }}
                        />
                        <input
                          type="text"
                          value={params[p.key] ?? p.default}
                          onChange={(e) =>
                            setParams((prev) => ({ ...prev, [p.key]: e.target.value }))
                          }
                          className="text-[9px] flex-1"
                          style={{
                            background: "#050410",
                            border: "1px solid #2a2a4a",
                            color: "#f5e6c8",
                            padding: "4px 6px",
                          }}
                          maxLength={9}
                        />
                      </div>
                    ) : (
                      <select
                        value={params[p.key] ?? p.default}
                        onChange={(e) =>
                          setParams((prev) => ({ ...prev, [p.key]: e.target.value }))
                        }
                        className="text-[9px]"
                        style={{
                          background: "#050410",
                          border: "1px solid #2a2a4a",
                          color: "#f5e6c8",
                          padding: "4px 6px",
                        }}
                      >
                        {(p.options ?? []).map((opt, i) => (
                          <option key={i} value={String(opt)}>
                            {typeof opt === "number"
                              ? `${p.label} #${opt + 1}`
                              : String(opt)}
                          </option>
                        ))}
                      </select>
                    )}
                  </div>
                ))}
              </div>
            )}

            {tab === "svg" && (
              <div className="mb-4">
                <div className="text-[8px] mb-2" style={{ color: "#95a5a6" }}>
                  {t("avatar.chooseTemplate") ?? "Choose template"}
                </div>
                <div className="flex flex-wrap gap-2">
                  {AVATAR_TEMPLATES.map((tpl) => (
                    <button
                      key={tpl.id}
                      onClick={() => setTplId(tpl.id)}
                      className="pixel-border-thin p-1"
                      style={{
                        borderColor: tpl.id === tplId ? "#f1c40f" : "#2a2a4a",
                        background: tpl.id === tplId ? "#2a2a4a" : "transparent",
                        cursor: "pointer",
                      }}
                      title={tpl.name}
                    >
                      <img
                        src={svgDataUrl(tpl.id, Object.fromEntries(
                          tpl.params.map((p) => [p.key, p.default])
                        ), 64)}
                        width={48}
                        height={48}
                        alt={tpl.name}
                        style={{ imageRendering: "pixelated", display: "block" }}
                      />
                    </button>
                  ))}
                </div>
              </div>
            )}

            {tab === "nft" && (
              <div className="flex flex-col gap-3 mb-4">
                <div className="flex flex-col gap-1">
                  <label className="text-[8px]" style={{ color: "#95a5a6" }}>
                    {t("avatar.nftContract") ?? "NFT Contract Address"}
                  </label>
                  <input
                    type="text"
                    value={nftContract}
                    onChange={(e) => setNftContract(e.target.value)}
                    placeholder="CDXYZ..."
                    className="text-[9px]"
                    style={{
                      background: "#050410",
                      border: "1px solid #2a2a4a",
                      color: "#f5e6c8",
                      padding: "4px 6px",
                    }}
                  />
                </div>
                <div className="flex flex-col gap-1">
                  <label className="text-[8px]" style={{ color: "#95a5a6" }}>
                    {t("avatar.nftTokenId") ?? "NFT Token ID"}
                  </label>
                  <input
                    type="number"
                    value={nftTokenId}
                    onChange={(e) => setNftTokenId(e.target.value)}
                    placeholder="42"
                    className="text-[9px]"
                    style={{
                      background: "#050410",
                      border: "1px solid #2a2a4a",
                      color: "#f5e6c8",
                      padding: "4px 6px",
                    }}
                  />
                </div>
                <div className="text-[7px]" style={{ color: "#7f8c8d" }}>
                  {t("avatar.nftNote") ??
                    "Only NFTs from admin-approved contracts are accepted."}
                </div>
              </div>
            )}

            {error && (
              <div className="text-[8px] mb-3 p-2" style={{ color: "#e74c3c", background: "rgba(231,76,60,0.1)" }}>
                {error}
              </div>
            )}

            <div className="flex justify-end gap-2">
              <button
                onClick={onClose}
                className="text-[9px] px-4 py-2"
                style={{
                  background: "transparent",
                  border: "1px solid #2a2a4a",
                  color: "#95a5a6",
                  cursor: "pointer",
                }}
              >
                {t("common.cancel") ?? "CANCEL"}
              </button>
              {tab === "svg" && (
                <button
                  onClick={submitSvg}
                  disabled={busy || !template}
                  className="text-[9px] px-4 py-2"
                  style={{
                    background: "#27ae60",
                    border: "none",
                    color: "#ffffff",
                    cursor: busy ? "not-allowed" : "pointer",
                    opacity: busy ? 0.6 : 1,
                  }}
                >
                  {busy
                    ? (t("common.submitting") ?? "SUBMITTING…")
                    : (t("avatar.saveSvg") ?? "SAVE SVG AVATAR")}
                </button>
              )}
              {tab === "nft" && (
                <button
                  disabled={busy || !nftContract || !nftTokenId}
                  className="text-[9px] px-4 py-2"
                  style={{
                    background: "#9b59b6",
                    border: "none",
                    color: "#ffffff",
                    cursor: busy ? "not-allowed" : "pointer",
                    opacity: busy || !nftContract || !nftTokenId ? 0.6 : 1,
                  }}
                >
                  {busy
                    ? (t("common.submitting") ?? "SUBMITTING…")
                    : (t("avatar.linkNft") ?? "LINK NFT")}
                </button>
              )}
              {tab === "none" && (
                <button
                  onClick={submitNone}
                  disabled={busy}
                  className="text-[9px] px-4 py-2"
                  style={{
                    background: "#e74c3c",
                    border: "none",
                    color: "#ffffff",
                    cursor: busy ? "not-allowed" : "pointer",
                    opacity: busy ? 0.6 : 1,
                  }}
                >
                  {busy
                    ? (t("common.submitting") ?? "SUBMITTING…")
                    : (t("avatar.resetConfirm") ?? "RESET TO IDENTICON")}
                </button>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
