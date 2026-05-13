use crate::query_cache;
use anyhow::Result;
use tree_sitter::StreamingIterator;
use tree_sitter_beancount::tree_sitter;

/// Represents a formateable line extracted from a Beancount file
/// Contains the components that bean-format uses for alignment
#[derive(Debug, Clone)]
pub(super) struct FormatableLine {
    /// Line number in the document
    pub(super) line_num: usize,
    /// Prefix text (account name or directive start)
    pub(super) prefix: String,
    /// Number text (amount value)
    pub(super) number: String,
    /// Rest of the line after the number (currency, comments, etc.)
    pub(super) rest: String,
}

/// Configuration for formatting calculations
#[derive(Debug)]
pub(super) struct FormatConfig {
    /// Final prefix width to use (may be overridden by config)
    pub(super) final_prefix_width: usize,
    /// Final number width to use (may be overridden by config)
    pub(super) final_num_width: usize,
}

// Adapter to convert rope chunks to bytes
struct ChunksBytes<'a> {
    chunks: ropey::iter::Chunks<'a>,
}
impl<'a> Iterator for ChunksBytes<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.next().map(str::as_bytes)
    }
}

struct RopeProvider<'a>(ropey::RopeSlice<'a>);
impl<'a> tree_sitter::TextProvider<&'a [u8]> for RopeProvider<'a> {
    type I = ChunksBytes<'a>;

    fn text(&mut self, node: tree_sitter::Node) -> Self::I {
        let start_char = self.0.byte_to_char(node.start_byte());
        let end_char = self.0.byte_to_char(node.end_byte());
        let fragment = self.0.slice(start_char..end_char);
        ChunksBytes {
            chunks: fragment.chunks(),
        }
    }
}

/// Extracts formateable lines from the document using tree-sitter
/// This mimics bean-format's regex-based line extraction
pub(super) fn extract_formateable_lines(
    doc: &crate::document::Document,
    tree: &tree_sitter::Tree,
) -> Result<Vec<FormatableLine>> {
    let query = query_cache::format_query();

    let mut query_cursor = tree_sitter::QueryCursor::new();
    let rope_slice = doc
        .content
        .get_slice(..)
        .ok_or_else(|| anyhow::anyhow!("Failed to get rope slice for document"))?;
    let mut matches = query_cursor.matches(query, tree.root_node(), RopeProvider(rope_slice));

    // open directives with multiple currencies (e.g. `open Assets:Foo CURR1 CURR2`) produce
    // one query match per currency node.  Keep only the first match per line so that the first
    // currency becomes @number and the rest trail it — duplicate edits corrupt the file.
    let mut seen_lines = std::collections::HashSet::new();
    let mut formateable_lines = Vec::new();

    while let Some(matched) = matches.next() {
        let mut prefix_node: Option<tree_sitter::Node> = None;
        let mut number_node: Option<tree_sitter::Node> = None;

        // Extract prefix and number nodes from captures
        for capture in matched.captures {
            let capture_name = query.capture_names()[capture.index as usize];
            match capture_name {
                "prefix" => prefix_node = Some(capture.node),
                "number" => number_node = Some(capture.node),
                _ => {}
            }
        }

        if let (Some(prefix), Some(number)) = (prefix_node, number_node) {
            let line_num = prefix.start_position().row;
            if seen_lines.insert(line_num)
                && let Some(line) = extract_line_components(doc, prefix, number)
            {
                formateable_lines.push(line);
            }
        }
    }

    Ok(formateable_lines)
}

/// Extracts the components (prefix, number, rest) from a single line
fn extract_line_components(
    doc: &crate::document::Document,
    prefix_node: tree_sitter::Node,
    number_node: tree_sitter::Node,
) -> Option<FormatableLine> {
    let line_num = prefix_node.start_position().row;

    // Get the line end boundary in the rope (character-based)
    let line_end_char = if line_num + 1 < doc.content.len_lines() {
        doc.content.line_to_char(line_num + 1)
    } else {
        doc.content.len_chars()
    };

    // IMPORTANT: tree-sitter node positions use BYTE offsets, not character offsets
    // We need to convert byte offsets to character offsets for UTF-8 safety

    // Extract prefix (from line start to end of account/directive)
    // The prefix should include everything from the beginning of the line to the end of the prefix node
    let line_start_byte = doc.content.char_to_byte(doc.content.line_to_char(line_num));
    let prefix_end_byte = prefix_node.end_byte().min(doc.content.len_bytes());
    let prefix_start_char = doc.content.byte_to_char(line_start_byte);
    let prefix_end_char = doc
        .content
        .byte_to_char(prefix_end_byte)
        .min(doc.content.len_chars());
    let prefix_text = doc
        .content
        .slice(prefix_start_char..prefix_end_char)
        .to_string();

    // Extract number text
    let number_start_byte = number_node.start_byte().min(doc.content.len_bytes());
    let number_end_byte = number_node.end_byte().min(doc.content.len_bytes());
    let number_start_char = doc
        .content
        .byte_to_char(number_start_byte)
        .min(doc.content.len_chars());
    let number_end_char = doc
        .content
        .byte_to_char(number_end_byte)
        .min(doc.content.len_chars());
    let number_text = doc
        .content
        .slice(number_start_char..number_end_char)
        .to_string();

    // Extract rest (everything after the number)
    // Use the rope directly to handle UTF-8 correctly - rope uses character indices
    let rest_text = if number_end_char < line_end_char {
        doc.content
            .slice(number_end_char..line_end_char)
            .to_string()
    } else {
        String::new()
    };

    Some(FormatableLine {
        line_num,
        prefix: prefix_text,
        number: number_text,
        rest: rest_text,
    })
}

/// Calculates formatting configuration including maximum widths and overrides
pub(super) fn calculate_format_config(
    formateable_lines: &[FormatableLine],
    user_config: &crate::config::FormattingConfig,
) -> FormatConfig {
    // Calculate maximum widths across all lines (bean-format behavior)
    let max_prefix_width = formateable_lines
        .iter()
        .map(|line| line.prefix.trim_end().len())
        .max()
        .unwrap_or(0);

    let max_number_width = formateable_lines
        .iter()
        .map(|line| line.number.len())
        .max()
        .unwrap_or(0);

    // Use configuration overrides if provided (like bean-format's -w and -W options)
    let final_prefix_width = user_config.prefix_width.unwrap_or(max_prefix_width);
    let final_num_width = user_config.num_width.unwrap_or(max_number_width);

    FormatConfig {
        final_prefix_width,
        final_num_width,
    }
}
