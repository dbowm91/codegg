use indexmap::IndexMap;
use std::sync::RwLock;

use crate::parser::Document;

#[derive(Default)]
pub struct ParseCache(RwLock<IndexMap<String, Document>>);

impl ParseCache {
    pub fn get(&self, input: &str) -> Option<Document> {
        self.0.read().ok()?.get(input).cloned()
    }

    pub fn insert(&self, input: &str, document: Document) {
        if let Ok(mut rows) = self.0.write() {
            rows.insert(input.to_owned(), document);
        }
    }
}
