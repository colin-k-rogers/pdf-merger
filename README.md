# pdf-merger

A small Rust CLI that merges many PDF documents into a single PDF, built on [`lopdf`](https://crates.io/crates/lopdf).

## Install

### Prebuilt binary (Linux, including Chromebooks)

On a Chromebook, first turn on Linux in Settings → Advanced → Developers → Linux development environment, then open the Terminal. Download the build matching `uname -m` (`x86_64` or `aarch64`) from the [latest release](https://github.com/colin-k-rogers/pdf-merger/releases/latest):

```bash
curl -L https://github.com/colin-k-rogers/pdf-merger/releases/latest/download/pdf-merger-$(uname -m)-unknown-linux-musl.tar.gz | tar xz
sudo mv pdf-merger /usr/local/bin/
pdf-merger --version
```

The binary is statically linked, so it has no dependencies. Files in your Chromebook's Downloads or Google Drive are visible to Linux under `/mnt/chromeos/` once you share the folder (Files app → right-click the folder → Share with Linux).

### From source

Requires a [Rust toolchain](https://rustup.rs) (on Debian/Chromebook, also `sudo apt install build-essential`):

```bash
cargo install --git https://github.com/colin-k-rogers/pdf-merger
```

Or from a local checkout:

```bash
cargo install --path .
```

## Web app (installable PWA)

The same merge code compiles to WebAssembly and runs in the browser, so
Chromebooks, phones, and desktops can merge PDFs with no install and no upload.
Everything happens on the device, and once installed it works offline. Pick
individual PDFs or a whole folder (its top-level PDFs, in the order the browser
lists them), or drag either onto the page. Reorder the list by dragging rows or
with the ↑/↓ buttons before merging.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # must match Cargo.lock
./scripts/build-web.sh
python3 -m http.server -d web 8000                 # then open http://localhost:8000
```

Serve the `web/` directory from any static host over HTTPS (for example GitHub
Pages) and Chrome will offer to install it. The service worker is network-first,
so online users always get the latest files; bump `CACHE` in `web/sw.js` only if
you change its precache list.

Once Pages is enabled (below), every merge to `main` that touches the app
deploys it automatically, so users just open the URL, no build needed.

Each [release](https://github.com/colin-k-rogers/pdf-merger/releases/latest)
also includes `pdf-merger-web-<version>.zip`: the built app, ready to unzip onto
any static HTTPS host. (Opening `index.html` straight from disk won't work,
because browsers block WebAssembly and service workers on `file://`.)

## Usage

```bash
pdf-merger a.pdf b.pdf c.pdf -o merged.pdf
```

Inputs are merged in the order given. A directory expands to the `.pdf` files inside it, sorted by name:

```bash
pdf-merger scans/ appendix.pdf -o report.pdf --bookmarks --compress
```

Inputs can also come from a text file listing one path per line. Each line is taken exactly as written, spaces included. Blank lines and lines starting with `#` are skipped; write `./#1.pdf` to list a file whose name starts with `#`. Relative paths resolve against the list file's own directory. Use `-` to read the list from stdin (once per run):

```bash
pdf-merger --list chapters.txt -o book.pdf
```

```bash
ls *.pdf | pdf-merger --list - -o merged.pdf
```

`--list` can be repeated and combined with positional inputs; listed inputs come first.

| Flag | Description |
| --- | --- |
| `-l, --list <FILE>` | Text file of input paths, one per line (`-` for stdin) |
| `-o, --output <FILE>` | Output path (required) |
| `-f, --force` | Overwrite the output if it exists |
| `-b, --bookmarks` | Add a bookmark per input file, named after the file |
| `-c, --compress` | Compress streams in the output |
| `-v, --verbose` | Print per-file progress |

## How it works

For each input, only the objects reachable from its pages are copied, shifted into their own object-id range. The pages are re-parented under one new page tree, in input order. Every step is linear in the number of objects, so merging hundreds of files stays fast. Inherited page attributes (`Resources`, `MediaBox`, `CropBox`, `Rotate`) are copied onto each page first, so pages keep their look once the old tree is gone. The old catalog, page tree and outlines are left behind. The output is written to a temporary file and renamed into place, so a failed write never leaves a truncated file.

## Limitations

- Encrypted PDFs are rejected; decrypt them first.
- The inputs' existing bookmarks, form fields (AcroForm), and named destinations are not carried over.

## Releasing

Releases are built in CI, so nothing needs compiling locally:

1. Bump `version` in `Cargo.toml`, commit, and merge to `main`.
2. Tag and push: `git tag v<version> && git push origin v<version>`.

The [Release workflow](.github/workflows/release.yml) builds the static Linux
binaries and the web zip and publishes the GitHub release. You can also run it
from the Actions tab to build without publishing. `scripts/release.sh` does the
same thing locally if you prefer (needs zig, cargo-zigbuild, wasm-bindgen-cli).

One-time setup for the hosted web app: Settings → Pages → Source: **GitHub
Actions**. The [Pages workflow](.github/workflows/pages.yml) then deploys `web/`.
