use async_lsp::lsp_types;
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
        let start = byte_to_lsp_position(source, 0);
        let end = byte_to_lsp_position(source, text_end_byte_idx);
        lsp_types::Range { start, end }
    };

    let start = lsp_position_to_core(source, range.start)?;
    let old_end = lsp_position_to_core(source, range.end)?;

    let new_end_byte = start.byte as usize + text_end_byte_idx;

    let new_end_position = {
        if new_end_byte >= source.len_bytes() {
            let line_idx = text.lines().count();
            let line_byte_idx = ropey::str_utils::line_to_byte_idx(text, line_idx);
            let row = u32::try_from(source.len_lines() + line_idx).unwrap() as usize;
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
    let row = u32::try_from(line_idx).unwrap() as usize;
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
