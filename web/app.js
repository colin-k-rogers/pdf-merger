import init, { Merger } from "./pkg/pdf_merger.js";

const $ = (id) => document.getElementById(id);
const els = {
  drop: $("drop"), pick: $("pick"), file: $("file"), list: $("list"),
  pickFolder: $("pickFolder"), folder: $("folder"),
  merge: $("merge"), clear: $("clear"), bookmarks: $("bookmarks"),
  status: $("status"), install: $("install"),
};

/** @type {File[]} Files to merge, in order. */
let files = [];
let busy = false;

const ready = init();

function setStatus(text, isError = false) {
  els.status.textContent = text;
  els.status.classList.toggle("error", isError);
}

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function isPdf(file) {
  return file.type === "application/pdf" || /\.pdf$/i.test(file.name);
}

function addFiles(incoming) {
  const pdfs = [...incoming].filter(isPdf);
  const rejected = incoming.length - pdfs.length;
  files.push(...pdfs);
  render();
  setStatus(rejected ? `Ignored ${rejected} file(s) that are not PDFs.` : "", rejected > 0);
}

/** Add the PDFs directly inside a folder (not subfolders), in the order the browser lists them. */
function addFolder(name, contents) {
  const pdfs = [...contents].filter(isPdf);
  files.push(...pdfs);
  render();
  setStatus(
    pdfs.length ? "" : `No PDF files found in "${name}".`,
    pdfs.length === 0,
  );
}

const readEntries = (reader) =>
  new Promise((resolve, reject) => reader.readEntries(resolve, reject));

async function folderFiles(dirEntry) {
  const reader = dirEntry.createReader();
  const found = [];
  // readEntries returns results in batches; keep reading until it is empty.
  for (let batch = await readEntries(reader); batch.length; batch = await readEntries(reader)) {
    found.push(...batch.filter((e) => e.isFile));
  }
  return Promise.all(found.map((e) => new Promise((res, rej) => e.file(res, rej))));
}

/** Handle a drop of files and/or folders, keeping the order they were dropped in. */
async function addDropped(dataTransfer) {
  // Entries must be grabbed synchronously, before the first await.
  const entries = [...dataTransfer.items]
    .filter((i) => i.kind === "file")
    .map((i) => ({ entry: i.webkitGetAsEntry?.(), file: i.getAsFile() }));
  try {
    for (const { entry, file } of entries) {
      if (entry?.isDirectory) addFolder(entry.name, await folderFiles(entry));
      else if (file) addFiles([file]);
    }
  } catch (err) {
    setStatus(`Could not read the dropped folder: ${err?.message ?? err}`, true);
  }
}

function move(index, delta) {
  const target = index + delta;
  if (target < 0 || target >= files.length) return;
  [files[index], files[target]] = [files[target], files[index]];
  render();
}

/** Move the file at `from` so it ends up at index `to`. */
function moveTo(from, to) {
  if (from === to) return;
  files.splice(to, 0, ...files.splice(from, 1));
  render();
}

let dragFrom = null;

function render() {
  els.list.replaceChildren(
    ...files.map((file, i) => {
      const li = document.createElement("li");
      const name = document.createElement("span");
      name.className = "name";
      name.textContent = file.name;
      name.title = file.name;
      const size = document.createElement("span");
      size.className = "size";
      size.textContent = formatSize(file.size);
      li.append(name, size);
      if (!busy) {
        li.draggable = true;
        li.addEventListener("dragstart", (e) => {
          dragFrom = i;
          e.dataTransfer.effectAllowed = "move";
          e.dataTransfer.setData("text/plain", file.name);
          li.classList.add("dragging");
        });
        li.addEventListener("dragend", () => {
          dragFrom = null;
          li.classList.remove("dragging");
          for (const el of els.list.children) el.classList.remove("drop-target");
        });
        li.addEventListener("dragover", (e) => {
          if (dragFrom === null) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          for (const el of els.list.children) el.classList.toggle("drop-target", el === li);
        });
        li.addEventListener("drop", (e) => {
          if (dragFrom === null) return;
          e.preventDefault();
          moveTo(dragFrom, i);
        });
      }
      for (const [label, aria, action, disabled] of [
        ["↑", "Move up", () => move(i, -1), i === 0],
        ["↓", "Move down", () => move(i, 1), i === files.length - 1],
        ["✕", "Remove", () => { files.splice(i, 1); render(); }, false],
      ]) {
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = label;
        b.setAttribute("aria-label", `${aria}: ${file.name}`);
        b.disabled = disabled || busy;
        b.addEventListener("click", action);
        li.append(b);
      }
      return li;
    }),
  );
  els.merge.disabled = busy || files.length === 0;
  els.clear.disabled = busy || files.length === 0;
}

function download(bytes, filename) {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

async function mergeAll() {
  busy = true;
  render();
  setStatus("Merging…");
  try {
    await ready;
    const merger = new Merger();
    try {
      for (const file of files) {
        merger.add(file.name, new Uint8Array(await file.arrayBuffer()));
      }
      const bytes = merger.finish(els.bookmarks.checked);
      const skipped = merger.skipped();
      download(bytes, "merged.pdf");
      setStatus(
        skipped.length
          ? `Done. Skipped files with no pages: ${skipped.join(", ")}`
          : `Done. Merged ${files.length} file(s) into merged.pdf (${formatSize(bytes.length)}).`,
      );
    } finally {
      merger.free();
    }
  } catch (err) {
    setStatus(err?.message ?? String(err), true);
  } finally {
    busy = false;
    render();
  }
}

els.pick.addEventListener("click", () => els.file.click());
els.file.addEventListener("change", () => {
  addFiles(els.file.files);
  els.file.value = "";
});
els.pickFolder.addEventListener("click", () => els.folder.click());
els.folder.addEventListener("change", () => {
  const picked = [...els.folder.files];
  const name = picked[0]?.webkitRelativePath.split("/")[0] ?? "that folder";
  // webkitRelativePath is "folder/file.pdf" for top-level files.
  addFolder(name, picked.filter((f) => f.webkitRelativePath.split("/").length === 2));
  els.folder.value = "";
});
els.merge.addEventListener("click", mergeAll);
els.clear.addEventListener("click", () => { files = []; render(); setStatus(""); });

for (const type of ["dragenter", "dragover"]) {
  els.drop.addEventListener(type, (e) => { e.preventDefault(); els.drop.classList.add("over"); });
}
for (const type of ["dragleave", "drop"]) {
  els.drop.addEventListener(type, (e) => { e.preventDefault(); els.drop.classList.remove("over"); });
}
els.drop.addEventListener("drop", (e) => addDropped(e.dataTransfer));

// Install prompt (Chrome / ChromeOS / Android).
let installEvent = null;
window.addEventListener("beforeinstallprompt", (e) => {
  e.preventDefault();
  installEvent = e;
  els.install.style.display = "inline-block";
});
els.install.addEventListener("click", async () => {
  if (!installEvent) return;
  installEvent.prompt();
  await installEvent.userChoice;
  installEvent = null;
  els.install.style.display = "none";
});
window.addEventListener("appinstalled", () => { els.install.style.display = "none"; });

// Open PDFs handed to the installed app (file_handlers in the manifest).
if ("launchQueue" in window) {
  window.launchQueue.setConsumer(async ({ files: handles }) => {
    const opened = await Promise.all(handles.map((h) => h.getFile()));
    addFiles(opened);
  });
}

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("sw.js").catch((err) =>
    console.warn("Service worker registration failed:", err));
}

render();
