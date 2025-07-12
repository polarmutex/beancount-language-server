# Intelligent Completion System

The beancount-language-server features a revolutionary completion system that uses tree-sitter to understand document structure and provide context-aware completions.

## Overview

Traditional LSP completions rely on simple trigger characters and basic pattern matching. Our system analyzes the entire document structure to understand exactly where you are and what you're trying to accomplish.

## How It Works

### 1. Context Analysis
The system uses tree-sitter to parse your beancount document and identify:
- What type of structure you're in (transaction, directive, etc.)
- Where you are within that structure
- What should come next based on beancount syntax

### 2. Intelligent Prediction
Based on context analysis, the system predicts what you want to complete:
- **Document root**: Dates and transaction types
- **Transaction posting**: Account names with fuzzy search
- **Open directive**: Account names, then currencies
- **Amount context**: Currency codes and common amounts

### 3. Focused Results
Instead of overwhelming you with every possible completion, the system provides only the most relevant options for your current context.

## Completion Types

### Account Completions
- **Trigger**: Typing in posting areas or after `:`
- **Features**: 
  - Fuzzy search with nucleo-matcher (6x faster than alternatives)
  - Capitalization-based filtering:
    - `A` → All accounts starting with "A" (Assets:*, Accounts:*)
    - `a` → Fuzzy search across all accounts
    - `Assets` → Exact prefix matching
  - Intelligent ranking based on relevance

### Currency Completions
- **Trigger**: After account names in postings, in open directives
- **Features**: 80+ world currencies (USD, EUR, GBP, JPY, etc.)
- **Filtering**: Prefix-based with case-insensitive matching

### Date Completions
- **Trigger**: At document root, beginning of lines
- **Features**:
  - Today's date
  - Current month
  - Previous/next month
  - Smart formatting (YYYY-MM-DD)

### Transaction Type Completions
- **Trigger**: After dates at document root
- **Options**: `txn`, `balance`, `open`, `close`, `price`, etc.
- **Context**: Only appears where transaction types are valid

### Flag Completions
- **Trigger**: In transaction headers
- **Options**: 
  - `*` (Complete transaction)
  - `!` (Incomplete transaction for debugging)

### Amount Completions
- **Trigger**: In posting contexts after accounts
- **Features**: Common amounts (100.00, 50.00, etc.)
- **Context-aware**: Adapts based on transaction context

### Narration/Payee Completions
- **Trigger**: In quote contexts, after `"`
- **Features**: 
  - Extracted from previous transactions
  - Smart quote handling
  - Fuzzy matching on historical data

### Tag Completions (#)
- **Trigger**: After `#` character
- **Features**: 
  - Extracted from document using tree-sitter queries
  - Alphabetically sorted
  - Duplicate elimination

### Link Completions (^)
- **Trigger**: After `^` character
- **Features**: 
  - Extracted from document using tree-sitter queries
  - Alphabetically sorted
  - Duplicate elimination

## Architecture

### Core Components

#### CompletionContext
Encapsulates all information needed for intelligent completions:
```rust
struct CompletionContext {
    structure_type: StructureType,  // Where are we?
    expected_next: Vec<ExpectedType>, // What's expected?
    prefix: String,                 // What's been typed?
    parent_context: Option<String>, // Parent structure info
}
```

#### StructureType
Identifies the beancount structure:
- `Transaction` - Inside transaction blocks
- `Posting` - Within specific posting lines
- `OpenDirective` - In open statements
- `BalanceDirective` - In balance statements
- `PriceDirective` - In price statements
- `DocumentRoot` - At top level

#### ExpectedType
Defines what completions are relevant:
- `Account` - Account names
- `Currency` - Currency codes
- `Amount` - Monetary amounts
- `Date` - Date strings
- `Flag` - Transaction flags
- `Narration` - Description text
- `Payee` - Payee names
- `Tag` - Tags (#tag)
- `Link` - Links (^link)
- `TransactionKind` - Directive types

### Processing Pipeline

1. **Context Determination** (`determine_completion_context`)
   - Parse cursor position with tree-sitter
   - Find relevant document node
   - Analyze structure and predict expectations

2. **Context Analysis** (`analyze_node_context`)
   - Walk up tree-sitter AST to find meaningful ancestors
   - Match against known beancount structures
   - Extract context-specific information

3. **Completion Dispatch** (`complete_based_on_context`)
   - Route to appropriate completion providers
   - Handle trigger character overrides
   - Focus on single vs. multiple expected types

4. **Provider Functions**
   - `complete_account_with_prefix` - Account completions
   - `complete_currency` - Currency completions
   - `complete_amount` - Amount suggestions
   - `complete_date` - Date completions
   - `complete_flag` - Flag completions
   - And more...

## Performance Optimizations

### Tree-sitter Efficiency
- Uses efficient node queries instead of manual traversal
- Caches parsed trees in language server state
- Minimal overhead for context analysis

### Fuzzy Search Performance
- **nucleo-matcher**: 6x faster than alternatives
- Optimized for Unicode and ASCII text
- Smart prefiltering and ranking

### Result Limiting
- Caps results at 20 items to prevent UI overwhelm
- Prioritizes most relevant matches
- Sorts by relevance score

### Memory Efficiency
- Reuses existing document data
- Minimal allocations during completion
- Efficient string handling

## Configuration

### Capitalization-Based Search Modes
```
A, B, C    → Prefix search (all accounts starting with letter)
a, b, c    → Fuzzy search (intelligent matching)
Mixed case → Exact prefix matching
```

### Completion Limits
- Maximum 20 items per completion request
- Top matches prioritized by score
- Alphabetical secondary sorting

## Examples

### Account Completion in Transaction
```beancount
2025-01-15 * "Grocery shopping"
    Exp|  
        ^ Typing "Exp" here triggers account completion
          Results: Expenses:Food:Groceries, Expenses:Transport:Gas, etc.
```

### Currency Completion in Open
```beancount
2025-01-01 open Assets:Cash:Checking |
                                     ^ Typing here suggests currencies
                                       Results: USD, EUR, GBP, etc.
```

### Date Completion at Root
```beancount
|
^ Typing here at document root provides dates and transaction types
  Results: 2025-07-12, 2025-07-, txn, balance, open, etc.
```

## Testing

The completion system includes comprehensive tests covering:
- Context detection accuracy
- Fuzzy search quality  
- Performance benchmarks
- Edge case handling
- Integration with LSP protocol

## Future Enhancements

### Planned Features
- **Template Expansion**: Complete entire transaction structures
- **Smart Amounts**: Balance-aware amount suggestions
- **Historical Context**: Learn from user patterns
- **Multi-file Support**: Cross-file account references

### Performance Improvements
- **Query Caching**: Cache tree-sitter queries for repeated patterns
- **Incremental Updates**: Update completions on document changes
- **Background Processing**: Pre-compute completion data

## Development Guide

### Adding New Completion Types

1. **Add ExpectedType variant**:
   ```rust
   enum ExpectedType {
       // ... existing variants
       NewType,
   }
   ```

2. **Create provider function**:
   ```rust
   fn complete_new_type(
       data: HashMap<PathBuf, BeancountData>,
       prefix: &str,
   ) -> Result<Option<Vec<lsp_types::CompletionItem>>> {
       // Implementation
   }
   ```

3. **Update dispatch logic**:
   ```rust
   ExpectedType::NewType => {
       complete_new_type(beancount_data, &context.prefix)
   }
   ```

4. **Add context detection**:
   - Update structure analysis functions
   - Add appropriate expected types to contexts

### Debugging Completion Issues

1. **Enable debug logging**:
   ```bash
   RUST_LOG=debug cargo run
   ```

2. **Check context detection**:
   - Look for "Completion context" debug messages
   - Verify structure_type and expected_next

3. **Test provider functions**:
   - Unit test individual completion providers
   - Verify data extraction and filtering

4. **Validate tree-sitter parsing**:
   - Check node types and ranges
   - Ensure proper ancestor traversal

## Conclusion

The intelligent completion system transforms the beancount editing experience by understanding document structure and user intent. It provides fast, relevant, and contextually appropriate suggestions that help users write beancount files more efficiently and accurately.