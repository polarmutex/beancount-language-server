use super::extraction::{FormatConfig, FormatableLine};
use anyhow::Result;

/// Generates text edits for currency column mode (bean-format -c option)
pub(super) fn generate_currency_column_edits(
    formateable_lines: &[FormatableLine],
    currency_col: usize,
    doc: &crate::document::Document,
    indent_width: Option<usize>,
) -> Vec<lsp_types::TextEdit> {
    let mut text_edits = Vec::new();

    for line in formateable_lines {
        // Apply custom indentation if specified, but only for postings, not top-level directives
        let (indent_str, account_name) = if let Some(target_indent) = indent_width {
            let account_part = line.prefix.trim_start().trim_end();

            // Check if this is a top-level directive that shouldn't be indented
            let line_start_char = doc.content.line_to_char(line.line_num);
            let line_end_char = if line.line_num + 1 < doc.content.len_lines() {
                doc.content.line_to_char(line.line_num + 1)
            } else {
                doc.content.len_chars()
            };
            let full_line = doc
                .content
                .slice(line_start_char..line_end_char)
                .to_string();

            let line_content = full_line.trim();
            let is_top_level_directive = line_content.contains("balance ")
                || line_content.contains("price ")
                || (line_content.starts_with("20")
                    && (line_content.contains(" balance ") || line_content.contains(" price ")));

            if is_top_level_directive {
                ("".to_string(), account_part)
            } else {
                (" ".repeat(target_indent), account_part)
            }
        } else {
            // Preserve original indentation
            let account_part = line.prefix.trim_end();
            let original_indent = if line.prefix.len() > account_part.len() {
                &line.prefix[..(line.prefix.len() - account_part.len())]
            } else {
                ""
            };
            (original_indent.to_string(), account_part)
        };

        // Calculate spacing needed to align currency at the specified column
        // Bean-format logic: num_of_spaces = currency_column - len(prefix) - len(number) - 3
        let prefix_len = indent_str.len() + account_name.len();
        let number_len = line.number.len();
        let spaces_needed = if currency_col >= prefix_len + number_len + 3 {
            currency_col - prefix_len - number_len - 3
        } else {
            2 // minimum spacing
        };

        // Create the formatted line: indent + account + spaces + "  " + number + " " + rest
        let target_line = format!(
            "{}{}{}  {} {}",
            indent_str,
            account_name,
            " ".repeat(spaces_needed),
            line.number,
            line.rest.trim_start()
        );

        if let Some(edit) = create_line_replacement_edit(line.line_num, &target_line, doc) {
            text_edits.push(edit);
        }
    }

    text_edits
}

/// Generates text edits for template mode (bean-format default behavior)
pub(super) fn generate_template_edits(
    formateable_lines: &[FormatableLine],
    config: &FormatConfig,
    number_currency_spacing: usize,
    indent_width: Option<usize>,
    doc: &crate::document::Document,
) -> Vec<lsp_types::TextEdit> {
    let mut text_edits = Vec::new();

    for line in formateable_lines {
        // Create formatted line using bean-format's template logic with custom number-currency spacing
        // Extract currency part from rest and apply custom spacing
        let rest_content = line.rest.trim_start();
        let formatted_rest = if rest_content.starts_with('"') {
            // Quoted string (e.g. booking method like "FIFO") — preserve with single space.
            format!(" {rest_content}")
        } else if rest_content.starts_with(|c: char| c.is_alphabetic()) {
            // Currency starts immediately — apply configured number-currency spacing.
            format!("{}{rest_content}", " ".repeat(number_currency_spacing))
        } else {
            // Comma-separated currencies (`, EUR`), inline comments (`; note`), or empty —
            // preserve the original bytes so punctuation is not silently stripped.
            line.rest.to_string()
        };

        // Apply custom indentation if specified, but only for postings, not top-level directives
        let (indent_str, account_name) = if let Some(target_indent) = indent_width {
            let account_part = line.prefix.trim_start().trim_end();

            // Check if this is a top-level directive (like balance) that shouldn't be indented
            // Get the full line to check for directive keywords
            let line_start_char = doc.content.line_to_char(line.line_num);
            let line_end_char = if line.line_num + 1 < doc.content.len_lines() {
                doc.content.line_to_char(line.line_num + 1)
            } else {
                doc.content.len_chars()
            };
            let full_line = doc
                .content
                .slice(line_start_char..line_end_char)
                .to_string();

            // More comprehensive check for balance/price directives
            let line_content = full_line.trim();
            let is_top_level_directive = line_content.contains("balance ")
                || line_content.contains("price ")
                || (line_content.starts_with("20")
                    && (line_content.contains(" balance ") || line_content.contains(" price ")));

            if is_top_level_directive {
                // Don't indent top-level directives
                ("".to_string(), account_part)
            } else {
                // Apply custom indentation for postings
                (" ".repeat(target_indent), account_part)
            }
        } else {
            // Preserve original indentation by finding the leading whitespace
            let account_part = line.prefix.trim_end();
            let original_indent = if line.prefix.len() > account_part.len() {
                &line.prefix[..(line.prefix.len() - account_part.len())]
            } else {
                ""
            };
            (original_indent.to_string(), account_part)
        };

        // Template: "{indent}{account_name:<adjusted_width}  {:>num_width}{custom_rest}"
        // Adjust the prefix width to account for the custom indentation
        let adjusted_prefix_width = if config.final_prefix_width > indent_str.len() {
            config.final_prefix_width - indent_str.len()
        } else {
            account_name.len() // fallback to actual account name length
        };

        let formatted_line = format!(
            "{}{:<width$}  {:>num_width$}{}",
            indent_str,
            account_name,
            line.number,
            formatted_rest,
            width = adjusted_prefix_width,
            num_width = config.final_num_width
        );

        if let Some(edit) = create_line_replacement_edit(line.line_num, &formatted_line, doc) {
            text_edits.push(edit);
        }
    }

    text_edits
}

/// Creates a text edit to replace an entire line with new content
fn create_line_replacement_edit(
    line_num: usize,
    new_content: &str,
    doc: &crate::document::Document,
) -> Option<lsp_types::TextEdit> {
    // Calculate the range of the original line (without trailing newline)
    let line_start_char = doc.content.line_to_char(line_num);
    let line_end_char = if line_num + 1 < doc.content.len_lines() {
        doc.content.line_to_char(line_num + 1)
    } else {
        doc.content.len_chars()
    };

    let original_line = doc
        .content
        .slice(line_start_char..line_end_char)
        .to_string();

    // Skip edit if the line content hasn't changed
    // This optimization prevents unnecessary edits for already-formatted lines
    if original_line.trim_end() == new_content.trim_end() {
        return None;
    }

    // LSP Position.character is a UTF-16 code unit offset, not a Unicode scalar count.
    // chars().count() would be wrong for characters outside the BMP (e.g. emoji).
    let original_line_utf16_len = original_line.trim_end().encode_utf16().count();

    let line_start = lsp_types::Position {
        line: line_num as u32,
        character: 0,
    };
    let line_end = lsp_types::Position {
        line: line_num as u32,
        character: original_line_utf16_len as u32,
    };

    Some(lsp_types::TextEdit {
        range: lsp_types::Range {
            start: line_start,
            end: line_end,
        },
        new_text: new_content.trim_end().to_string(),
    })
}

/// Applies indent normalization to lines not already handled by main formatting
/// This ensures that indent changes don't conflict with amount/currency formatting
pub(super) fn apply_indent_normalization_to_remaining_lines(
    doc: &crate::document::Document,
    _tree: &tree_sitter_beancount::tree_sitter::Tree,
    target_indent_width: usize,
    mut existing_edits: Vec<lsp_types::TextEdit>,
) -> Result<Vec<lsp_types::TextEdit>> {
    use std::collections::HashSet;

    let target_indent = " ".repeat(target_indent_width);

    // Collect line numbers that already have edits from main formatting
    let edited_lines: HashSet<u32> = existing_edits
        .iter()
        .map(|edit| edit.range.start.line)
        .collect();

    // Process all lines in the document
    for line_num in 0..doc.content.len_lines() {
        let line_num_u32 = line_num as u32;

        // Skip lines that already have formatting edits
        if edited_lines.contains(&line_num_u32) {
            continue;
        }

        let line_start_char = doc.content.line_to_char(line_num);
        let line_end_char = if line_num + 1 < doc.content.len_lines() {
            doc.content.line_to_char(line_num + 1)
        } else {
            doc.content.len_chars()
        };

        let current_line = doc
            .content
            .slice(line_start_char..line_end_char)
            .to_string();

        // Only process lines that start with whitespace AND are likely to be postings/metadata
        // Don't normalize lines that contain top-level directive keywords at the start
        if current_line.starts_with(char::is_whitespace) {
            let trimmed_line = current_line.trim_start();
            if !trimmed_line.is_empty() {
                // Skip lines that appear to be top-level directives that are just indented
                // Look for common directive keywords after trimming
                let starts_with_directive = trimmed_line.starts_with("balance ")
                    || trimmed_line.starts_with("pad ")
                    || trimmed_line.starts_with("price ")
                    || trimmed_line.starts_with("open ")
                    || trimmed_line.starts_with("close ")
                    || trimmed_line.starts_with("note ")
                    || trimmed_line.starts_with("document ")
                    || trimmed_line.contains(" * \"")
                    || trimmed_line.contains(" ! \"")
                    || trimmed_line.contains(" txn \"");

                if !starts_with_directive {
                    let new_line = format!("{target_indent}{trimmed_line}");

                    // Only create edit if indentation actually changes
                    if current_line.trim_end() != new_line.trim_end()
                        && let Some(edit) = create_line_replacement_edit(line_num, &new_line, doc)
                    {
                        existing_edits.push(edit);
                    }
                }
            }
        }
    }

    Ok(existing_edits)
}
