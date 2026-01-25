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
    Function,
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
        TokenKind::Function => SemanticTokenType::FUNCTION,
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
    collect_tokens(&tree.root_node(), &content, &mut raw_tokens);

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

fn collect_tokens(node: &Node, content: &Rope, out: &mut Vec<RawToken>) {
    let child = match NodeKind::from(node.kind()) {
        NodeKind::Include
        | NodeKind::Pushtag
        | NodeKind::Poptag
        | NodeKind::Pushmeta
        | NodeKind::Popmeta
        | NodeKind::Plugin
        | NodeKind::Option => Some((0, TokenKind::Function)),

        NodeKind::Open
        | NodeKind::Pad
        | NodeKind::Note
        | NodeKind::Balance
        | NodeKind::Transaction
        | NodeKind::Custom => Some((1, TokenKind::Function)),

        _ => None,
    };

    if let Some((index, kind)) = child
        && let Some(child) = node.child(index)
        && let Some(token) = to_semantic_token(&child, content, kind)
    {
        out.push(token);
    }

    if let Some(kind) = classify_node(node.kind().into())
        && let Some(tok) = to_semantic_token(node, content, kind)
    {
        out.push(tok);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens(&child, content, out);
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

        // All other NodeKind variants are not classified for semantic highlighting
        _ => Option::None,
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

    #[test]
    fn test_legend() {
        let legend = legend();
        assert_eq!(legend.token_types.len(), TokenKind::iter().count());
        assert_eq!(legend.token_modifiers.len(), 0);
    }

    #[test]
    fn test_classify_node_operators() {
        assert_eq!(classify_node(NodeKind::Asterisk), Some(TokenKind::Operator));
        assert_eq!(classify_node(NodeKind::At), Some(TokenKind::Operator));
        assert_eq!(classify_node(NodeKind::Atat), Some(TokenKind::Operator));
        assert_eq!(classify_node(NodeKind::Plus), Some(TokenKind::Operator));
        assert_eq!(classify_node(NodeKind::Minus), Some(TokenKind::Operator));
        assert_eq!(classify_node(NodeKind::Slash), Some(TokenKind::Operator));
    }

    #[test]
    fn test_classify_node_keywords() {
        assert_eq!(classify_node(NodeKind::Flag), Some(TokenKind::Keyword));
        assert_eq!(classify_node(NodeKind::Bool), Some(TokenKind::Keyword));
        assert_eq!(classify_node(NodeKind::Item), Some(TokenKind::Keyword));
        assert_eq!(classify_node(NodeKind::Key), Some(TokenKind::Keyword));
    }

    #[test]
    fn test_classify_node_strings() {
        assert_eq!(classify_node(NodeKind::Narration), Some(TokenKind::String));
        assert_eq!(classify_node(NodeKind::Payee), Some(TokenKind::String));
        assert_eq!(classify_node(NodeKind::String), Some(TokenKind::String));
    }

    #[test]
    fn test_classify_node_numbers() {
        assert_eq!(classify_node(NodeKind::Date), Some(TokenKind::Number));
        assert_eq!(classify_node(NodeKind::Number), Some(TokenKind::Number));
    }

    #[test]
    fn test_classify_node_other_types() {
        assert_eq!(classify_node(NodeKind::Comment), Some(TokenKind::Comment));
        assert_eq!(classify_node(NodeKind::Currency), Some(TokenKind::Class));
        assert_eq!(classify_node(NodeKind::Link), Some(TokenKind::Parameter));
        assert_eq!(classify_node(NodeKind::Tag), Some(TokenKind::Parameter));
    }

    #[test]
    fn test_classify_node_none_cases() {
        // Test node kinds that explicitly return None
        assert_eq!(classify_node(NodeKind::Account), None);
        assert_eq!(classify_node(NodeKind::Unknown), None);
        // Test other node kinds that fall through to the _ => None case
        assert_eq!(classify_node(NodeKind::Directive), None);
        assert_eq!(classify_node(NodeKind::Posting), None);
        assert_eq!(classify_node(NodeKind::Transaction), None);
    }

    #[test]
    fn test_to_semantic_token_basic() {
        // Create a simple beancount content
        let content = ropey::Rope::from_str("2024-01-01 open Assets:Checking");

        // Parse with tree-sitter
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser
            .parse("2024-01-01 open Assets:Checking", None)
            .unwrap();

        // Find the date node
        let root = tree.root_node();
        let mut cursor = root.walk();
        let open_directive = root.children(&mut cursor).next().unwrap();
        let mut open_cursor = open_directive.walk();
        let date_node = open_directive
            .children(&mut open_cursor)
            .find(|n| n.kind() == "date")
            .unwrap();

        // Convert to semantic token
        let token = to_semantic_token(&date_node, &content, TokenKind::Number).unwrap();

        assert_eq!(token.line, 0);
        assert_eq!(token.start, 0); // Date starts at column 0
        assert_eq!(token.length, 10); // "2024-01-01" is 10 characters
        assert_eq!(token.token_type, token_index(TokenKind::Number));
        assert_eq!(token.modifiers_bitset, 0);
    }

    #[test]
    fn test_to_semantic_token_with_utf8() {
        // Test with multi-byte UTF-8 content
        let content = ropey::Rope::from_str("2024-01-01 * \"Café ☕\"");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser.parse("2024-01-01 * \"Café ☕\"", None).unwrap();

        // Find the narration node
        let root = tree.root_node();
        let mut cursor = root.walk();
        let txn = root.children(&mut cursor).next().unwrap();
        let mut txn_cursor = txn.walk();
        let narration_node = txn
            .children(&mut txn_cursor)
            .find(|n| n.kind() == "narration")
            .unwrap();

        // Convert to semantic token
        let token = to_semantic_token(&narration_node, &content, TokenKind::String).unwrap();

        assert_eq!(token.line, 0);
        // UTF-16 length should account for multi-byte characters
        assert!(token.length > 0);
        assert_eq!(token.token_type, token_index(TokenKind::String));
    }

    #[test]
    fn test_collect_tokens_simple() {
        let content = ropey::Rope::from_str("2024-01-01 open Assets:Checking");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser
            .parse("2024-01-01 open Assets:Checking", None)
            .unwrap();

        let mut tokens = Vec::new();
        collect_tokens(&tree.root_node(), &content, &mut tokens);

        // Should collect at least the date token
        assert!(!tokens.is_empty());
        // Verify we collected a date token (should be first)
        assert_eq!(tokens[0].token_type, token_index(TokenKind::Function));
    }

    #[test]
    fn test_collect_tokens_transaction() {
        let content = ropey::Rope::from_str(
            "2024-01-01 * \"Payee\" \"Narration\"\n  Assets:Cash  100.00 USD",
        );

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser
            .parse(
                "2024-01-01 * \"Payee\" \"Narration\"\n  Assets:Cash  100.00 USD",
                None,
            )
            .unwrap();

        let mut tokens = Vec::new();
        collect_tokens(&tree.root_node(), &content, &mut tokens);

        // Should collect multiple tokens: date, payee, narration, numbers, currency
        assert!(tokens.len() >= 4, "Should collect at least 4 tokens");

        // Verify we have different token types
        let mut has_number = false;
        let mut has_string = false;
        let mut has_class = false;

        for token in &tokens {
            if token.token_type == token_index(TokenKind::Number) {
                has_number = true;
            }
            if token.token_type == token_index(TokenKind::String) {
                has_string = true;
            }
            if token.token_type == token_index(TokenKind::Class) {
                has_class = true;
            }
        }

        assert!(has_number, "Should have number token (date or amount)");
        assert!(has_string, "Should have string token (payee/narration)");
        assert!(has_class, "Should have class token (currency)");
    }

    #[test]
    fn test_collect_tokens_with_comments() {
        let content = ropey::Rope::from_str("; Comment\n2024-01-01 open Assets:Checking");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .unwrap();
        let tree = parser
            .parse("; Comment\n2024-01-01 open Assets:Checking", None)
            .unwrap();

        let mut tokens = Vec::new();
        collect_tokens(&tree.root_node(), &content, &mut tokens);

        // Should have both comment and date tokens
        let has_comment = tokens
            .iter()
            .any(|t| t.token_type == token_index(TokenKind::Comment));
        let has_date = tokens
            .iter()
            .any(|t| t.token_type == token_index(TokenKind::Number));

        assert!(has_comment, "Should have comment token");
        assert!(has_date, "Should have date token");
    }
}
