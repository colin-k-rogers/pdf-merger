use std::io::Read;
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

    /// Text file listing inputs, one path per line (`-` reads stdin). Blank
    /// lines and lines starting with `#` are ignored; relative paths resolve
    /// against the list file's directory. May be repeated; listed inputs are
    /// merged before positional INPUTs.
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

    let mut sources = Vec::new();
    for list in &cli.list {
        sources.extend(read_list(list)?);
    }
    sources.extend(cli.inputs.iter().cloned());
    let paths = expand_inputs(&sources)?;
    if cli.output.exists() && !cli.force {
        bail!(
            "{} already exists (use --force to overwrite)",
            cli.output.display()
        );
    }
    let output_canon = cli.output.canonicalize().ok();
    if let Some(out) = &output_canon
        && paths
            .iter()
            .any(|p| p.canonicalize().ok().as_ref() == Some(out))
    {
        bail!("output file {} is also an input", cli.output.display());
    }

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

    let mut merged = merge(
        inputs,
        MergeOptions {
            bookmarks: cli.bookmarks,
        },
    )?;
    if cli.compress {
        merged.compress();
    }
    merged
        .save(&cli.output)
        .with_context(|| format!("failed to write {}", cli.output.display()))?;

    eprintln!(
        "merged {} files ({} pages) into {}",
        paths.len(),
        merged.get_pages().len(),
        cli.output.display()
    );
    Ok(())
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

/// One path per line; skips blank lines and `#` comments. Relative paths are
/// joined onto `base`.
fn parse_list(content: &str, base: &Path) -> Vec<PathBuf> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
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
            bail!("{} does not exist", input.display());
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
        let content = "# cover first\na.pdf\n\n  b dir/c.pdf  \n#d.pdf\n/abs/e.pdf\n";
        assert_eq!(
            parse_list(content, Path::new("lists")),
            [
                PathBuf::from("lists/a.pdf"),
                PathBuf::from("lists/b dir/c.pdf"),
                PathBuf::from("/abs/e.pdf"),
            ]
        );
    }

    #[test]
    fn parse_list_handles_crlf() {
        assert_eq!(
            parse_list("a.pdf\r\nb.pdf\r\n", Path::new("")),
            [PathBuf::from("a.pdf"), PathBuf::from("b.pdf")]
        );
    }
}
