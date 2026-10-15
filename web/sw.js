// Network-first service worker: online loads always get the newest files and
// refresh the cache; offline loads fall back to the cache. The precache below
// makes the first offline load work. Bump CACHE only when SHELL changes.
const CACHE = "pdf-merger-v3";
const SHELL = [
  "./",
  "index.html",
  "app.js",
  "manifest.webmanifest",
  "pkg/pdf_merger.js",
  "pkg/pdf_merger_bg.wasm",
  "icons/icon-192.png",
  "icons/icon-512.png",
  "icons/icon-maskable-512.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET" || new URL(request.url).origin !== self.location.origin) return;
  event.respondWith(
    fetch(request, { cache: "no-cache" })
      .then((response) => {
        // Only full 200s are cacheable (cache.put rejects 206 partials). waitUntil
        // keeps the worker alive until the write lands, or it could be killed
        // right after the response is returned and the cache would go stale.
        if (response.status === 200) {
          const copy = response.clone();
          event.waitUntil(caches.open(CACHE).then((c) => c.put(request, copy)));
        }
        return response;
      })
      .catch(() => caches.match(request, { ignoreSearch: true })),
  );
});
