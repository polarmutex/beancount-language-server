# Beancount Language Server Brownfield Architecture Document

## Introduction

This document captures the CURRENT STATE of the Beancount Language Server codebase, including architecture patterns, technical decisions, and real-world implementation details. It serves as a reference for AI agents working on enhancements and maintenance.

### Document Scope

Comprehensive documentation of the entire beancount-language-server system, including the Rust LSP server, VSCode extension, Python integration components, and development infrastructure.

### Change Log

| Date | Version | Description | Author |
|------|---------|-------------|--------|
| 2025-01-09 | 1.0 | Initial brownfield analysis | Claude |

## Quick Reference - Key Files and Entry Points

### Critical Files for Understanding the System

- **Main Entry**: `crates/lsp/src/main.rs` - CLI argument parsing and logging setup
- **Core LSP Logic**: `crates/lsp/src/lib.rs` - Server initialization and main event loop
- **Configuration**: `crates/lsp/src/config.rs` - Runtime configuration management
- **LSP Handlers**: `crates/lsp/src/handlers.rs` - Request/response handlers for LSP protocol
- **Core Providers**: `crates/lsp/src/providers/` - Feature implementations (completion, diagnostics, formatting)
- **Bean-check Strategy**: `crates/lsp/src/checkers/mod.rs` - Pluggable validation architecture
- **VSCode Extension**: `vscode/src/extension.ts` - Client-side LSP integration

### Key Algorithms and Business Logic

- **Diagnostics Provider**: `crates/lsp/src/providers/diagnostics.rs` - Multi-method beancount validation
- **Completion Engine**: `crates/lsp/src/providers/completion.rs` - Context-aware autocompletion
- **Formatting Logic**: `crates/lsp/src/providers/formatting.rs` - Bean-format compatible formatting
- **Tree-sitter Integration**: `crates/lsp/src/treesitter_utils.rs` - AST parsing utilities

## High Level Architecture

### Technical Summary

This is a **Language Server Protocol (LSP) implementation** written in Rust that provides rich editing features for Beancount accounting files. The system follows a plugin-based architecture for beancount validation with three different execution strategies.

**Core Value Proposition**: Brings modern IDE features (completions, diagnostics, formatting, references) to the Beancount plain-text accounting ecosystem through LSP protocol compliance.

### Actual Tech Stack (from package.json/Cargo.toml)

| Category | Technology | Version | Notes |
|----------|------------|---------|--------|
| **Core Runtime** | Rust | 1.75.0+ | Stable toolchain, edition 2021 |
| **LSP Framework** | lsp-server | 0.7 | LSP protocol implementation |
| **LSP Types** | lsp-types | 0.97 | LSP data structures and protocol |
| **Text Processing** | ropey | 1.6 | Efficient rope data structure |
| **Parsing** | tree-sitter-beancount | 2.4.1 | Grammar-based parsing |
| **Pattern Matching** | regex | 1.0 | Error message parsing |
| **Python Integration** | pyo3 | 0.25 | Embedded Python (optional feature) |
| **Threading** | crossbeam-channel | 0.5 | Message passing between threads |
| **JSON/Config** | serde + serde_json | 1.0 | Configuration and data serialization |
| **CLI** | clap | 4.0 | Command line argument parsing |
| **Logging** | tracing + tracing-subscriber | 0.3 | Structured logging framework |
| **Error Handling** | anyhow + thiserror | 1.0/2.0 | Error propagation and custom errors |
| **VSCode Extension** | TypeScript | 4.6.3 | Client-side LSP integration |
| **VSCode LSP Client** | vscode-languageclient | 8.0.0-next.14 | LSP protocol client |

### Repository Structure Reality Check

- **Type**: Hybrid workspace (Rust workspace + NPM package for VSCode extension)
- **Package Manager**: Cargo (Rust) + NPM (VSCode extension)
- **Build System**: Cargo + cargo-dist for releases, esbuild for VSCode extension
- **Notable**: Nix flake for reproducible development environment

## Source Tree and Module Organization

### Project Structure (Actual)

```text
beancount-language-server/
├── crates/lsp/              # Main Rust LSP server implementation
│   ├── src/
│   │   ├── main.rs          # CLI entry point with logging setup
│   │   ├── lib.rs           # Core LSP server logic and initialization
│   │   ├── server.rs        # LSP server state management and message loop
│   │   ├── handlers.rs      # LSP request/notification handlers
│   │   ├── config.rs        # Configuration management and defaults
│   │   ├── providers/       # LSP feature implementations
│   │   │   ├── completion.rs    # Autocompletion engine
│   │   │   ├── diagnostics.rs   # Multi-method validation provider
│   │   │   ├── formatting.rs    # Bean-format compatible formatter
│   │   │   ├── references.rs    # Find references implementation
│   │   │   └── text_document.rs # Document lifecycle management
│   │   ├── checkers/        # Pluggable bean-check validation strategies
│   │   │   ├── mod.rs           # Strategy trait and factory pattern
│   │   │   ├── system_call.rs   # Traditional bean-check subprocess
│   │   │   ├── pyo3_embedded.rs # Embedded Python validation (feature-gated)
│   │   │   └── types.rs         # Shared validation data structures
│   │   ├── document.rs      # Document representation and management
│   │   ├── forest.rs        # Multi-document forest management
│   │   ├── beancount_data.rs    # Beancount-specific data extraction
│   │   └── treesitter_utils.rs  # Tree-sitter parsing utilities
│   ├── tests/               # Integration and unit tests (insta snapshots)
│   └── Cargo.toml          # Package manifest with optional features
├── vscode/                  # VS Code extension (TypeScript)
│   ├── src/
│   │   ├── extension.ts         # Main extension entry point
│   │   ├── config.ts           # Configuration management
│   │   ├── persistent_state.ts # Client-side state persistence
│   │   ├── semantic_tokens.ts  # Semantic highlighting (tree-sitter)
│   │   └── util.ts             # Utility functions
│   ├── package.json        # Extension manifest and dependencies
│   └── language-configuration.json # Beancount language definition
├── python/                  # Python integration utilities
│   └── bean_check.py       # Enhanced validation script with JSON output
├── docs/                    # Additional documentation
│   └── completion-system.md # Completion system architecture
├── flake.nix               # Nix development environment with Crane
├── Cargo.toml              # Workspace configuration
└── cliff.toml              # git-cliff changelog configuration
```

### Key Modules and Their Purpose

**Core LSP Infrastructure**:
- **Server State Management**: `src/server.rs` - Manages LSP connection state, document forest, configuration
- **Message Dispatcher**: `src/dispatcher.rs` - Routes LSP messages to appropriate handlers
- **Document Forest**: `src/forest.rs` - Multi-file document management with tree-sitter integration

**Feature Providers** (implements LSP capabilities):
- **Completion Engine**: `src/providers/completion.rs` - Context-aware completions for accounts, payees, dates
- **Diagnostics Provider**: `src/providers/diagnostics.rs` - Pluggable validation with multiple backends
- **Formatting Provider**: `src/providers/formatting.rs` - Bean-format compatible with configuration options
- **References Provider**: `src/providers/references.rs` - Find all references across files

**Validation Architecture** (Strategy pattern):
- **Strategy Interface**: `src/checkers/mod.rs` - BeancountChecker trait and factory
- **System Call Checker**: `src/checkers/system_call.rs` - Traditional bean-check subprocess execution
- **Embedded Python**: `src/checkers/pyo3_embedded.rs` - Direct Python library integration (optional)

**Configuration System**:
- **Config Management**: `src/config.rs` - JSON-based configuration with defaults and validation
- **LSP Integration**: Configuration passed via LSP initialization options

## Data Models and APIs

### Core Data Structures

**Document Representation**:
- **Document**: `src/document.rs` - Individual beancount file with rope-based text storage
- **BeancountData**: `src/beancount_data.rs` - Extracted semantic information (accounts, payees, etc.)
- **Forest**: `src/forest.rs` - Multi-document management with include file resolution

**Validation Types**:
- **BeancountCheckResult**: `src/checkers/types.rs` - Validation results with errors and flagged entries
- **BeancountError**: Structured error representation with file/line information
- **FlaggedEntry**: Warning-level issues found in beancount files

### Configuration Schema

See `crates/lsp/src/config.rs` for complete configuration structure:

```rust
pub struct Config {
    pub journal_file: Option<PathBuf>,
    pub bean_check_config: BeancountCheckConfig,
    pub formatting_options: Option<FormattingOptions>,
    // ... other fields
}
```

**Bean-check Configuration**: Three validation methods with different performance/accuracy tradeoffs
**Formatting Options**: Bean-format compatibility with prefix_width, num_width, currency_column support

### LSP Protocol Implementation

**Supported LSP Features**:
- `textDocument/completion` - Context-aware autocompletion
- `textDocument/publishDiagnostics` - Multi-method validation
- `textDocument/formatting` - Document formatting
- `textDocument/references` - Find all references
- `textDocument/rename` - Symbol renaming across files

**VSCode Extension Integration**:
- **Language Configuration**: `.beancount` and `.bean` file association
- **Tree-sitter Grammar**: Client-side syntax highlighting
- **Configuration Bridge**: Maps VSCode settings to LSP initialization options

## Technical Debt and Known Issues

### Architectural Strengths

1. **Clean Strategy Pattern**: Checker architecture is well-designed and extensible
2. **Proper LSP Implementation**: Follows LSP protocol correctly with good client compatibility
3. **Tree-sitter Integration**: Fast, incremental parsing with proper AST handling
4. **Feature Flag System**: Optional PyO3 integration properly feature-gated
5. **Comprehensive Testing**: Good test coverage with snapshot testing (insta)

### Areas for Improvement

1. **Python Script Method**: The python-script checker method (`python/bean_check.py`) is basic and could use better error handling
2. **Configuration Validation**: Limited validation of user-provided configuration options
3. **Error Recovery**: Some error conditions could have more graceful degradation
4. **Caching Strategy**: Diagnostic results and completion data could benefit from caching
5. **Performance Monitoring**: No built-in performance metrics or profiling capabilities

### Technical Constraints

1. **Python Dependency**: Diagnostics require Python beancount library to be installed
2. **Tree-sitter Version**: Tied to specific tree-sitter-beancount grammar version
3. **LSP Protocol Limits**: Some advanced features constrained by LSP specification
4. **Single-threaded Processing**: Most operations are single-threaded (could benefit from parallelization)

## Integration Points and External Dependencies

### External Services and Tools

| Service/Tool | Purpose | Integration Type | Key Files |
|--------------|---------|------------------|-----------|
| **beancount (Python)** | Validation and parsing | System command / Python import | `checkers/system_call.rs`, `checkers/pyo3_embedded.rs` |
| **bean-check** | Traditional validation | System command execution | `checkers/system_call.rs` |
| **tree-sitter-beancount** | AST parsing | Direct library integration | `treesitter_utils.rs` |
| **VSCode** | Editor integration | LSP protocol | `vscode/src/extension.ts` |

### Internal Integration Points

**LSP Protocol Compliance**:
- **Message Handling**: Bidirectional JSON-RPC over stdio/TCP
- **Document Lifecycle**: Proper handling of open/change/save/close events
- **Configuration Management**: LSP initialization options and workspace/did_change_configuration

**Multi-File Processing**:
- **Include Resolution**: Automatic discovery and processing of included beancount files
- **Cross-File References**: Find references and rename operations across file boundaries
- **Forest Management**: Efficient tracking of document dependencies and changes

**Validation Pipeline**:
- **Strategy Selection**: Factory pattern for choosing validation method
- **Error Aggregation**: Combining results from multiple validation sources
- **Incremental Updates**: Re-validation on file changes with proper debouncing

## Development and Deployment

### Local Development Setup

**Using Nix (Recommended)**:
```bash
nix develop  # Enters development shell with all dependencies
```

**Manual Setup**:
```bash
# Install Rust toolchain
rustup install stable
# Install Python dependencies
pip install beancount  # Required for diagnostics
# Build language server
cargo build --release
# VSCode extension development
cd vscode && npm install && npm run build
```

### Build and Deployment Process

**Release Process** (automated via GitHub Actions):
- **cargo-dist**: Cross-platform binary builds for Linux (x86_64, aarch64, loongarch64), macOS (x86_64, aarch64), Windows (x86_64)
- **Deployment Targets**: GitHub releases, Crates.io, Homebrew, Nix packages
- **VSCode Extension**: Manual packaging via `npm run package` → VSIX file

**Development Commands**:
```bash
# Core development
cargo build                    # Standard build
cargo build --features python-embedded  # With embedded Python
cargo test                    # Run all tests
cargo clippy --all-targets    # Linting
cargo fmt                     # Code formatting

# VSCode extension
cd vscode
npm run build                 # Build extension
npm run watch                 # Watch mode
npm run package              # Create VSIX package
```

### Quality Assurance

**Testing Strategy**:
- **Unit Tests**: Comprehensive test coverage with `cargo test`
- **Integration Tests**: End-to-end LSP functionality testing
- **Snapshot Testing**: Using `insta` for output validation
- **Feature Testing**: Optional PyO3 feature tested separately

**CI/CD Pipeline** (GitHub Actions):
- **Continuous Integration**: Format, clippy, tests on multiple OS/Rust versions
- **Security**: CodeQL analysis for vulnerability scanning
- **Release Automation**: Automated binary builds and publishing
- **Quality Gates**: All checks must pass before merge

## Current Feature Implementation Status

### ✅ Fully Implemented

| Feature | Implementation Status | Performance Notes |
|---------|----------------------|-------------------|
| **Completions** | Production ready | Fast with tree-sitter parsing |
| **Diagnostics** | Multiple backends available | Depends on chosen validation method |
| **Formatting** | Bean-format compatible | Configurable width/alignment options |
| **References** | Cross-file support | Efficient with forest management |
| **Rename** | Symbol renaming | Works across file boundaries |

### 📋 Planned Features (from README)

| Feature | Priority | Implementation Complexity |
|---------|----------|---------------------------|
| **Hover** | High | Medium (requires balance computation) |
| **Go to Definition** | High | Low (similar to references) |
| **Document Symbols** | High | Medium (requires semantic analysis) |
| **Folding Ranges** | Medium | Low (tree-sitter based) |
| **Semantic Highlighting** | Medium | Medium (requires token classification) |
| **Code Actions** | Medium | High (requires transaction analysis) |

## Configuration and Customization

### Runtime Configuration

**Configuration Sources** (in precedence order):
1. LSP initialization options (primary)
2. Default configuration values
3. Environment-based detection (PATH for executables)

**Key Configuration Areas**:
- **Journal File**: Path to main beancount file for validation
- **Validation Method**: Choice between system/python-script/python-embedded
- **Formatting Options**: Width settings and alignment preferences
- **Logging**: File-based logging with configurable levels

### Editor Integration Status

**Production Ready**:
- **VSCode**: Full extension with marketplace publication
- **Neovim**: Well-documented nvim-lspconfig integration
- **Helix**: Configuration examples provided

**Community Supported**:
- **Emacs**: lsp-mode integration documented
- **Vim**: vim-lsp configuration examples
- **Sublime Text**: LSP package integration

## Performance Characteristics

### Current Performance Profile

**Startup Time**: Sub-second initialization for most projects
**Memory Usage**: Moderate (depends on project size and validation method)
**Validation Speed**: 
- System call: Fastest startup, moderate validation speed
- Python embedded: Higher memory but fastest validation
- Python script: Slowest but most flexible

**Scalability Limits**:
- **File Count**: Tested with projects up to 50+ files
- **File Size**: Individual files up to several MB handled efficiently
- **Concurrent Operations**: Single-threaded processing with proper debouncing

### Optimization Opportunities

1. **Parallel Processing**: Validation and completion could be parallelized
2. **Incremental Parsing**: More granular tree-sitter update tracking
3. **Result Caching**: Cache validation results and completion data
4. **Memory Optimization**: More efficient document storage for large projects

## Appendix - Useful Commands and Scripts

### Development Workflow

```bash
# Quick development cycle
cargo watch -x 'test'         # Auto-test on changes
cargo run -- --stdio         # Run server locally
cargo run -- --log debug     # Run with debug logging

# Release preparation
git cliff                     # Generate changelog
cargo dist build             # Test release build
cargo dist plan             # Review release plan

# Nix-based development
nix flake check              # Run all checks
nix build                    # Build with nix
nix develop                  # Enter dev environment
```

### Debugging and Troubleshooting

**Log Analysis**:
- **Server Logs**: `beancount-language-server.log` (when --log flag used)
- **Debug Mode**: Set log level to `debug` or `trace` for detailed output
- **VSCode Logs**: Check Output panel → Beancount Language Server

**Common Issues**:
- **Bean-check Not Found**: Ensure `bean-check` binary is in PATH or configure path
- **Python Import Errors**: Verify beancount library is installed in correct Python environment
- **Performance Issues**: Consider switching to python-embedded method for large projects
- **Configuration Problems**: Check LSP initialization options format and values

**Testing Specific Features**:
```bash
# Test specific validation methods
cargo test --features python-embedded  # Test PyO3 integration
cargo test system_call                 # Test system call validation
cargo test completion                  # Test completion engine
```