// USED FORM https://github.com/silvanshade/lsp-text
use crate::core::{TextEdit, TextPosition};
use bytes::Bytes;
use ropey::{iter::Chunks, Rope};
use std::{convert::TryFrom, sync::Arc};
use tower_lsp::lsp_types;

use std::borrow::Cow;

trait ChunkExt<'a> {
    fn next_str(&mut self) -> &'a str;
    fn prev_str(&mut self) -> &'a str;
}

impl<'a> ChunkExt<'a> for Chunks<'a> {
    fn next_str(&mut self) -> &'a str {
        self.next().unwrap_or("")
    }

    fn prev_str(&mut self) -> &'a str {
        self.prev().unwrap_or("")
    }
}

pub struct ChunkWalker {
    rope: Arc<Rope>,
    cursor: usize,
    cursor_chunk: &'static str,
    chunks: Chunks<'static>,
}

impl ChunkWalker {
    #[inline]
    fn prev_chunk(&mut self) {
        self.cursor -= self.cursor_chunk.len();
        self.cursor_chunk = self.chunks.prev_str();
        while 0 < self.cursor && self.cursor_chunk.is_empty() {
            self.cursor_chunk = self.chunks.prev_str();
        }
    }

    #[inline]
    fn next_chunk(&mut self) {
        self.cursor += self.cursor_chunk.len();
        self.cursor_chunk = self.chunks.next_str();
        while self.cursor < self.rope.len_bytes() && self.cursor_chunk.is_empty() {
            self.cursor_chunk = self.chunks.next_str();
        }
    }

    #[inline]
    pub fn callback_adapter(mut self) -> impl FnMut(usize, Option<usize>) -> Bytes {
        move |start_index, _end_index| {
            let start_index = start_index as usize;

            while start_index < self.cursor && 0 < self.cursor {
                self.prev_chunk();
            }

            while start_index >= self.cursor + self.cursor_chunk.len() && start_index < self.rope.len_bytes() {
                self.next_chunk();
            }

            let bytes = self.cursor_chunk.as_bytes();
            let bytes = &bytes[start_index - self.cursor..];
            Bytes::from_static(bytes)
        }
    }

    #[inline]
    pub fn callback_adapter_for_tree_sitter(self) -> impl FnMut(usize, tree_sitter::Point) -> Bytes {
        let mut adapter = self.callback_adapter();
        move |start_index, _position| adapter(start_index, None)
    }
}

pub trait RopeExt {
    fn apply_edit(&mut self, edit: &TextEdit);
    fn build_edit<'a>(&self, change: &'a lsp_types::TextDocumentContentChangeEvent) -> anyhow::Result<TextEdit<'a>>;
    fn byte_to_lsp_position(&self, offset: usize) -> lsp_types::Position;
    fn byte_to_tree_sitter_point(&self, offset: usize) -> anyhow::Result<tree_sitter::Point>;
    fn chunk_walker(self, byte_idx: usize) -> ChunkWalker;
    fn lsp_position_to_core(&self, position: lsp_types::Position) -> anyhow::Result<TextPosition>;
    fn lsp_position_to_utf16_cu(&self, position: lsp_types::Position) -> anyhow::Result<u32>;
    fn lsp_range_to_tree_sitter_range(&self, range: lsp_types::Range) -> anyhow::Result<tree_sitter::Range>;
    fn tree_sitter_range_to_lsp_range(&self, range: tree_sitter::Range) -> lsp_types::Range;
    fn utf8_text_for_tree_sitter_node<'rope, 'tree>(&'rope self, node: &tree_sitter::Node<'tree>) -> Cow<'rope, str>;
}

impl RopeExt for Rope {
    fn apply_edit(&mut self, edit: &TextEdit) {
        self.remove(edit.start_char_idx..edit.end_char_idx);
        if !edit.text.is_empty() {
            self.insert(edit.start_char_idx, edit.text);
        }
    }

    fn build_edit<'a>(&self, change: &'a lsp_types::TextDocumentContentChangeEvent) -> anyhow::Result<TextEdit<'a>> {
        let text = change.text.as_str();
        let text_bytes = text.as_bytes();
        let text_end_byte_idx = text_bytes.len();

        let range = if let Some(range) = change.range {
            range
        } else {
            let start = self.byte_to_lsp_position(0);
            let end = self.byte_to_lsp_position(text_end_byte_idx);
            lsp_types::Range { start, end }
        };

        let start = self.lsp_position_to_core(range.start)?;
        let old_end = self.lsp_position_to_core(range.end)?;

        let new_end_byte = start.byte as usize + text_end_byte_idx;

        let new_end_position = {
            if new_end_byte >= self.len_bytes() {
                let line_idx = text.lines().count();
                let line_byte_idx = ropey::str_utils::line_to_byte_idx(text, line_idx);
                let row = self.len_lines() + line_idx;
                let column = text_end_byte_idx - line_byte_idx;
                Ok(tree_sitter::Point::new(row, column))
            } else {
                self.byte_to_tree_sitter_point(new_end_byte)
            }
        }?;

        let input_edit = {
            let start_byte = start.byte;
            let old_end_byte = old_end.byte;
            let new_end_byte = new_end_byte;
            let start_position = start.point;
            let old_end_position = old_end.point;
            tree_sitter::InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position,
                old_end_position,
                new_end_position,
            }
        };

        Ok(TextEdit {
            input_edit,
            start_char_idx: start.char as usize,
            end_char_idx: old_end.char as usize,
            text,
        })
    }

    fn byte_to_lsp_position(&self, byte_idx: usize) -> lsp_types::Position {
        let line_idx = self.byte_to_line(byte_idx);

        let line_utf16_cu_idx = {
            let char_idx = self.line_to_char(line_idx);
            self.char_to_utf16_cu(char_idx)
        };

        let character_utf16_cu_idx = {
            let char_idx = self.byte_to_char(byte_idx);
            self.char_to_utf16_cu(char_idx)
        };

        let line = line_idx;
        let character = character_utf16_cu_idx - line_utf16_cu_idx;

        lsp_types::Position::new(line as u32, character as u32)
    }

    fn byte_to_tree_sitter_point(&self, byte_idx: usize) -> anyhow::Result<tree_sitter::Point> {
        let line_idx = self.byte_to_line(byte_idx);
        let line_byte_idx = self.line_to_byte(line_idx);
        let row = line_idx;
        let column = byte_idx - line_byte_idx;
        Ok(tree_sitter::Point::new(row, column))
    }

    fn chunk_walker(self, byte_idx: usize) -> ChunkWalker {
        let rope = Arc::new(self);
        // NOTE: safe because `rope` is owned by the resulting `ChunkWalker` and won't be dropped early
        #[allow(unsafe_code)]
        let (mut chunks, chunk_byte_idx, ..) = unsafe { (&*Arc::as_ptr(&rope)).chunks_at_byte(byte_idx) };
        let cursor = chunk_byte_idx;
        let cursor_chunk = chunks.next_str();
        ChunkWalker {
            rope,
            cursor,
            cursor_chunk,
            chunks,
        }
    }

    fn lsp_position_to_core(&self, position: lsp_types::Position) -> anyhow::Result<TextPosition> {
        let row_idx = position.line as usize;

        let col_code_idx = position.character as usize;

        let row_char_idx = self.line_to_char(row_idx);
        let col_char_idx = self.utf16_cu_to_char(col_code_idx);

        let row_byte_idx = self.line_to_byte(row_idx);
        let col_byte_idx = self.char_to_byte(col_char_idx);

        let row_code_idx = self.char_to_utf16_cu(row_char_idx);

        let point = {
            let row = position.line as usize;
            let col = col_byte_idx;
            tree_sitter::Point::new(row, col)
        };

        Ok(TextPosition {
            char: row_char_idx + col_char_idx,
            byte: row_byte_idx + col_byte_idx,
            code: row_code_idx + col_code_idx,
            point,
        })
    }

    fn lsp_position_to_utf16_cu(&self, position: lsp_types::Position) -> anyhow::Result<u32> {
        let line_idx = position.line as usize;
        let line_utf16_cu_idx = {
            let char_idx = self.line_to_char(line_idx);
            self.char_to_utf16_cu(char_idx)
        };
        let char_utf16_cu_idx = position.character as usize;
        let result = u32::try_from(line_utf16_cu_idx + char_utf16_cu_idx)?;
        Ok(result)
    }

    fn lsp_range_to_tree_sitter_range(&self, range: lsp_types::Range) -> anyhow::Result<tree_sitter::Range> {
        let start = self.lsp_position_to_core(range.start)?;
        let end = self.lsp_position_to_core(range.end)?;
        let range = tree_sitter::Range {
            start_byte: start.byte,
            end_byte: end.byte,
            start_point: start.point,
            end_point: end.point,
        };
        Ok(range)
    }

    fn tree_sitter_range_to_lsp_range(&self, range: tree_sitter::Range) -> lsp_types::Range {
        let start = self.byte_to_lsp_position(range.start_byte as usize);
        let end = self.byte_to_lsp_position(range.end_byte as usize);
        lsp_types::Range::new(start, end)
    }

    fn utf8_text_for_tree_sitter_node<'rope, 'tree>(&'rope self, node: &tree_sitter::Node<'tree>) -> Cow<'rope, str> {
        let start = self.byte_to_char(node.start_byte() as usize);
        let end = self.byte_to_char(node.end_byte() as usize);
        let slice = self.slice(start..end);
        slice.into()
    }
}
