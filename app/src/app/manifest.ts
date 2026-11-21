import type { MetadataRoute } from "next";

/**
 * Web app manifest (#632). Lets players install the table to their home
 * screen. Served by Next at `/manifest.webmanifest`.
 */
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "Poker on Stellar",
    short_name: "Stellar Poker",
    description: "Onchain poker with private cards via MPC + ZK proofs on Stellar",
    start_url: "/",
    scope: "/",
    display: "standalone",
    orientation: "portrait",
    background_color: "#0c0a18",
    theme_color: "#0c0a18",
    icons: [
      {
        src: "/icons/icon-192.png",
        sizes: "192x192",
        type: "image/png",
        purpose: "any",
      },
      {
        src: "/icons/icon-512.png",
        sizes: "512x512",
        type: "image/png",
        purpose: "any",
      },
      {
        src: "/icon.svg",
        sizes: "any",
        type: "image/svg+xml",
        purpose: "any",
      },
    ],
  };
}
