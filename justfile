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

# Check all feature combinations (requires cargo-hack)
hack-check:
    cargo hack check --feature-powerset

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
ci: fmt-check clippy hack-check test build-release
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
all: fmt clippy hack-check test build
    @echo "✅ All tasks completed successfully!"

# Release preparation: all checks plus coverage
release-prep: fmt-check clippy hack-check test-all build-release coverage-lcov
    @echo "✅ Release preparation complete!"
    @echo "📊 Coverage report generated at lcov.info"


# lazy version so it will only be executed when we need it
VSCODE_EXT_VERSION:="(jq -r '.version' vscode/package.json)"
CARGO_VERSION:="(grep -A5 '^\\[workspace.package\\]' Cargo.toml | grep '^version =' | head -n1 | sed 's/version = \"\\(.*\\)\"/\\1/')"

# Create a VS Code release tag (vscode/vX.Y.Z) using version from vscode/package.json
tag-vscode:
    git tag -a "vscode/v`{{VSCODE_EXT_VERSION}}`" -m "bump(vscode): v`{{VSCODE_EXT_VERSION}}`"

# Create a cargo release tag (vX.Y.Z) using version from Cargo.toml workspace
tag-cargo:
    git tag -a "v`{{CARGO_VERSION}}`" -m "chore(release): v`{{CARGO_VERSION}}`"

# ========================================
# Release Management
# ========================================

# Update CHANGELOG using git-cliff
changelog:
    git cliff --output CHANGELOG.md

# Update CHANGELOG for unreleased changes
changelog-unreleased:
    git cliff --unreleased --output CHANGELOG.md

# Bump version and update changelog (requires cargo-edit)
# Usage: just release-bump [patch|minor|major]
release-bump LEVEL:
    #!/usr/bin/env bash
    set -euo pipefail

    # Verify clean working directory
    if [[ -n $(git status --porcelain) ]]; then
        echo "❌ Error: Working directory is not clean. Commit or stash changes first."
        exit 1
    fi

    # Get current version from workspace
    CURRENT_VERSION=$(grep -A5 "^\[workspace.package\]" Cargo.toml | grep "^version =" | head -n1 | sed 's/version = "\(.*\)"/\1/')
    echo "📦 Current version: $CURRENT_VERSION"

    # Bump version using cargo-set-version
    echo "⬆️  Bumping {{LEVEL}} version..."
    cargo set-version --workspace --bump {{LEVEL}}

    # Update Cargo.lock with new version
    echo "🔄 Updating Cargo.lock..."
    cargo update --workspace

    # Get new version from workspace
    NEW_VERSION=$(grep -A5 "^\[workspace.package\]" Cargo.toml | grep "^version =" | head -n1 | sed 's/version = "\(.*\)"/\1/')
    echo "📦 New version: $NEW_VERSION"

    # Update CHANGELOG
    echo "📝 Updating CHANGELOG..."
    git cliff --tag "v$NEW_VERSION" --output CHANGELOG.md

    # Stage changes
    git add Cargo.toml crates/*/Cargo.toml CHANGELOG.md Cargo.lock

    # Commit
    echo "💾 Creating release commit..."
    git commit -m "chore(release): prepare for v$NEW_VERSION"

    echo ""
    echo "✅ Release v$NEW_VERSION prepared!"

# Prepare patch release (0.0.x)
release-patch: release-prep
    just release-bump patch

# Prepare minor release (0.x.0)
release-minor: release-prep
    just release-bump minor

# Prepare major release (x.0.0)
release-major: release-prep
    just release-bump major
