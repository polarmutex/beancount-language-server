# Justfile for Beancount Language Server
# Run `just --list` to see all available commands

# Default recipe - show help
default:
    @just --list

# Run all tests
test:
    cargo test --all-features --workspace

# Run library tests only
test-lib:
    cargo test --lib --all-features

# Run all tests including ignored ones
test-all:
    cargo test --all-features --workspace -- --include-ignored

# Run tests in watch mode (requires cargo-watch)
test-watch:
    cargo watch -x "test --all-features --workspace"

# Run tests for a specific test name pattern
test-filter PATTERN:
    cargo test --all-features --workspace {{PATTERN}}

# Run clippy lints
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy and automatically fix issues
clippy-fix:
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged

# Build in debug mode
build:
    cargo build --all-features

# Build in release mode
build-release:
    cargo build --release --all-features

# Build with specific features
build-features FEATURES:
    cargo build --features {{FEATURES}}

# Clean build artifacts
clean:
    cargo clean

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Generate code coverage report (HTML)
coverage:
    cargo llvm-cov --all-features --locked --workspace --html

# Generate code coverage report (LCOV format for CI)
coverage-lcov:
    cargo llvm-cov --all-features --locked --workspace --lcov --output-path lcov.info

# Generate coverage and open in browser
coverage-open:
    cargo llvm-cov --all-features --locked --workspace --html --open

# Clean coverage artifacts
coverage-clean:
    cargo llvm-cov clean

# Run all CI checks (format, clippy, test, build)
ci: fmt-check clippy test build-release
    @echo "✅ All CI checks passed!"

# Quick check before committing
check: fmt clippy test
    @echo "✅ Pre-commit checks passed!"

# Install the language server locally
install:
    cargo install --path crates/lsp/ --all-features

# Install with Python embedded support
install-python:
    cargo install --path crates/lsp/ --features python-embedded

# Run the language server
run:
    cargo run --bin beancount-language-server

# Run the language server with debug logging
run-debug:
    RUST_LOG=debug cargo run --bin beancount-language-server

# Check for security vulnerabilities
audit:
    cargo audit

# Update dependencies
update:
    cargo update

# Show outdated dependencies
outdated:
    cargo outdated

# Generate documentation
doc:
    cargo doc --all-features --no-deps

# Generate and open documentation
doc-open:
    cargo doc --all-features --no-deps --open

# Benchmark tests (if any exist)
bench:
    cargo bench --all-features

# Run nix flake checks
nix-check:
    nix flake check

# Build with nix
nix-build:
    nix build

# Enter nix development shell
nix-shell:
    nix develop

# Run tree-sitter tests (if needed)
tree-sitter-test:
    tree-sitter test

# Combined: format, lint, test, and build
all: fmt clippy test build
    @echo "✅ All tasks completed successfully!"

# Release preparation: all checks plus coverage
release-prep: fmt-check clippy test-all build-release coverage-lcov
    @echo "✅ Release preparation complete!"
    @echo "📊 Coverage report generated at lcov.info"
