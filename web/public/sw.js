const CACHE_NAME = "remora-v1";
const SHELL_ASSETS = ["/"];
// Only cache static asset types — prevents API response caching and cache poisoning
const CACHEABLE_TYPES = ["text/html", "application/javascript", "text/css", "image/svg+xml", "image/png"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(SHELL_ASSETS))
  );
  // Don't skipWaiting — let the user refresh naturally to avoid version skew
  // between cached assets and running JavaScript.
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((names) =>
      Promise.all(names.filter((n) => n !== CACHE_NAME).map((n) => caches.delete(n)))
    )
  );
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);

  // Don't intercept API calls, WebSocket upgrades, or non-same-origin requests
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/sessions") || url.pathname === "/health") return;
  if (event.request.method !== "GET") return;

  event.respondWith(
    fetch(event.request)
      .then((response) => {
        // Only cache responses with safe content types
        if (response.ok) {
          const contentType = response.headers.get("content-type") || "";
          const isCacheable = CACHEABLE_TYPES.some((t) => contentType.includes(t));
          if (isCacheable) {
            const clone = response.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          }
        }
        return response;
      })
      .catch(() => {
        // Only fall back to cached root for navigation requests (not JS/CSS/images)
        if (event.request.mode === "navigate") {
          return caches.match("/");
        }
        return caches.match(event.request);
      })
  );
});
