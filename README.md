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

## Usage

```bash
pdf-merger a.pdf b.pdf c.pdf -o merged.pdf
```

Inputs are merged in the order given. A directory expands to the `.pdf` files inside it, sorted by name:

```bash
pdf-merger scans/ appendix.pdf -o report.pdf --bookmarks --compress
```

Inputs can also come from a text file listing one path per line. Blank lines and lines starting with `#` are skipped, and relative paths resolve against the list file's own directory. Use `-` to read the list from stdin:

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

Each input is renumbered into its own object-id range. Its pages are re-parented under one new page tree, in input order. Inherited page attributes (`Resources`, `MediaBox`, `CropBox`, `Rotate`) are copied onto each page first, so pages keep their look once the old tree is gone. The old catalog, page tree and outlines are dropped, and unreferenced objects are pruned.

## Limitations

- Encrypted PDFs are rejected; decrypt them first.
- The inputs' existing bookmarks, form fields (AcroForm), and named destinations are not carried over.
