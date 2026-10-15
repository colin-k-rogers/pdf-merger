use std::fs;
use std::io::{BufWriter, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Parser};
use lopdf::Document;
use pdf_merger::{Input, MergeOptions, merge};

/// Merge many PDF documents into a single PDF, preserving input order.
#[derive(Parser, Debug)]
#[command(version, about)]
#[command(group(ArgGroup::new("sources").required(true).multiple(true).args(["inputs", "list"])))]
struct Cli {
    /// Input PDF files, merged in the order given. A directory expands to the
    /// `.pdf` files it contains, sorted by name.
    #[arg(value_name = "INPUT")]
    inputs: Vec<PathBuf>,

    /// Text file listing inputs, one path per line (`-` reads stdin, at most
    /// once). Blank lines and lines starting with `#` are ignored (write
    /// `./#name.pdf` for a file whose name starts with `#`); relative paths
    /// resolve against the list file's directory. May be repeated; listed
    /// inputs are merged before positional INPUTs.
    #[arg(short, long, value_name = "FILE")]
    list: Vec<PathBuf>,

    /// Output file path.
    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,

    /// Overwrite the output file if it already exists.
    #[arg(short, long)]
    force: bool,

    /// Add a bookmark for each input file, named after the file.
    #[arg(short, long)]
    bookmarks: bool,

    /// Compress streams in the output.
    #[arg(short, long)]
    compress: bool,

    /// Print progress to stderr.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.list.iter().filter(|l| *l == Path::new("-")).count() > 1 {
        bail!("`--list -` can only be given once (stdin can only be read once)");
    }
    let mut sources = Vec::new();
    for list in &cli.list {
        sources.extend(read_list(list)?);
    }
    sources.extend(cli.inputs.iter().cloned());
    let paths = expand_inputs(&sources)?;
    check_output(&cli.output, &paths, cli.force)?;

    let mut inputs = Vec::with_capacity(paths.len());
    for path in &paths {
        let doc =
            Document::load(path).with_context(|| format!("failed to read {}", path.display()))?;
        if doc.is_encrypted() {
            bail!("{} is encrypted; decrypt it before merging", path.display());
        }
        if cli.verbose {
            eprintln!(
                "loaded {} ({} pages)",
                path.display(),
                doc.get_pages().len()
            );
        }
        let label = path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        inputs.push(Input { label, doc });
    }

    let mut result = merge(
        inputs,
        MergeOptions {
            bookmarks: cli.bookmarks,
        },
    )?;
    for label in &result.skipped {
        eprintln!("warning: {label} has no pages; skipped");
    }
    if cli.compress {
        result.doc.compress();
    }
    save_atomically(&mut result.doc, &cli.output)?;

    eprintln!(
        "merged {} files ({} pages) into {}",
        result.merged.len(),
        result.doc.get_pages().len(),
        cli.output.display()
    );
    Ok(())
}

/// Fail fast, before any input is parsed, if the output can't or shouldn't
/// be written.
fn check_output(output: &Path, inputs: &[PathBuf], force: bool) -> Result<()> {
    if output.is_dir() {
        bail!("output {} is a directory", output.display());
    }
    let parent = output_dir(output);
    if !parent.is_dir() {
        bail!("output directory {} does not exist", parent.display());
    }
    if output.exists() {
        if !force {
            bail!(
                "{} already exists (use --force to overwrite)",
                output.display()
            );
        }
        let out = output.canonicalize()?;
        if inputs
            .iter()
            .any(|p| p.canonicalize().is_ok_and(|p| p == out))
        {
            bail!("output file {} is also an input", output.display());
        }
    }
    Ok(())
}

/// Write to a temporary file next to `output`, then rename it into place, so
/// a failed write never leaves a truncated file or clobbers the old one.
fn save_atomically(doc: &mut Document, output: &Path) -> Result<()> {
    let name = output.file_name().unwrap_or_default().to_string_lossy();
    let tmp = output_dir(output).join(format!(".{name}.{}.tmp", std::process::id()));
    let mut write = || -> Result<()> {
        let mut file = BufWriter::new(fs::File::create(&tmp)?);
        doc.save_to(&mut file)?;
        file.into_inner()?.sync_all()?;
        fs::rename(&tmp, output)?;
        Ok(())
    };
    write().map_err(|err| {
        let _ = fs::remove_file(&tmp);
        err.context(format!("failed to write {}", output.display()))
    })
}

fn output_dir(output: &Path) -> &Path {
    match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// Read a list file (or stdin for `-`) into input paths.
fn read_list(list: &Path) -> Result<Vec<PathBuf>> {
    let (content, base) = if list == Path::new("-") {
        let mut content = String::new();
        std::io::stdin()
            .read_to_string(&mut content)
            .context("failed to read input list from stdin")?;
        (content, Path::new(""))
    } else {
        let content = std::fs::read_to_string(list)
            .with_context(|| format!("failed to read input list {}", list.display()))?;
        (content, list.parent().unwrap_or(Path::new("")))
    };
    let paths = parse_list(&content, base);
    if paths.is_empty() {
        bail!("input list {} contains no paths", list.display());
    }
    Ok(paths)
}

/// One path per line, taken verbatim (spaces are part of the name); skips
/// whitespace-only lines and lines starting with `#`. Relative paths are
/// joined onto `base`.
fn parse_list(content: &str, base: &Path) -> Vec<PathBuf> {
    content
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| base.join(line))
        .collect()
}

/// Expand directories into their sorted `.pdf` entries; keep files as given.
fn expand_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(input)
                .with_context(|| format!("failed to read directory {}", input.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && is_pdf(p))
                .collect();
            entries.sort();
            if entries.is_empty() {
                bail!("directory {} contains no PDF files", input.display());
            }
            paths.extend(entries);
        } else if input.is_file() {
            paths.push(input.clone());
        } else {
            bail!("{:?} does not exist", input);
        }
    }
    Ok(paths)
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_skips_blanks_and_comments() {
        let content = "# cover first\na.pdf\n\n   \nb dir/c.pdf\n#d.pdf\n./#e.pdf\n/abs/f.pdf\n";
        assert_eq!(
            parse_list(content, Path::new("lists")),
            [
                PathBuf::from("lists/a.pdf"),
                PathBuf::from("lists/b dir/c.pdf"),
                PathBuf::from("lists/./#e.pdf"),
                PathBuf::from("/abs/f.pdf"),
            ]
        );
    }

    #[test]
    fn parse_list_keeps_spaces_in_names() {
        assert_eq!(
            parse_list(" lead.pdf\ntrail.pdf \n", Path::new("")),
            [PathBuf::from(" lead.pdf"), PathBuf::from("trail.pdf ")]
        );
    }

    #[test]
    fn parse_list_handles_crlf() {
        assert_eq!(
            parse_list("a.pdf\r\nb.pdf\r\n", Path::new("")),
            [PathBuf::from("a.pdf"), PathBuf::from("b.pdf")]
        );
    }

    #[test]
    fn read_list_resolves_against_list_directory() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("files.txt");
        fs::write(&list, "a.pdf\nsub/b.pdf\n").unwrap();
        assert_eq!(
            read_list(&list).unwrap(),
            [dir.path().join("a.pdf"), dir.path().join("sub/b.pdf")]
        );
    }

    #[test]
    fn read_list_rejects_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("files.txt");
        fs::write(&list, "# nothing here\n\n").unwrap();
        assert!(read_list(&list).is_err());
    }

    #[test]
    fn expand_inputs_sorts_directory_pdfs_and_skips_others() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.pdf", "a.PDF", "notes.txt"] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        fs::create_dir(dir.path().join("nested.pdf")).unwrap();
        let extra = dir.path().join("notes.txt");
        assert_eq!(
            expand_inputs(&[dir.path().to_path_buf(), extra.clone()]).unwrap(),
            [dir.path().join("a.PDF"), dir.path().join("b.pdf"), extra]
        );
    }

    #[test]
    fn expand_inputs_rejects_missing_and_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(expand_inputs(&[dir.path().to_path_buf()]).is_err());
        assert!(expand_inputs(&[dir.path().join("missing.pdf")]).is_err());
    }
}
