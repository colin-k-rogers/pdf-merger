//! Merge many PDF documents into a single document.
//!
//! For each input, only the objects reachable from its pages are copied,
//! shifted into a disjoint object-id range, and the pages are re-parented
//! under one new page tree (in input order). The inputs' old catalog,
//! page-tree and outline nodes are left behind. Every step is linear in the
//! number of objects, so merging many large files stays fast.

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

/// The merged document plus what happened to each input.
pub struct Merged {
    pub doc: Document,
    /// Labels of the inputs that were merged, in order.
    pub merged: Vec<String>,
    /// Labels of inputs skipped because they have no pages.
    pub skipped: Vec<String>,
}

/// Merge `inputs` in order into a single new document.
pub fn merge(inputs: Vec<Input>, opts: MergeOptions) -> Result<Merged> {
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
    let mut merged = Vec::new();
    let mut skipped = Vec::new();

    for Input { label, mut doc } in inputs {
        let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if page_ids.is_empty() {
            skipped.push(label);
            continue;
        }

        // Flatten inherited attributes onto each page and detach it from the
        // old tree, so the reachability walk below doesn't pull the tree in.
        for &page_id in &page_ids {
            let inherited = collect_inherited(&doc, page_id);
            let page = doc
                .get_object_mut(page_id)
                .and_then(Object::as_dict_mut)
                .with_context(|| format!("{label}: page {page_id:?} is not a dictionary"))?;
            for (key, value) in inherited {
                page.set(key, value);
            }
            page.remove(b"Parent");
        }

        let offset = out.max_id;
        let shift = |id: ObjectId| (id.0 + offset, id.1);
        for id in reachable(&doc, &page_ids) {
            let Some(mut obj) = doc.objects.remove(&id) else {
                continue;
            };
            shift_references(&mut obj, offset);
            let new_id = shift(id);
            out.max_id = out.max_id.max(new_id.0);
            out.objects.insert(new_id, obj);
        }

        let page_ids: Vec<ObjectId> = page_ids.into_iter().map(shift).collect();
        for &page_id in &page_ids {
            if let Ok(page) = out.get_object_mut(page_id).and_then(Object::as_dict_mut) {
                page.set("Parent", pages_id);
            }
        }
        if opts.bookmarks {
            out.add_bookmark(
                Bookmark::new(label.clone(), [0.0, 0.0, 0.0], 0, page_ids[0]),
                None,
            );
        }
        kids.extend(page_ids.into_iter().map(Object::Reference));
        merged.push(label);
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

    Ok(Merged {
        doc: out,
        merged,
        skipped,
    })
}

/// Inheritable attributes missing from `page_id` but present on an ancestor.
/// The nearest ancestor wins.
fn collect_inherited(doc: &Document, page_id: ObjectId) -> Vec<(Vec<u8>, Object)> {
    let Ok(page) = doc.get_dictionary(page_id) else {
        return Vec::new();
    };
    let mut missing: Vec<&[u8]> = INHERITABLE_ATTRS
        .into_iter()
        .filter(|k| !page.has(k))
        .collect();
    let mut found = Vec::new();
    let mut seen = HashSet::from([page_id]);
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    while let Some(parent_id) = parent
        && !missing.is_empty()
        && seen.insert(parent_id)
    {
        let Ok(node) = doc.get_dictionary(parent_id) else {
            break;
        };
        missing.retain(|key| match node.get(key) {
            Ok(value) => {
                found.push((key.to_vec(), value.clone()));
                false
            }
            Err(_) => true,
        });
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    found
}

/// Ids of every object reachable from `page_ids`, not crossing into the old
/// catalog, page-tree or outline nodes (which the merged document replaces).
fn reachable(doc: &Document, page_ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut seen: HashSet<ObjectId> = page_ids.iter().copied().collect();
    let mut stack: Vec<ObjectId> = page_ids.to_vec();
    let mut order = Vec::new();
    while let Some(id) = stack.pop() {
        let Ok(obj) = doc.get_object(id) else {
            continue;
        };
        order.push(id);
        collect_references(obj, &mut |child| {
            let structural = doc.get_object(child).is_ok_and(|o| {
                matches!(
                    o.type_name().unwrap_or(b""),
                    b"Catalog" | b"Pages" | b"Outlines" | b"Outline"
                )
            });
            if !structural && seen.insert(child) {
                stack.push(child);
            }
        });
    }
    order
}

fn collect_references(obj: &Object, f: &mut impl FnMut(ObjectId)) {
    match obj {
        Object::Reference(id) => f(*id),
        Object::Array(items) => items.iter().for_each(|o| collect_references(o, f)),
        Object::Dictionary(dict) => dict.iter().for_each(|(_, o)| collect_references(o, f)),
        Object::Stream(stream) => stream
            .dict
            .iter()
            .for_each(|(_, o)| collect_references(o, f)),
        _ => {}
    }
}

fn shift_references(obj: &mut Object, offset: u32) {
    match obj {
        Object::Reference(id) => id.0 += offset,
        Object::Array(items) => items.iter_mut().for_each(|o| shift_references(o, offset)),
        Object::Dictionary(dict) => dict
            .iter_mut()
            .for_each(|(_, o)| shift_references(o, offset)),
        Object::Stream(stream) => stream
            .dict
            .iter_mut()
            .for_each(|(_, o)| shift_references(o, offset)),
        _ => {}
    }
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
        .unwrap()
        .doc;
        assert_eq!(
            page_texts(&merged),
            ["A 0", "A 1", "B 0", "C 0", "C 1", "C 2"]
        );
    }

    #[test]
    fn copies_inherited_attributes_onto_pages() {
        let merged = merge(vec![input("a", sample(2, "A"))], MergeOptions::default())
            .unwrap()
            .doc;
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
        .unwrap()
        .doc;
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
        .unwrap()
        .doc;
        let catalog = merged.catalog().unwrap();
        let outlines = merged
            .get_dictionary(catalog.get(b"Outlines").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(outlines.get(b"Count").unwrap().as_i64().unwrap(), 2);
    }

    #[test]
    fn reports_inputs_without_pages_as_skipped() {
        let result = merge(
            vec![input("a", sample(1, "A")), input("empty", sample(0, "E"))],
            MergeOptions::default(),
        )
        .unwrap();
        assert_eq!(result.merged, ["a"]);
        assert_eq!(result.skipped, ["empty"]);
        assert_eq!(page_texts(&result.doc), ["A 0"]);
    }

    #[test]
    fn leaves_old_catalogs_and_page_trees_behind() {
        let merged = merge(
            vec![input("a", sample(2, "A")), input("b", sample(2, "B"))],
            MergeOptions::default(),
        )
        .unwrap()
        .doc;
        let count = |ty: &[u8]| {
            merged
                .objects
                .values()
                .filter(|o| o.type_name().is_ok_and(|t| t == ty))
                .count()
        };
        assert_eq!(count(b"Catalog"), 1);
        assert_eq!(count(b"Pages"), 1);
        assert_eq!(count(b"Page"), 4);
    }

    #[test]
    fn rejects_empty_input() {
        assert!(merge(vec![], MergeOptions::default()).is_err());
    }
}
