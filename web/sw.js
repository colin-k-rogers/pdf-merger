// Cache-first service worker so the app works offline once loaded.
// Bump CACHE when shipping new files so old copies are dropped.
const CACHE = "pdf-merger-v2";
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
  if (event.request.method !== "GET") return;
  event.respondWith(
    caches.match(event.request, { ignoreSearch: true }).then((hit) => hit || fetch(event.request)),
  );
});
