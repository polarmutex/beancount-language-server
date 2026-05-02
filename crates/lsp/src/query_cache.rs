//! Centralised tree-sitter query cache.
//!
//! Every tree-sitter `Query` used in the server is compiled once on first call
//! and cached in a `OnceLock` static.  Call sites use the named accessor
//! functions here; this module is the single place where query strings live.
//!
//! # Adding a new query
//! 1. Define a `static FOO_QUERY: OnceLock<tree_sitter::Query>`.
//! 2. Add a `pub(crate) fn foo_query()` that calls `get_or_init`.
//! 3. Replace the call-site's inline `Query::new(…)` with `query_cache::foo_query()`.

use std::sync::OnceLock;
use tree_sitter_beancount::tree_sitter;

// ── beancount_data queries ────────────────────────────────────────────────────

static UNIFIED_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();
static CURRENCY_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();
static NOTE_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();
static OPTION_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();

/// Tags, links, flags, open-directive accounts, and transactions in one pass.
pub(crate) fn unified_query() -> &'static tree_sitter::Query {
    UNIFIED_QUERY.get_or_init(|| {
        let q = r#"
            (tag) @tag
            (link) @link
            (flag) @flag
            (open account: (account) @account)
            (transaction) @transaction
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), q)
            .expect("Failed to compile unified query")
    })
}

/// Currencies from open, commodity, and free-standing currency nodes.
pub(crate) fn currency_query() -> &'static tree_sitter::Query {
    CURRENCY_QUERY.get_or_init(|| {
        let q = r#"
            (open (currency) @currency)
            (commodity (currency) @currency)
            (currency) @currency
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), q)
            .expect("Failed to compile currency query")
    })
}

/// Note directives: account + string body.
pub(crate) fn note_query() -> &'static tree_sitter::Query {
    NOTE_QUERY.get_or_init(|| {
        let q = r#"
            (note account: (account) @account (string) @note)
            (note (account) @account (string) @note)
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), q)
            .expect("Failed to compile note query")
    })
}

/// Option directives: key + value strings.
pub(crate) fn option_query() -> &'static tree_sitter::Query {
    OPTION_QUERY.get_or_init(|| {
        let q = r#"
            (option key: (string) @key value: (string) @value)
        "#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), q)
            .expect("Failed to compile option query")
    })
}

// ── references query ──────────────────────────────────────────────────────────

static ACCOUNT_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();

/// All account nodes — used for cross-file reference and rename search.
pub(crate) fn account_query() -> &'static tree_sitter::Query {
    ACCOUNT_QUERY.get_or_init(|| {
        tree_sitter::Query::new(&tree_sitter_beancount::language(), "(account) @account")
            .expect("Failed to compile account query")
    })
}

// ── forest query ──────────────────────────────────────────────────────────────

static INCLUDE_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();

/// Include directives: extracts the string path argument.
pub(crate) fn include_query() -> &'static tree_sitter::Query {
    INCLUDE_QUERY.get_or_init(|| {
        let q = r#"(include (string) @string)"#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), q)
            .expect("Failed to compile include query")
    })
}

// ── formatting query ──────────────────────────────────────────────────────────

static FORMAT_QUERY: OnceLock<tree_sitter::Query> = OnceLock::new();

/// Postings, balances, prices and open directives for column-alignment.
pub(crate) fn format_query() -> &'static tree_sitter::Query {
    FORMAT_QUERY.get_or_init(|| {
        let q = r#"
( posting
    (account) @prefix
    amount: (incomplete_amount
        [
            (number)
            (unary_number_expr)
            (binary_number_expr)
        ] @number
    )?
)
( balance
    (account) @prefix
    (amount_tolerance
        ([
            (number)
            (unary_number_expr)
            (binary_number_expr)
        ] @number)
    )
)
( price
    currency: (_) @prefix
    amount: (amount
        ([
            (number)
            (unary_number_expr)
            (binary_number_expr)
        ] @number)
    )
)
( open
    (account) @prefix
    (currency) @number
)
"#;
        tree_sitter::Query::new(&tree_sitter_beancount::language(), q)
            .expect("Failed to compile format query")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_queries_compile() {
        unified_query();
        currency_query();
        note_query();
        option_query();
        account_query();
        include_query();
        format_query();
    }

    #[test]
    fn queries_are_singletons() {
        assert!(std::ptr::eq(unified_query(), unified_query()));
        assert!(std::ptr::eq(currency_query(), currency_query()));
        assert!(std::ptr::eq(account_query(), account_query()));
        assert!(std::ptr::eq(include_query(), include_query()));
        assert!(std::ptr::eq(format_query(), format_query()));
    }
}
