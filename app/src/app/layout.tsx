import type { Metadata, Viewport } from "next";
import "./globals.css";
import { I18nProvider } from "@/lib/i18n/context";
import { ThemeToggle } from "@/components/ThemeToggle";
import { NotificationsCenter } from "@/components/NotificationsCenter";
import { ReducedDataToggle } from "@/components/ReducedDataToggle";
import { PwaRegistrar } from "@/components/PwaRegistrar";
import { OfflineBanner } from "@/components/OfflineBanner";

/** Metadata.
 * @param Safari - Safari.
 */
export const metadata: Metadata = {
  title: "Poker on Stellar",
  description: "Onchain poker with private cards via MPC + ZK proofs on Stellar",
};

// Ensure the viewport is correctly sized on mobile browsers and that
// Freighter's in-app browser (Safari/Chrome WebView) doesn't auto-zoom
// input fields by keeping font-size at 16px minimum (#18).
/** Viewport */
export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  maximumScale: 1,
  userScalable: false,
  themeColor: "#0c0a18",
};

/** Default.
 * @param props(object) - props(object).
 */
export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>
        <I18nProvider>
          <div className="app-top-controls" style={{ position: "absolute", top: 8, insetInlineEnd: 8, zIndex: 9999 }}>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <NotificationsCenter />
              <ThemeToggle />
              <ReducedDataToggle />
            </div>
          </div>
          {children}
          <OfflineBanner />
          <PwaRegistrar />
        </I18nProvider>
      </body>
    </html>
  );
}
