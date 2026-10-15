# pdf-merger

A small Rust CLI that merges many PDF documents into a single PDF, built on [`lopdf`](https://crates.io/crates/lopdf).

## Install

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
