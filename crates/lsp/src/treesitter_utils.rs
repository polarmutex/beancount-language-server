use tree_sitter_beancount::tree_sitter;

pub fn lsp_textdocchange_to_ts_inputedit(
    source: &ropey::Rope,
    change: &lsp_types::TextDocumentContentChangeEvent,
) -> anyhow::Result<tree_sitter::InputEdit> {
    let text = change.text.as_str();
    let text_bytes = text.as_bytes();
    let text_end_byte_idx = text_bytes.len();

    let range = if let Some(range) = change.range {
        range
    } else {
        // Full document replacement: range covers the entire OLD document
        let start = byte_to_lsp_position(source, 0);
        let end = byte_to_lsp_position(source, source.len_bytes());
        lsp_types::Range { start, end }
    };

    let start = lsp_position_to_core(source, range.start)?;
    let old_end = lsp_position_to_core(source, range.end)?;

    let new_end_byte = start.byte as usize + text_end_byte_idx;

    let new_end_position = {
        if new_end_byte >= source.len_bytes() {
            let line_idx = text.lines().count();
            let line_byte_idx = ropey::str_utils::line_to_byte_idx(text, line_idx);
            let row = u32::try_from(source.len_lines() + line_idx)? as usize;
            let column = u32::try_from(text_end_byte_idx - line_byte_idx)? as usize;
            Ok(tree_sitter::Point::new(row, column))
        } else {
            byte_to_tree_sitter_point(source, new_end_byte)
        }
    }?;

    Ok(tree_sitter::InputEdit {
        start_byte: start.byte as usize,
        old_end_byte: old_end.byte as usize,
        new_end_byte: u32::try_from(new_end_byte)? as usize,
        start_position: start.point,
        old_end_position: old_end.point,
        new_end_position,
    })
}

fn byte_to_lsp_position(text: &ropey::Rope, byte_idx: usize) -> lsp_types::Position {
    let line_idx = text.byte_to_line(byte_idx);

    let line_utf16_cu_idx = {
        let char_idx = text.line_to_char(line_idx);
        text.char_to_utf16_cu(char_idx)
    };

    let character_utf16_cu_idx = {
        let char_idx = text.byte_to_char(byte_idx);
        text.char_to_utf16_cu(char_idx)
    };

    let line = line_idx;
    let character = character_utf16_cu_idx - line_utf16_cu_idx;

    lsp_types::Position::new(line as u32, character as u32)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextPosition {
    pub char: u32,
    pub byte: u32,
    pub code: u32,
    pub point: tree_sitter::Point,
}

fn lsp_position_to_core(
    source: &ropey::Rope,
    position: lsp_types::Position,
) -> anyhow::Result<TextPosition> {
    let row_idx = position.line as usize;

    let col_code_idx = position.character as usize;

    let row_char_idx = source.line_to_char(row_idx);
    let col_char_idx = source.utf16_cu_to_char(col_code_idx);

    let row_byte_idx = source.line_to_byte(row_idx);
    let col_byte_idx = source.char_to_byte(col_char_idx);

    let row_code_idx = source.char_to_utf16_cu(row_char_idx);

    let point = {
        let row = position.line as usize;
        let col = u32::try_from(col_byte_idx)? as usize;
        tree_sitter::Point::new(row, col)
    };

    Ok(TextPosition {
        char: u32::try_from(row_char_idx + col_char_idx)?,
        byte: u32::try_from(row_byte_idx + col_byte_idx)?,
        code: u32::try_from(row_code_idx + col_code_idx)?,
        point,
    })
}

fn byte_to_tree_sitter_point(
    source: &ropey::Rope,
    byte_idx: usize,
) -> anyhow::Result<tree_sitter::Point> {
    let line_idx = source.byte_to_line(byte_idx);
    let line_byte_idx = source.line_to_byte(line_idx);
    let row = u32::try_from(line_idx)? as usize;
    let column = u32::try_from(byte_idx - line_byte_idx)? as usize;
    Ok(tree_sitter::Point::new(row, column))
}

pub fn text_for_tree_sitter_node(
    source: &ropey::Rope,
    node: &tree_sitter::Node,
) -> std::string::String {
    let start = source.byte_to_char(node.start_byte());
    let end = source.byte_to_char(node.end_byte());
    let slice = source.slice(start..end);
    slice.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range, TextDocumentContentChangeEvent};
    use ropey::Rope;
    use tree_sitter::Point;

    #[test]
    fn test_lsp_textdocchange_simple_insertion() {
        let source = Rope::from("Hello World");
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 5),
                end: Position::new(0, 5),
            }),
            range_length: None,
            text: " Beautiful".to_string(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 5);
        assert_eq!(edit.old_end_byte, 5);
        assert_eq!(edit.new_end_byte, 15); // Added 10 bytes
        assert_eq!(edit.start_position, Point::new(0, 5));
        assert_eq!(edit.old_end_position, Point::new(0, 5));
    }

    #[test]
    fn test_lsp_textdocchange_simple_deletion() {
        let source = Rope::from("Hello World");
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 0),
                end: Position::new(0, 6),
            }),
            range_length: None,
            text: String::new(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 0);
        assert_eq!(edit.old_end_byte, 6);
        assert_eq!(edit.new_end_byte, 0);
        assert_eq!(edit.start_position, Point::new(0, 0));
        assert_eq!(edit.old_end_position, Point::new(0, 6));
        assert_eq!(edit.new_end_position, Point::new(0, 0));
    }

    #[test]
    fn test_lsp_textdocchange_replacement() {
        let source = Rope::from("Hello World");
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 6),
                end: Position::new(0, 11),
            }),
            range_length: None,
            text: "Rust".to_string(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 6);
        assert_eq!(edit.old_end_byte, 11);
        assert_eq!(edit.new_end_byte, 10); // "Rust" is 4 bytes
        assert_eq!(edit.start_position, Point::new(0, 6));
    }

    #[test]
    fn test_lsp_textdocchange_full_document_replacement() {
        let source = Rope::from("Old content");
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "New content".to_string(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 0);
        assert_eq!(edit.old_end_byte, 11);
        assert_eq!(edit.new_end_byte, 11);
        assert_eq!(edit.start_position, Point::new(0, 0));
    }

    #[test]
    fn test_lsp_textdocchange_multiline_insertion() {
        let source = Rope::from("Line 1\nLine 2");
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(1, 0),
                end: Position::new(1, 0),
            }),
            range_length: None,
            text: "New line\n".to_string(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 7); // After "Line 1\n"
        assert_eq!(edit.old_end_byte, 7);
        assert_eq!(edit.new_end_byte, 16); // Added "New line\n" (9 bytes)
    }

    #[test]
    fn test_lsp_textdocchange_with_multibyte_utf8() {
        let source = Rope::from("Hello 世界");
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 6),
                end: Position::new(0, 8),
            }),
            range_length: None,
            text: "🌍".to_string(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 6);
        // 世界 is 6 bytes (3 bytes each), but we're replacing it with 🌍 (4 bytes)
        assert_eq!(edit.old_end_byte, 12);
        assert_eq!(edit.new_end_byte, 10); // 6 + 4
    }

    #[test]
    fn test_lsp_textdocchange_empty_document() {
        let source = Rope::from("");
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 0),
                end: Position::new(0, 0),
            }),
            range_length: None,
            text: "New content".to_string(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 0);
        assert_eq!(edit.old_end_byte, 0);
        assert_eq!(edit.new_end_byte, 11);
        assert_eq!(edit.start_position, Point::new(0, 0));
        assert_eq!(edit.old_end_position, Point::new(0, 0));
    }

    #[test]
    fn test_lsp_textdocchange_to_empty() {
        let source = Rope::from("Content to delete");
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: String::new(),
        };

        let edit = lsp_textdocchange_to_ts_inputedit(&source, &change).unwrap();
        assert_eq!(edit.start_byte, 0);
        assert_eq!(edit.old_end_byte, 17);
        assert_eq!(edit.new_end_byte, 0);
    }

    #[test]
    fn test_byte_to_lsp_position_simple() {
        let text = Rope::from("Hello\nWorld");
        let pos = byte_to_lsp_position(&text, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        let pos = byte_to_lsp_position(&text, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_lsp_position_to_core_simple() {
        let source = Rope::from("Hello\nWorld");
        let pos = Position::new(0, 5);
        let core_pos = lsp_position_to_core(&source, pos).unwrap();
        assert_eq!(core_pos.byte, 5);
        assert_eq!(core_pos.point, Point::new(0, 5));
    }

    #[test]
    fn test_lsp_position_to_core_second_line() {
        let source = Rope::from("Hello\nWorld");
        let pos = Position::new(1, 3);
        let core_pos = lsp_position_to_core(&source, pos).unwrap();
        assert_eq!(core_pos.byte, 9); // "Hello\n" is 6 bytes + 3
        assert_eq!(core_pos.point, Point::new(1, 3)); // Point column is byte offset from line start
    }

    #[test]
    fn test_byte_to_tree_sitter_point_simple() {
        let source = Rope::from("Hello\nWorld\nTest");
        let point = byte_to_tree_sitter_point(&source, 6).unwrap();
        assert_eq!(point, Point::new(1, 0));

        let point = byte_to_tree_sitter_point(&source, 12).unwrap();
        assert_eq!(point, Point::new(2, 0));
    }

    #[test]
    fn test_text_for_tree_sitter_node() {
        let source = Rope::from("2024-01-01 open Assets:Checking");

        // We need to parse the source to get a tree-sitter node
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(source.to_string(), None).unwrap();
        let root = tree.root_node();

        // Get the text for the entire tree
        let text = text_for_tree_sitter_node(&source, &root);
        assert_eq!(text, "2024-01-01 open Assets:Checking");
    }

    #[test]
    fn test_text_for_tree_sitter_node_with_utf8() {
        let source = Rope::from("2024-01-01 * \"Coffee ☕\"");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse(source.to_string(), None).unwrap();
        let root = tree.root_node();

        let text = text_for_tree_sitter_node(&source, &root);
        assert_eq!(text, "2024-01-01 * \"Coffee ☕\"");
    }
}
