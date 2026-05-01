use crate::beancount_data::BeancountData;
use crate::document::Document;
use crate::treesitter_utils::lsp_textdocchange_to_ts_inputedit;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tree_sitter_beancount::tree_sitter;

/// Arc-wrapped views of the three public maps, used to construct `LspServerStateSnapshot`.
///
/// Each field is an `Arc<HashMap<…>>` so that taking a snapshot is a cheap pointer
/// clone; the underlying HashMap is only copied (via [`Arc::make_mut`]) when the
/// `DocumentStore` actually modifies it.
pub(crate) struct DocumentStoreMaps {
    pub open_docs: Arc<HashMap<PathBuf, Document>>,
    pub forest: Arc<HashMap<PathBuf, Arc<tree_sitter::Tree>>>,
    pub beancount_data: Arc<HashMap<PathBuf, Arc<BeancountData>>>,
}

/// Owns all four document-related maps and enforces their consistency invariant:
/// - Every open document has a parser (private, hidden from callers).
/// - When a tree is invalidated, its `beancount_data` is removed atomically.
/// - `beancount_data` is extracted lazily via `ensure_beancount_data`.
///
/// The three public maps are stored as `Arc<HashMap<…>>` so that
/// [`snapshot_maps`][DocumentStore::snapshot_maps] is an O(1) pointer clone.
/// [`Arc::make_mut`] is used before every mutation to ensure copy-on-write
/// semantics: if a snapshot is currently live the HashMap is cloned once, then
/// mutated; otherwise the existing allocation is reused.
pub(crate) struct DocumentStore {
    open_docs: Arc<HashMap<PathBuf, Document>>,
    /// Stateful parsers for incremental re-parsing. Private: callers never need a parser directly.
    parsers: HashMap<PathBuf, tree_sitter::Parser>,
    forest: Arc<HashMap<PathBuf, Arc<tree_sitter::Tree>>>,
    beancount_data: Arc<HashMap<PathBuf, Arc<BeancountData>>>,
}

impl DocumentStore {
    pub(crate) fn new() -> Self {
        Self {
            open_docs: Arc::new(HashMap::new()),
            parsers: HashMap::new(),
            forest: Arc::new(HashMap::new()),
            beancount_data: Arc::new(HashMap::new()),
        }
    }

    /// Open a document: insert the doc buffer, initialise (or reuse) a parser, do a
    /// fresh parse, and eagerly extract `BeancountData`.
    ///
    /// Always parses fresh — the file may have been modified externally between
    /// close and reopen, so cached trees cannot be trusted.
    pub(crate) fn open(&mut self, uri: PathBuf, text: &str, version: i32) {
        let content = ropey::Rope::from_str(text);
        Arc::make_mut(&mut self.open_docs)
            .insert(uri.clone(), Document { content, version });

        self.parsers.entry(uri.clone()).or_insert_with(|| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_beancount::language())
                .expect("Failed to set language for tree-sitter parser");
            parser
        });
        let parser = self
            .parsers
            .get_mut(&uri)
            .expect("parser should exist after insertion");

        let tree = Arc::new(
            parser
                .parse(text, None)
                .expect("Failed to parse document"),
        );

        let doc_content = &self
            .open_docs
            .get(&uri)
            .expect("doc should exist after insertion")
            .content;
        let beancount_data = BeancountData::new(&tree, doc_content);

        Arc::make_mut(&mut self.forest).insert(uri.clone(), tree);
        Arc::make_mut(&mut self.beancount_data).insert(uri, Arc::new(beancount_data));
    }

    /// Apply incremental content changes to an open document.
    ///
    /// Updates the rope, does an incremental tree-sitter re-parse, and lazily
    /// invalidates `beancount_data` (removed so it is re-extracted on next demand).
    pub(crate) fn apply_change(
        &mut self,
        uri: &PathBuf,
        changes: &[lsp_types::TextDocumentContentChangeEvent],
        new_version: i32,
    ) -> Result<()> {
        // Step 1 — calculate tree-sitter edit positions before mutating the rope.
        let ts_edits = {
            let doc = match self.open_docs.get(uri) {
                Some(d) => d,
                None => {
                    tracing::warn!("Document not found in open_docs: {:?}", uri);
                    return Ok(());
                }
            };

            let current_version = doc.version;
            if new_version <= current_version {
                tracing::warn!(
                    "Received out-of-order or duplicate change: current version={}, received version={}",
                    current_version,
                    new_version
                );
            }
            tracing::trace!("Document version: {} -> {}", current_version, new_version);

            changes
                .iter()
                .map(|change| lsp_textdocchange_to_ts_inputedit(&doc.content, change))
                .collect::<Result<Vec<_>, _>>()?
            // doc borrow released
        };

        // Step 2 — apply rope edits and update the version.
        {
            let doc = Arc::make_mut(&mut self.open_docs)
                .get_mut(uri)
                .expect("doc should exist after prior check");

            for change in changes {
                let (text, range) = match change {
                    lsp_types::TextDocumentContentChangeEvent::TextDocumentContentChangePartial(c) => {
                        (c.text.as_str(), c.range)
                    }
                    lsp_types::TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(c) => {
                        let end_line = (doc.content.len_lines().saturating_sub(1)) as u32;
                        let end_line_len = if doc.content.len_lines() > 0 {
                            let last_line = doc.content.line(end_line as usize);
                            last_line.len_chars().saturating_sub(1) as u32
                        } else {
                            0
                        };
                        let r = lsp_types::Range {
                            start: lsp_types::Position::new(0, 0),
                            end: lsp_types::Position::new(end_line, end_line_len),
                        };
                        (c.text.as_str(), r)
                    }
                };

                let start_row_char_idx = doc.content.line_to_char(range.start.line as usize);
                let end_row_char_idx = doc.content.line_to_char(range.end.line as usize);

                let start_line_utf16_cu = doc.content.char_to_utf16_cu(start_row_char_idx);
                let start_utf16_idx =
                    (start_line_utf16_cu + range.start.character as usize)
                        .min(doc.content.len_utf16_cu());
                let start_col_char_idx =
                    doc.content.utf16_cu_to_char(start_utf16_idx) - start_row_char_idx;

                let end_line_utf16_cu = doc.content.char_to_utf16_cu(end_row_char_idx);
                let end_utf16_idx = (end_line_utf16_cu + range.end.character as usize)
                    .min(doc.content.len_utf16_cu());
                let end_col_char_idx =
                    doc.content.utf16_cu_to_char(end_utf16_idx) - end_row_char_idx;

                let start_char_idx = start_row_char_idx + start_col_char_idx;
                let end_char_idx = end_row_char_idx + end_col_char_idx;

                tracing::trace!(
                    "Applying change: range={}:{}-{}:{}, char_idx={}-{}, text_len={}",
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                    start_char_idx,
                    end_char_idx,
                    text.len()
                );

                doc.content.remove(start_char_idx..end_char_idx);
                if !text.is_empty() {
                    doc.content.insert(start_char_idx, text);
                }
            }

            doc.version = new_version;
            // doc borrow released
        }

        // Step 3 — clone the old tree (applying ts_edits) and snapshot the text.
        // Both borrows are released before step 4 mutates `parsers`.
        let (edited_old_tree, text_str) = {
            let old_tree_arc = match self.forest.get(uri) {
                Some(t) => t,
                None => {
                    tracing::warn!("Tree not found in forest: {:?}", uri);
                    return Ok(());
                }
            };
            let mut old_tree = (**old_tree_arc).clone();
            for edit in &ts_edits {
                old_tree.edit(edit);
            }
            let text_str = self
                .open_docs
                .get(uri)
                .expect("doc should exist")
                .text_string();
            (old_tree, text_str)
            // forest and open_docs borrows released
        };

        // Step 4 — incremental parse (only mutates `parsers`).
        let new_tree = {
            let parser = match self.parsers.get_mut(uri) {
                Some(p) => p,
                None => {
                    tracing::warn!("Parser not found for document: {:?}", uri);
                    return Ok(());
                }
            };
            parser.parse(&text_str, Some(&edited_old_tree))
        };

        // Step 5 — commit new tree, lazily invalidate beancount_data.
        if let Some(tree) = new_tree {
            *Arc::make_mut(&mut self.forest)
                .get_mut(uri)
                .expect("tree should exist in forest") = Arc::new(tree);
            Arc::make_mut(&mut self.beancount_data).remove(uri);
        }

        Ok(())
    }

    /// Close a document: remove the buffer, tree, and data. Keep the parser for reuse.
    ///
    /// Trees and data are cleared so that a reopen gets a fresh parse, correctly
    /// handling external modifications made while the file was closed.
    pub(crate) fn close(&mut self, uri: &PathBuf) {
        Arc::make_mut(&mut self.open_docs).remove(uri);
        Arc::make_mut(&mut self.forest).remove(uri);
        Arc::make_mut(&mut self.beancount_data).remove(uri);
        // parsers intentionally kept for potential reuse
    }

    /// Insert a freshly parsed external file (includes, watched-file reloads).
    ///
    /// Wraps the tree in `Arc`, creates `BeancountData`, and stores both.
    /// Does not touch `open_docs` or `parsers`.
    pub(crate) fn insert_parsed(&mut self, uri: PathBuf, tree: tree_sitter::Tree, content: &str) {
        let tree_arc = Arc::new(tree);
        let rope = ropey::Rope::from_str(content);
        let beancount_data = BeancountData::new(&tree_arc, &rope);
        Arc::make_mut(&mut self.forest).insert(uri.clone(), tree_arc);
        Arc::make_mut(&mut self.beancount_data).insert(uri, Arc::new(beancount_data));
    }

    /// Insert pre-computed `Arc`-wrapped tree and data (used by the ForestInit background task).
    pub(crate) fn insert_tree_and_data(
        &mut self,
        uri: PathBuf,
        tree: Arc<tree_sitter::Tree>,
        data: Arc<BeancountData>,
    ) {
        Arc::make_mut(&mut self.forest).insert(uri.clone(), tree);
        Arc::make_mut(&mut self.beancount_data).insert(uri, data);
    }

    /// Remove all caches for an externally deleted file.
    pub(crate) fn remove_external(&mut self, uri: &PathBuf) {
        Arc::make_mut(&mut self.forest).remove(uri);
        Arc::make_mut(&mut self.beancount_data).remove(uri);
        self.parsers.remove(uri);
    }

    /// Clear stale caches for an externally changed file before re-parsing.
    pub(crate) fn invalidate_external(&mut self, uri: &PathBuf) {
        Arc::make_mut(&mut self.forest).remove(uri);
        Arc::make_mut(&mut self.beancount_data).remove(uri);
        // open_docs and parsers untouched — file is not open in the editor
    }

    /// Lazily extract `BeancountData` for `uri` if it is absent.
    ///
    /// Called before requests that need semantic data (completion, hover, …).
    /// `beancount_data` is absent after every `apply_change` to avoid blocking
    /// per-keystroke parsing; it is (re-)created here on the first read after
    /// each edit.
    pub(crate) fn ensure_beancount_data(&mut self, uri: &PathBuf) {
        if self.beancount_data.contains_key(uri) {
            return;
        }
        if let (Some(tree), Some(doc)) = (self.forest.get(uri), self.open_docs.get(uri)) {
            let beancount_data = BeancountData::new(tree, &doc.content);
            Arc::make_mut(&mut self.beancount_data)
                .insert(uri.clone(), Arc::new(beancount_data));
            tracing::debug!("Lazy extraction: BeancountData extracted for {:?}", uri);
        }
    }

    // ── Readers ──────────────────────────────────────────────────────────────

    pub(crate) fn get_tree(&self, uri: &PathBuf) -> Option<&Arc<tree_sitter::Tree>> {
        self.forest.get(uri)
    }

    pub(crate) fn has_open_doc(&self, uri: &PathBuf) -> bool {
        self.open_docs.contains_key(uri)
    }

    pub(crate) fn open_doc_keys(&self) -> impl Iterator<Item = &PathBuf> {
        self.open_docs.keys()
    }

    pub(crate) fn forest_keys(&self) -> impl Iterator<Item = &PathBuf> {
        self.forest.keys()
    }

    // ── Snapshot ─────────────────────────────────────────────────────────────

    /// Clone the three public map Arcs for constructing `LspServerStateSnapshot`.
    ///
    /// This is an O(1) operation: only the Arc reference counts are incremented.
    /// The underlying HashMaps are not copied unless the store subsequently
    /// mutates them (copy-on-write via [`Arc::make_mut`]).
    pub(crate) fn snapshot_maps(&self) -> DocumentStoreMaps {
        DocumentStoreMaps {
            open_docs: Arc::clone(&self.open_docs),
            forest: Arc::clone(&self.forest),
            beancount_data: Arc::clone(&self.beancount_data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tree_sitter_beancount::tree_sitter::Parser;

    fn make_parser() -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .expect("Failed to set language");
        parser
    }

    fn parse(content: &str) -> tree_sitter::Tree {
        make_parser().parse(content, None).expect("Failed to parse")
    }

    const CONTENT: &str = "2024-01-01 open Assets:Checking USD\n";

    #[test]
    fn test_open_populates_all_maps() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");

        store.open(uri.clone(), CONTENT, 1);

        assert!(store.open_docs.contains_key(&uri));
        assert!(store.get_tree(&uri).is_some());
        assert!(store.beancount_data.contains_key(&uri));
        assert_eq!(store.open_docs.get(&uri).unwrap().version, 1);
    }

    #[test]
    fn test_apply_change_invalidates_beancount_data() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        #[allow(deprecated)]
        let change = lsp_types::TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
            lsp_types::TextDocumentContentChangePartial {
                range: lsp_types::Range {
                    start: lsp_types::Position::new(0, 0),
                    end: lsp_types::Position::new(0, 0),
                },
                range_length: None,
                text: "".to_string(),
            });
        store.apply_change(&uri, &[change], 2).unwrap();

        // beancount_data should be invalidated after change
        assert!(!store.beancount_data.contains_key(&uri));
        // but tree and doc should still be present
        assert!(store.get_tree(&uri).is_some());
        assert!(store.open_docs.get(&uri).is_some());
    }

    #[test]
    fn test_apply_change_updates_content_and_version() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), "hello", 1);

        #[allow(deprecated)]
        let change = lsp_types::TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
            lsp_types::TextDocumentContentChangePartial {
                range: lsp_types::Range {
                    start: lsp_types::Position::new(0, 0),
                    end: lsp_types::Position::new(0, 5),
                },
                range_length: None,
                text: "world".to_string(),
            });
        store.apply_change(&uri, &[change], 2).unwrap();

        let doc = store.open_docs.get(&uri).unwrap();
        assert_eq!(doc.text_string(), "world");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn test_close_removes_doc_tree_data_but_keeps_parser() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        store.close(&uri);

        assert!(store.open_docs.get(&uri).is_none());
        assert!(store.get_tree(&uri).is_none());
        assert!(!store.beancount_data.contains_key(&uri));
        // parser retained for reuse
        assert!(store.parsers.contains_key(&uri));
    }

    #[test]
    fn test_reopen_after_close_gets_fresh_state() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);
        store.close(&uri);

        let new_content = "2024-06-01 open Liabilities:CreditCard USD\n";
        store.open(uri.clone(), new_content, 2);

        let doc = store.open_docs.get(&uri).unwrap();
        assert_eq!(doc.version, 2);
        assert!(doc.text_string().contains("Liabilities"));
    }

    #[test]
    fn test_ensure_beancount_data_lazy_extraction() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        // Simulate post-edit state: data absent, tree present
        #[allow(deprecated)]
        let change = lsp_types::TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
            lsp_types::TextDocumentContentChangePartial {
                range: lsp_types::Range {
                    start: lsp_types::Position::new(0, 0),
                    end: lsp_types::Position::new(0, 0),
                },
                range_length: None,
                text: "".to_string(),
            });
        store.apply_change(&uri, &[change], 2).unwrap();
        assert!(!store.beancount_data.contains_key(&uri));

        store.ensure_beancount_data(&uri);
        assert!(store.beancount_data.contains_key(&uri));
    }

    #[test]
    fn test_ensure_beancount_data_skips_if_present() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        let first_ptr = Arc::as_ptr(store.beancount_data.get(&uri).unwrap());
        store.ensure_beancount_data(&uri);
        let second_ptr = Arc::as_ptr(store.beancount_data.get(&uri).unwrap());

        assert_eq!(first_ptr, second_ptr, "should not re-extract if data present");
    }

    #[test]
    fn test_ensure_beancount_data_does_nothing_without_tree() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        // doc exists but no tree
        Arc::make_mut(&mut store.open_docs).insert(
            uri.clone(),
            Document {
                content: ropey::Rope::from_str(CONTENT),
                version: 1,
            },
        );

        store.ensure_beancount_data(&uri); // must not panic
        assert!(!store.beancount_data.contains_key(&uri));
    }

    #[test]
    fn test_insert_parsed_stores_tree_and_data() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/included.beancount");
        let tree = parse(CONTENT);

        store.insert_parsed(uri.clone(), tree, CONTENT);

        assert!(store.get_tree(&uri).is_some());
        assert!(store.beancount_data.contains_key(&uri));
        // not an open doc
        assert!(store.open_docs.get(&uri).is_none());
    }

    #[test]
    fn test_insert_tree_and_data() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/bg.beancount");
        let tree = Arc::new(parse(CONTENT));
        let rope = ropey::Rope::from_str(CONTENT);
        let data = Arc::new(BeancountData::new(&tree, &rope));

        store.insert_tree_and_data(uri.clone(), tree, data);

        assert!(store.get_tree(&uri).is_some());
        assert!(store.beancount_data.contains_key(&uri));
    }

    #[test]
    fn test_remove_external_clears_all_caches() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/ext.beancount");
        let tree = parse(CONTENT);
        store.insert_parsed(uri.clone(), tree, CONTENT);

        store.remove_external(&uri);

        assert!(store.get_tree(&uri).is_none());
        assert!(!store.beancount_data.contains_key(&uri));
    }

    #[test]
    fn test_invalidate_external_clears_tree_and_data() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/ext.beancount");
        let tree = parse(CONTENT);
        store.insert_parsed(uri.clone(), tree, CONTENT);

        store.invalidate_external(&uri);

        assert!(store.get_tree(&uri).is_none());
        assert!(!store.beancount_data.contains_key(&uri));
    }

    #[test]
    fn test_snapshot_maps_clones_three_maps() {
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        let maps = store.snapshot_maps();

        assert!(maps.open_docs.contains_key(&uri));
        assert!(maps.forest.contains_key(&uri));
        assert!(maps.beancount_data.contains_key(&uri));
        // parsers NOT in snapshot
    }

    #[test]
    fn test_snapshot_maps_shares_arc_identity() {
        // Snapshot should share the same Arc allocation (pointer equality),
        // not clone the underlying HashMaps.
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        let maps1 = store.snapshot_maps();
        let maps2 = store.snapshot_maps();

        assert!(
            Arc::ptr_eq(&maps1.forest, &maps2.forest),
            "consecutive snapshots should share forest Arc"
        );
        assert!(
            Arc::ptr_eq(&maps1.beancount_data, &maps2.beancount_data),
            "consecutive snapshots should share beancount_data Arc"
        );
        assert!(
            Arc::ptr_eq(&maps1.open_docs, &maps2.open_docs),
            "consecutive snapshots should share open_docs Arc"
        );
    }

    #[test]
    fn test_mutation_after_snapshot_does_not_alias() {
        // After a mutation, the live snapshot must not reflect the change
        // (copy-on-write: make_mut allocates a new HashMap).
        let mut store = DocumentStore::new();
        let uri = PathBuf::from("/test/file.beancount");
        store.open(uri.clone(), CONTENT, 1);

        let snapshot_before = store.snapshot_maps();

        // Mutate by inserting another key
        let uri2 = PathBuf::from("/test/file2.beancount");
        store.open(uri2.clone(), CONTENT, 1);

        // snapshot_before should still point to the old allocation
        assert!(
            !Arc::ptr_eq(&snapshot_before.forest, &store.snapshot_maps().forest),
            "snapshot taken before mutation should not alias the new forest"
        );
        // The old snapshot should not contain the new key
        assert!(
            !snapshot_before.forest.contains_key(&uri2),
            "old snapshot must not see keys added after snapshot"
        );
    }
}
