# Claude Code Assistant - Beancount Language Server

This is a **beancount language server** implementation written in Rust that provides LSP (Language Server Protocol) support for Beancount files (.bean, .beancount). Beancount is a double-entry bookkeeping system that uses plain text files.

## Project Structure

This is a Rust workspace with the following structure:

- **`crates/lsp/`** - Main Rust language server implementation
  - **`src/checkers/`** - Bean-check validation implementations (Strategy pattern)
    - **`mod.rs`** - Strategy trait and factory pattern
    - **`system_call.rs`** - Traditional bean-check binary execution
    - **`python.rs`** - Python script integration via subprocess
    - **`pyo3_embedded.rs`** - PyO3 embedded Python integration
    - **`types.rs`** - Shared data structures
- **`vscode/`** - VSCode extension (TypeScript)
- **`python/`** - Python utilities for beancount integration
  - **`bean_check.py`** - Enhanced Python validation script with JSON output
- **Root workspace** - Cargo workspace configuration

## Key Files

- **`Cargo.toml`** - Workspace configuration with cargo-dist setup
- **`crates/lsp/Cargo.toml`** - Main LSP server package
- **`vscode/package.json`** - VSCode extension configuration
- **`flake.nix`** - Nix development environment with Crane build system
- **`cliff.toml`** - git-cliff configuration for changelog generation

## Development Commands

### Rust Language Server

```bash
# Build the language server
cargo build

# Build with PyO3 embedded Python support (experimental)
cargo build --features python-embedded

# Run tests with coverage
cargo llvm-cov --all-features --locked --workspace --lcov --output-path lcov.info -- --include-ignored

# Run tests with PyO3 feature
cargo test --features python-embedded

# Format code
cargo fmt

# Lint with clippy
cargo clippy --all-targets --all-features

# Install locally
cargo install --path crates/lsp/

# Run the language server
cargo run --bin beancount-language-server
```

### VSCode Extension (in vscode/ directory)

```bash
# Install dependencies
npm install

# Build extension
npm run build

# Watch for changes
npm run watch

# Lint and format
npm run lint
npm run fix

# Run tests
npm run test

# Package extension
npm run package
```

### Nix Development

```bash
# Enter development shell
nix develop

# Build with nix
nix build

# Run checks (format, clippy, tests, audit)
nix flake check
```

## Architecture

### Language Server Features

- **Diagnostics** - Multi-method validation system with pluggable checkers
  - **System Call** - Traditional bean-check binary execution (default)
  - **Python Script** - Enhanced Python script with JSON output (experimental)
  - **PyO3 Embedded** - Direct Python library integration (experimental)
- **Formatting** - Generates edits similar to bean-format
- **Completions** - Shows completions for Payees, Accounts, Dates
- **Future planned**: definitions, folding, hover, rename

### Bean-check Integration Architecture

The diagnostics system uses a Strategy pattern with three implementations:

1. **SystemCallChecker** - Executes bean-check binary via subprocess and parses stderr
2. **PythonChecker** - Runs Python script that outputs structured JSON
3. **PyO3EmbeddedChecker** - Calls beancount library directly via embedded Python

Factory pattern in `checkers/mod.rs` handles creation based on configuration.

### Key Dependencies

- **tree-sitter-beancount** - Parsing via tree-sitter
- **lsp-server** / **lsp-types** - LSP protocol implementation
- **ropey** - Efficient text rope data structure
- **tracing** - Structured logging
- **anyhow** / **thiserror** - Error handling
- **regex** - Pattern matching for error parsing
- **chrono** - Date/time handling
- **serde** / **serde_json** - JSON serialization for Python integration
- **pyo3** - Python integration (optional, feature-gated)

## Configuration

Language server accepts configuration via LSP initialization:
- **journal_file** - Path to main beancount journal file
- **bean_check.method** - Validation method: "system" (default), "python-script", or "python-embedded"
- **bean_check.bean_check_cmd** - Path to bean-check binary (for system method)
- **bean_check.python_cmd** - Path to Python executable (for Python methods)
- **bean_check.python_script** - Path to Python validation script (for python-script method)

## Testing

Tests use:
- **insta** for snapshot testing
- **test-log** for test logging
- **env_logger** for development logging

Run tests with: `cargo test`

## CI/CD

GitHub Actions workflows:
- **ci.yml** - Main CI (format, clippy, tests on multiple OS/Rust versions)
- **release.yml** - Automated releases
- **pr-lints.yml** - PR-specific checks
- **codeql-analysis.yml** - Security analysis

## Editor Integration

Supports multiple editors:
- **Neovim** - Via nvim-lspconfig
- **VSCode** - Via included extension
- **Helix** - Via languages.toml configuration
- **Vim/Emacs** - Planned support

## Development Environment

- Uses **Nix flakes** for reproducible development environment
- **Rust stable** toolchain with clippy, rustfmt, rust-analyzer
- **Crane** for efficient Nix-based Rust builds
- **cargo-dist** for cross-platform release builds

## Release Process

- Uses **cargo-dist** for building releases
- Targets: Linux (x86_64, aarch64, loongarch64), macOS (x86_64, aarch64), Windows (x86_64)
- **git-cliff** for changelog generation
- Automated via GitHub Actions

## Common Tasks

- **Add new LSP feature**: Modify `crates/lsp/src/handlers.rs` and related provider files
- **Update completions**: Modify `crates/lsp/src/providers/completion.rs`
- **Add new checker method**: Implement `BeancountChecker` trait in `crates/lsp/src/checkers/`
- **Modify bean-check integration**: Update checker implementations or factory in `crates/lsp/src/checkers/`
- **Enhance Python validation**: Modify `python/bean_check.py` script
- **Update VSCode extension**: Modify files in `vscode/src/`

## External Dependencies

- **beancount** Python package (for diagnostics)
- **tree-sitter** grammar for parsing
- Standard Rust toolchain