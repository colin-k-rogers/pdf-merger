//! End-to-end tests that run the built `pdf-merger` binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{Document, Object, dictionary};

/// Write a PDF with `pages` blank pages to `path`.
fn write_pdf(path: &Path, pages: usize) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let kids: Vec<Object> = (0..pages)
        .map(|_| {
            doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            })
            .into()
        })
        .collect();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => pages as i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pdf-merger"))
        .args(args)
        .output()
        .unwrap()
}

fn page_count(path: &Path) -> usize {
    Document::load(path).unwrap().get_pages().len()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        write_pdf(&f.path("a.pdf"), 2);
        write_pdf(&f.path("b.pdf"), 3);
        f
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

#[test]
fn merges_files_from_list_and_positional() {
    let f = Fixture::new();
    fs::write(f.path("list.txt"), "# comment\nb.pdf\n").unwrap();
    let out = f.path("out.pdf");
    let res = run(&[
        "-l".as_ref(),
        f.path("list.txt").as_os_str(),
        f.path("a.pdf").as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
    ]);
    assert!(res.status.success(), "{}", stderr(&res));
    assert_eq!(page_count(&out), 5);
    assert!(stderr(&res).contains("merged 2 files (5 pages)"));
}

#[test]
fn refuses_to_overwrite_without_force() {
    let f = Fixture::new();
    let out = f.path("out.pdf");
    fs::write(&out, "keep me").unwrap();
    let res = run(&[f.path("a.pdf").as_os_str(), "-o".as_ref(), out.as_os_str()]);
    assert!(!res.status.success());
    assert!(stderr(&res).contains("--force"));
    assert_eq!(fs::read_to_string(&out).unwrap(), "keep me");

    let res = run(&[
        f.path("a.pdf").as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--force".as_ref(),
    ]);
    assert!(res.status.success(), "{}", stderr(&res));
    assert_eq!(page_count(&out), 2);
}

#[test]
fn refuses_output_that_is_an_input() {
    let f = Fixture::new();
    let a = f.path("a.pdf");
    let res = run(&[
        a.as_os_str(),
        f.path("b.pdf").as_os_str(),
        "-o".as_ref(),
        a.as_os_str(),
        "-f".as_ref(),
    ]);
    assert!(!res.status.success());
    assert!(stderr(&res).contains("is also an input"));
    assert_eq!(page_count(&a), 2);
}

#[test]
fn rejects_missing_output_directory_before_loading() {
    let f = Fixture::new();
    // The input is not a valid PDF: if it were parsed first, the error would
    // be about reading it rather than about the output directory.
    fs::write(f.path("broken.pdf"), "not a pdf").unwrap();
    let res = run(&[
        f.path("broken.pdf").as_os_str(),
        "-o".as_ref(),
        f.path("missing/out.pdf").as_os_str(),
    ]);
    assert!(!res.status.success());
    assert!(
        stderr(&res).contains("output directory"),
        "{}",
        stderr(&res)
    );
}

#[test]
fn rejects_repeated_stdin_list() {
    let f = Fixture::new();
    let res = run(&[
        "-l".as_ref(),
        "-".as_ref(),
        "-l".as_ref(),
        "-".as_ref(),
        "-o".as_ref(),
        f.path("out.pdf").as_os_str(),
    ]);
    assert!(!res.status.success());
    assert!(stderr(&res).contains("only be given once"));
}

#[test]
fn reports_skipped_empty_inputs_in_summary() {
    let f = Fixture::new();
    write_pdf(&f.path("empty.pdf"), 0);
    let out = f.path("out.pdf");
    let res = run(&[
        f.path("a.pdf").as_os_str(),
        f.path("empty.pdf").as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
    ]);
    assert!(res.status.success(), "{}", stderr(&res));
    let err = stderr(&res);
    assert!(err.contains("empty has no pages; skipped"));
    assert!(err.contains("merged 1 files (2 pages)"));
}
