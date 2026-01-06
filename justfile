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

# ========================================
# VSCode Extension
# ========================================

# Build the VSCode extension VSIX with Nix (default)
vscode-build:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "🔨 Building VSIX with Nix..."
    nix build .#beancount-vscode-vsix
    echo "✅ VSIX built:"
    ls -lh result/*.vsix

# Install the VSCode extension locally for testing
vscode-install: vscode-build
    #!/usr/bin/env bash
    set -euo pipefail
    VSIX=$(ls result/*.vsix 2>/dev/null | head -n1)
    if [[ -z "$VSIX" ]]; then
        echo "❌ No VSIX file found. Run 'just vscode-build' first."
        exit 1
    fi
    echo "📥 Installing $VSIX..."
    code --install-extension "$VSIX" --force
    echo "✅ Extension installed! Reload VSCode to use it."

# Test the VSCode extension (build, install, and show testing guide)
vscode-test: vscode-install
    #!/usr/bin/env bash
    set -euo pipefail
    echo "🧪 Extension installed and ready for testing!"
    echo ""
    echo "📝 Testing checklist:"
    echo "   1. Open a .beancount file"
    echo "   2. Check 'Output > Beancount Language Server' for logs"
    echo "   3. Test completions (Ctrl+Space)"
    echo "   4. Test formatting (Shift+Alt+F)"
    echo "   5. Verify diagnostics appear for errors"
    echo ""
    echo "🔍 To view extension logs:"
    echo "   - VSCode: View > Output > Select 'Beancount Language Server'"
    echo ""
    echo "✅ Happy testing!"

# Uninstall the VSCode extension
vscode-uninstall:
    code --uninstall-extension polarmutex.beancount-langserver

# Install VSCode extension dependencies
vscode-deps:
    #!/usr/bin/env bash
    set -euo pipefail
    cd vscode
    if command -v pnpm &> /dev/null; then
        pnpm install
    else
        echo "⚠️  pnpm not found. Running via nix develop..."
        cd .. && nix develop -c bash -c 'cd vscode && pnpm install'
    fi
    echo "✅ VSCode dependencies installed!"

# Clean VSCode build artifacts
vscode-clean:
    #!/usr/bin/env bash
    rm -rf result
    cd vscode 2>/dev/null && rm -rf dist/ out/ node_modules/ .cache/ || true
    echo "✅ Cleaned VSCode build artifacts"

# Check VSCode extension formatting and linting (auto-installs dependencies if needed)
vscode-lint:
    #!/usr/bin/env bash
    set -euo pipefail
    # Install dependencies if node_modules doesn't exist
    if [[ ! -d "vscode/node_modules" ]]; then
        echo "📦 Installing dependencies first..."
        just vscode-deps
    fi
    cd vscode
    if command -v pnpm &> /dev/null; then
        pnpm run lint
    else
        echo "⚠️  pnpm not found. Running via nix develop..."
        cd .. && nix develop -c bash -c 'cd vscode && pnpm run lint'
    fi
    echo "✅ VSCode extension lint check passed!"

# Auto-fix VSCode extension formatting and linting issues (auto-installs dependencies if needed)
vscode-fix:
    #!/usr/bin/env bash
    set -euo pipefail
    # Install dependencies if node_modules doesn't exist
    if [[ ! -d "vscode/node_modules" ]]; then
        echo "📦 Installing dependencies first..."
        just vscode-deps
    fi
    cd vscode
    if command -v pnpm &> /dev/null; then
        pnpm run fix
    else
        echo "⚠️  pnpm not found. Running via nix develop..."
        cd .. && nix develop -c bash -c 'cd vscode && pnpm run fix'
    fi
    echo "✅ VSCode extension formatting and linting fixed!"

# Run all VSCode extension checks (lint + build)
vscode-check: vscode-lint vscode-build
    @echo "✅ All VSCode extension checks passed!"

# Create a release tag (X.Y.Z) using version from Cargo.toml workspace
# Both Rust and VSCode releases use the same version/tag
tag-release:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(grep -A5 "^\[workspace.package\]" Cargo.toml | grep "^version =" | head -n1 | sed 's/version = "\(.*\)"/\1/')
    git tag -a "$VERSION" -m "chore(release): $VERSION"
    echo "✅ Created tag: $VERSION"
    echo "📋 Next step: git push origin $VERSION"

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

    # Update vscode/package.json version
    echo "📝 Updating vscode/package.json version to $NEW_VERSION..."
    jq --arg version "$NEW_VERSION" '.version = $version' vscode/package.json > vscode/package.json.tmp
    mv vscode/package.json.tmp vscode/package.json

    # Update CHANGELOG (prepend new release, keep existing)
    echo "📝 Updating CHANGELOG..."
    git cliff --unreleased --tag "$NEW_VERSION" --prepend CHANGELOG.md

    # Stage changes
    git add Cargo.toml crates/*/Cargo.toml vscode/package.json CHANGELOG.md Cargo.lock

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

# Auto-detect version bump from conventional commits and prepare release
release-auto: release-prep
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

    # Auto-detect next version using git-cliff
    echo "🔍 Analyzing conventional commits to determine version bump..."
    BUMPED_VERSION=$(git cliff --unreleased --bumped-version 2>/dev/null | tail -1 | sed 's/^v//')

    if [[ -z "$BUMPED_VERSION" ]]; then
        echo "❌ Error: Could not determine version bump from commits."
        echo "💡 Hint: Use conventional commits (feat:, fix:, BREAKING CHANGE:)"
        echo "   - feat: → minor version bump"
        echo "   - fix: → patch version bump"
        echo "   - BREAKING CHANGE: → major version bump"
        exit 1
    fi

    echo "⬆️  Detected version bump: $CURRENT_VERSION → $BUMPED_VERSION"

    # Set version directly
    echo "📝 Updating version to $BUMPED_VERSION..."
    cargo set-version --workspace "$BUMPED_VERSION"

    # Update Cargo.lock with new version
    echo "🔄 Updating Cargo.lock..."
    cargo update --workspace

    # Update vscode/package.json version
    echo "📝 Updating vscode/package.json version to $BUMPED_VERSION..."
    jq --arg version "$BUMPED_VERSION" '.version = $version' vscode/package.json > vscode/package.json.tmp
    mv vscode/package.json.tmp vscode/package.json

    # Update CHANGELOG (prepend new release, keep existing)
    echo "📝 Updating CHANGELOG..."
    git cliff --unreleased --tag "$BUMPED_VERSION" --prepend CHANGELOG.md

    # Stage changes
    git add Cargo.toml crates/*/Cargo.toml vscode/package.json CHANGELOG.md Cargo.lock

    # Commit
    echo "💾 Creating release commit..."
    git commit -m "chore(release): prepare for v$BUMPED_VERSION"

    echo ""
    echo "✅ Release v$BUMPED_VERSION prepared!"
    echo "📋 Next steps:"
    echo "   1. Review the changes: git show HEAD"
    echo "   2. Create and push tag: git tag v$BUMPED_VERSION && git push origin v$BUMPED_VERSION"
