export type SeatPrivacyMode = "alias" | "address" | "anonymous";

const STORAGE_KEY = "stellpoker:seat-privacy-mode";

export function getSeatPrivacyMode(): SeatPrivacyMode {
  if (typeof window === "undefined") return "alias";
  const value = window.localStorage.getItem(STORAGE_KEY);
  return value === "address" || value === "anonymous" ? value : "alias";
}

export function setSeatPrivacyMode(mode: SeatPrivacyMode): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(STORAGE_KEY, mode);
}

export function displaySeatName({
  address,
  alias,
  seat,
  isUser,
  mode,
}: {
  address: string;
  alias?: string;
  seat: number;
  isUser: boolean;
  mode: SeatPrivacyMode;
}): string {
  if (mode === "anonymous") return isUser ? "YOU" : `Player ${seat + 1}`;
  if (mode === "alias" && alias) return isUser ? `${alias} (YOU)` : alias;
  return isUser ? "YOU" : `${address.slice(0, 6)}...${address.slice(-6)}`;
}
