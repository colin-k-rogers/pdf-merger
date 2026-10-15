//! Merge many PDF documents into a single document.
//!
//! Each input is renumbered into a disjoint object-id range, its pages are
//! re-parented under one new page tree (in input order), and its old
//! catalog / page-tree / outline nodes are dropped. Unreferenced objects are
//! pruned before the result is returned.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use lopdf::{Bookmark, Document, Object, ObjectId, dictionary};

/// Page attributes that a `Page` may inherit from its ancestor `Pages` nodes
/// (PDF 32000-1:2008, section 7.7.3.4). These must be copied onto each page
/// because the original page tree is discarded.
const INHERITABLE_ATTRS: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// One document to merge, with the label used for its bookmark.
pub struct Input {
    pub label: String,
    pub doc: Document,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MergeOptions {
    /// Add a top-level bookmark per input pointing at its first page.
    pub bookmarks: bool,
}

/// Merge `inputs` in order into a single new document.
pub fn merge(inputs: Vec<Input>, opts: MergeOptions) -> Result<Document> {
    if inputs.is_empty() {
        bail!("no input documents given");
    }

    let version = inputs
        .iter()
        .map(|i| i.doc.version.as_str())
        .max_by(|a, b| version_key(a).cmp(&version_key(b)))
        .unwrap_or("1.5")
        .to_string();
    let mut out = Document::with_version(version);
    let pages_id = out.new_object_id();
    let mut kids: Vec<Object> = Vec::new();

    for Input { label, mut doc } in inputs {
        doc.renumber_objects_with(out.max_id + 1);

        let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if page_ids.is_empty() {
            eprintln!("warning: {label} has no pages; skipping");
            continue;
        }

        // Flatten inherited attributes onto each page before the tree goes away.
        for &page_id in &page_ids {
            let inherited = collect_inherited(&doc, page_id);
            let page = doc
                .get_object_mut(page_id)
                .and_then(Object::as_dict_mut)
                .with_context(|| format!("{label}: page {page_id:?} is not a dictionary"))?;
            for (key, value) in inherited {
                page.set(key, value);
            }
            page.set("Parent", pages_id);
        }

        let skip = structural_ids(&doc, &page_ids);
        let max_id = doc.max_id;
        for (id, obj) in doc.objects {
            if !skip.contains(&id) {
                out.objects.insert(id, obj);
            }
        }
        out.max_id = out.max_id.max(max_id);

        if opts.bookmarks {
            out.add_bookmark(Bookmark::new(label, [0.0, 0.0, 0.0], 0, page_ids[0]), None);
        }
        kids.extend(page_ids.into_iter().map(Object::Reference));
    }

    if kids.is_empty() {
        bail!("none of the input documents contain any pages");
    }

    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => kids.len() as i64,
            "Kids" => kids,
        }),
    );
    let mut catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    if let Some(outline_id) = out.build_outline() {
        catalog.set("Outlines", outline_id);
        catalog.set("PageMode", "UseOutlines");
    }
    let catalog_id = out.add_object(catalog);
    out.trailer.set("Root", catalog_id);

    out.prune_objects();
    out.renumber_objects();
    Ok(out)
}

/// Inheritable attributes missing from `page_id` but present on an ancestor.
fn collect_inherited(doc: &Document, page_id: ObjectId) -> Vec<(Vec<u8>, Object)> {
    let Ok(page) = doc.get_dictionary(page_id) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut seen = HashSet::from([page_id]);
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    while let Some(parent_id) = parent {
        if !seen.insert(parent_id) {
            break; // malformed cyclic tree
        }
        let Ok(node) = doc.get_dictionary(parent_id) else {
            break;
        };
        for key in INHERITABLE_ATTRS {
            let already = page.has(key) || found.iter().any(|(k, _): &(Vec<u8>, _)| k == key);
            if !already && let Ok(value) = node.get(key) {
                found.push((key.to_vec(), value.clone()));
            }
        }
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    found
}

/// Ids of the old catalog, intermediate page-tree nodes and outline items,
/// which are replaced by the merged document's own structure.
fn structural_ids(doc: &Document, page_ids: &[ObjectId]) -> HashSet<ObjectId> {
    let pages: HashSet<_> = page_ids.iter().copied().collect();
    doc.objects
        .iter()
        .filter(|(id, obj)| {
            !pages.contains(id)
                && matches!(
                    obj.type_name().unwrap_or(b""),
                    b"Catalog" | b"Pages" | b"Outlines" | b"Outline"
                )
        })
        .map(|(id, _)| *id)
        .collect()
}

fn version_key(v: &str) -> (u32, u32) {
    let mut parts = v.split('.').map(|p| p.trim().parse().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Stream;
    use lopdf::content::{Content, Operation};

    /// Build a document with `n` pages whose MediaBox is set only on the
    /// `Pages` root, so pages rely on inheritance.
    fn sample(n: usize, text: &str) -> Document {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut kids = Vec::new();
        for i in 0..n {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 24.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::string_literal(format!("{text} {i}"))]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            kids.push(
                doc.add_object(dictionary! {
                    "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
                })
                .into(),
            );
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => n as i64,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    fn input(label: &str, doc: Document) -> Input {
        Input {
            label: label.into(),
            doc,
        }
    }

    fn page_texts(doc: &Document) -> Vec<String> {
        doc.get_pages()
            .keys()
            .map(|&n| doc.extract_text(&[n]).unwrap().trim().to_string())
            .collect()
    }

    #[test]
    fn merges_pages_in_input_order() {
        let merged = merge(
            vec![
                input("a", sample(2, "A")),
                input("b", sample(1, "B")),
                input("c", sample(3, "C")),
            ],
            MergeOptions::default(),
        )
        .unwrap();
        assert_eq!(
            page_texts(&merged),
            ["A 0", "A 1", "B 0", "C 0", "C 1", "C 2"]
        );
    }

    #[test]
    fn copies_inherited_attributes_onto_pages() {
        let merged = merge(vec![input("a", sample(2, "A"))], MergeOptions::default()).unwrap();
        for page_id in merged.get_pages().into_values() {
            let page = merged.get_dictionary(page_id).unwrap();
            assert!(page.has(b"MediaBox"));
            assert!(page.has(b"Resources"));
        }
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let mut merged = merge(
            vec![input("a", sample(1, "A")), input("b", sample(1, "B"))],
            MergeOptions::default(),
        )
        .unwrap();
        let mut buf = Vec::new();
        merged.save_to(&mut buf).unwrap();
        let reloaded = Document::load_mem(&buf).unwrap();
        assert_eq!(page_texts(&reloaded), ["A 0", "B 0"]);
    }

    #[test]
    fn adds_one_bookmark_per_input() {
        let merged = merge(
            vec![
                input("first", sample(2, "A")),
                input("second", sample(1, "B")),
            ],
            MergeOptions { bookmarks: true },
        )
        .unwrap();
        let catalog = merged.catalog().unwrap();
        let outlines = merged
            .get_dictionary(catalog.get(b"Outlines").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(outlines.get(b"Count").unwrap().as_i64().unwrap(), 2);
    }

    #[test]
    fn rejects_empty_input() {
        assert!(merge(vec![], MergeOptions::default()).is_err());
    }
}
