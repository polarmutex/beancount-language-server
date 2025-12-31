use crate::server::LspServerStateSnapshot;
use crate::utils::ToFilePath;
use anyhow::Result;
use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
    SemanticTokensParams, SemanticTokensResult,
};
use ropey::Rope;
use std::cmp::Ordering;
use std::convert::TryFrom;
use std::path::PathBuf;
use strum::IntoEnumIterator;
use tree_sitter_beancount::NodeKind;
use tree_sitter_beancount::tree_sitter::{self, Node};

#[repr(u8)]
#[derive(
    strum_macros::EnumIter, strum_macros::EnumCount, Copy, Clone, Debug, PartialEq, Eq, Hash,
)]
#[allow(dead_code)] // Some kinds may be mapped in the legend before being emitted.
enum TokenKind {
    Keyword,
    Comment,
    String,
    Number,
    Type,
    Macro,
    Operator,
    Parameter,
    Property,
    Class,
}

fn token_types() -> Vec<SemanticTokenType> {
    TokenKind::iter().map(token_type).collect()
}

fn token_index(kind: TokenKind) -> u32 {
    kind as u32
}

fn token_type(kind: TokenKind) -> SemanticTokenType {
    match kind {
        TokenKind::Keyword => SemanticTokenType::KEYWORD,
        TokenKind::Comment => SemanticTokenType::COMMENT,
        TokenKind::String => SemanticTokenType::STRING,
        TokenKind::Number => SemanticTokenType::NUMBER,
        TokenKind::Type => SemanticTokenType::TYPE,
        TokenKind::Macro => SemanticTokenType::MACRO,
        TokenKind::Operator => SemanticTokenType::OPERATOR,
        TokenKind::Parameter => SemanticTokenType::PARAMETER,
        TokenKind::Property => SemanticTokenType::PROPERTY,
        TokenKind::Class => SemanticTokenType::CLASS,
    }
}

// We currently do not expose token modifiers; keep list empty for a minimal implementation.
const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[];

#[derive(Debug)]
struct RawToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers_bitset: u32,
}

/// Public legend shared between capability advertisement and token responses.
pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: token_types(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}

/// Handle `textDocument/semanticTokens/full`.
pub(crate) fn semantic_tokens_full(
    snapshot: LspServerStateSnapshot,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokensResult>> {
    let uri_path: PathBuf = match params.text_document.uri.to_file_path() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };

    let tree: &tree_sitter::Tree = match snapshot.forest.get(&uri_path) {
        Some(tree) => tree,
        None => return Ok(None),
    };

    let document = match snapshot.open_docs.get(&uri_path) {
        Some(doc) => doc.clone(),
        None => return Ok(None),
    };
    let content: Rope = document.content;

    let mut raw_tokens = Vec::new();
    collect_tokens(tree.root_node(), &content, &mut raw_tokens);

    if raw_tokens.is_empty() {
        return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: vec![],
        })));
    }

    raw_tokens.sort_by(|a, b| match a.line.cmp(&b.line) {
        Ordering::Equal => a.start.cmp(&b.start),
        other => other,
    });

    let mut data = Vec::with_capacity(raw_tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for token in raw_tokens {
        let delta_line = token.line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            token.start.saturating_sub(prev_start)
        } else {
            token.start
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.modifiers_bitset,
        });

        prev_line = token.line;
        prev_start = token.start;
    }

    Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    })))
}

fn collect_tokens(node: Node, content: &Rope, out: &mut Vec<RawToken>) {
    if let Some(kind) = classify_node(node.kind().into())
        && let Some(tok) = to_semantic_token(&node, content, kind)
    {
        out.push(tok);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens(child, content, out);
    }
}

fn classify_node(kind: NodeKind) -> Option<TokenKind> {
    match kind {
        NodeKind::Account => Option::None,

        NodeKind::Asterisk => Option::Some(TokenKind::Operator),
        NodeKind::At => Option::Some(TokenKind::Operator),
        NodeKind::Atat => Option::Some(TokenKind::Operator),
        NodeKind::Plus => Option::Some(TokenKind::Operator),
        NodeKind::Minus => Option::Some(TokenKind::Operator),
        NodeKind::Slash => Option::Some(TokenKind::Operator),
        NodeKind::Flag => Option::Some(TokenKind::Keyword),
        NodeKind::Bool => Option::Some(TokenKind::Keyword),

        NodeKind::Comment => Option::Some(TokenKind::Comment),

        NodeKind::Currency => Option::Some(TokenKind::Class),
        NodeKind::Date => Option::Some(TokenKind::Number),
        NodeKind::Number => Option::Some(TokenKind::Number),

        NodeKind::Item => Option::Some(TokenKind::Keyword),
        NodeKind::Key => Option::Some(TokenKind::Keyword),

        NodeKind::Link => Option::Some(TokenKind::Parameter),
        NodeKind::Tag => Option::Some(TokenKind::Parameter),

        NodeKind::Narration => Option::Some(TokenKind::String),
        NodeKind::Payee => Option::Some(TokenKind::String),
        NodeKind::String => Option::Some(TokenKind::String),

        NodeKind::Unknown => Option::None,
    }
}

fn to_semantic_token(node: &Node, content: &Rope, kind: TokenKind) -> Option<RawToken> {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();

    let line = u32::try_from(content.byte_to_line(start_byte)).ok()?;
    let start_char_idx = content.byte_to_char(start_byte);
    let end_char_idx = content.byte_to_char(end_byte);

    let line_start_char_idx = content.line_to_char(line as usize);
    let line_start_utf16 = content.char_to_utf16_cu(line_start_char_idx);

    let start_utf16 = content.char_to_utf16_cu(start_char_idx);
    let end_utf16 = content.char_to_utf16_cu(end_char_idx);

    let column_utf16 = start_utf16.checked_sub(line_start_utf16)?;
    let length_utf16 = end_utf16.checked_sub(start_utf16)?;
    if length_utf16 == 0 {
        return None;
    }

    Some(RawToken {
        line,
        start: u32::try_from(column_utf16).ok()?,
        length: u32::try_from(length_utf16).ok()?,
        token_type: token_index(kind),
        modifiers_bitset: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn token_kind_map_is_ordered_and_unique() {
        use std::collections::HashSet;

        let kinds: Vec<TokenKind> = TokenKind::iter().collect();
        let types: Vec<SemanticTokenType> = token_types();

        assert_eq!(
            kinds.len(),
            types.len(),
            "TokenKind variants must match token_types length"
        );

        let legend_len = types.len() as u32;
        let mut seen = HashSet::new();

        for (expected_idx, kind) in kinds.iter().copied().enumerate() {
            let idx = token_index(kind);
            assert!(idx < legend_len, "token_index produced out-of-range value");
            assert_eq!(
                idx as usize, expected_idx,
                "token_index mapping must be ordered to match token_types"
            );
            assert_eq!(
                token_type(kind),
                types[expected_idx],
                "token_type must follow token_types ordering"
            );
            let inserted = seen.insert(idx);
            assert!(inserted, "token_index values are not unique");
        }
    }

    #[test]
    fn token_types_align_with_token_index() {
        let types = token_types();

        for (idx, kind) in TokenKind::iter().enumerate() {
            assert_eq!(
                idx,
                token_index(kind) as usize,
                "token_index must equal declaration order"
            );
            assert_eq!(
                types[idx],
                token_type(kind),
                "token_type must equal legend entry built from token_types()"
            );
        }
    }
}
