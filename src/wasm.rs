//! Browser bindings, compiled only for `wasm32`. The page feeds in PDF bytes
//! and gets the merged PDF bytes back; nothing touches a filesystem.

use lopdf::Document;
use wasm_bindgen::prelude::*;

use crate::{Input, MergeOptions, merge};

/// Collects PDFs in order, then merges them.
#[wasm_bindgen]
#[derive(Default)]
pub struct Merger {
    inputs: Vec<Input>,
    skipped: Vec<String>,
}

#[wasm_bindgen]
impl Merger {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Merger {
        Merger::default()
    }

    /// Parse `bytes` as a PDF named `name`. Fails if it isn't a readable PDF.
    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<(), JsError> {
        let doc = Document::load_mem(bytes)
            .map_err(|e| JsError::new(&format!("{name}: not a readable PDF ({e})")))?;
        self.inputs.push(Input {
            label: name.to_string(),
            doc,
        });
        Ok(())
    }

    /// Merge everything added so far and return the PDF bytes.
    pub fn finish(&mut self, bookmarks: bool) -> Result<Vec<u8>, JsError> {
        let inputs = std::mem::take(&mut self.inputs);
        let mut merged = merge(inputs, MergeOptions { bookmarks })
            .map_err(|e| JsError::new(&format!("{e:#}")))?;
        self.skipped = merged.skipped;
        let mut out = Vec::new();
        merged
            .doc
            .save_to(&mut out)
            .map_err(|e| JsError::new(&format!("could not write merged PDF ({e})")))?;
        Ok(out)
    }

    /// Names of inputs left out by the last `finish` because they had no pages.
    pub fn skipped(&self) -> Vec<String> {
        self.skipped.clone()
    }
}
