/*
 * Service worker for the installable app shell (#632).
 *
 * Caches the built static assets and the last-seen home page so the app opens
 * offline. It does not cache or replay anything that changes state:
 *   - Only GET requests are handled. Transaction submissions and every other
 *     write (POST) go straight to the network, so an offline action fails
 *     instead of being queued.
 *   - Only same-origin requests are handled. The coordinator, the Soroban RPC
 *     and WebSockets are never cached.
 *   - API paths are never cached, so table state is always live.
 */

const CACHE_VERSION = "v1";
const CACHE_PREFIX = "stellar-poker-shell-";
const CACHE_NAME = `${CACHE_PREFIX}${CACHE_VERSION}`;
const SHELL_URLS = ["/", "/icon.svg", "/icons/icon-192.png", "/icons/icon-512.png"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) =>
      // Precache one URL at a time so one failed fetch does not block install.
      Promise.all(
        SHELL_URLS.map((url) =>
          fetch(url, { cache: "reload" })
            .then((res) => (res.ok ? cache.put(url, res) : undefined))
            .catch(() => undefined)
        )
      )
    )
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key.startsWith(CACHE_PREFIX) && key !== CACHE_NAME)
            .map((key) => caches.delete(key))
        )
      )
      .then(() => self.clients.claim())
  );
});

function isStaticAsset(pathname) {
  return (
    pathname.startsWith("/_next/static/") ||
    pathname.startsWith("/cat_sprites/") ||
    pathname.startsWith("/icons/") ||
    pathname === "/icon.svg"
  );
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE_NAME);
  const cached = await cache.match(request);
  if (cached) return cached;
  const res = await fetch(request);
  if (res.ok) cache.put(request, res.clone());
  return res;
}

async function networkFirstPage(request) {
  const cache = await caches.open(CACHE_NAME);
  try {
    const res = await fetch(request);
    if (res.ok) cache.put(request, res.clone());
    return res;
  } catch {
    const cached = (await cache.match(request)) || (await cache.match("/"));
    if (cached) return cached;
    return new Response("You are offline and this page is not cached yet.", {
      status: 503,
      headers: { "Content-Type": "text/plain; charset=utf-8" },
    });
  }
}

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/api/")) return;

  if (request.mode === "navigate") {
    event.respondWith(networkFirstPage(request));
    return;
  }

  if (isStaticAsset(url.pathname)) {
    event.respondWith(cacheFirst(request));
  }
});
