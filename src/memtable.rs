use crate::query::doc_terms;
use crate::Doc;
use std::collections::HashMap;

#[derive(Default)]
pub struct Memtable {
    docs: Vec<Doc>,
    terms: HashMap<String, Vec<u32>>,
    bytes: usize,
}

impl Memtable {
    pub fn new() -> Memtable {
        Memtable::default()
    }

    pub fn insert(&mut self, d: Doc) {
        let id = self.docs.len() as u32;
        for t in doc_terms(&d) {
            self.terms.entry(t).or_default().push(id);
        }
        self.bytes += d.message.len() + d.level.len() + d.service.len() + 16;
        self.docs.push(d);
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn docs(&self) -> &[Doc] {
        &self.docs
    }

    pub fn doc(&self, id: u32) -> &Doc {
        &self.docs[id as usize]
    }

    pub fn postings(&self, term: &str) -> Option<&[u32]> {
        self.terms.get(term).map(Vec::as_slice)
    }
}
