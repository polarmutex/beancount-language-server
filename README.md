# Beancount Language Server

![License](https://img.shields.io/github/license/polarmutex/beancount-language-server)
![GitHub release (latest by date)](https://img.shields.io/github/v/release/polarmutex/beancount-language-server)
![Crates.io](https://img.shields.io/crates/v/beancount-language-server)

A [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) (LSP) implementation for [Beancount](https://beancount.github.io/), the double-entry bookkeeping language. This provides rich editing features like completions, diagnostics, formatting, and more for Beancount files in your favorite editor.

![nixos](https://socialify.git.ci/polarmutex/beancount-language-server/image?description=1&font=Source%20Code%20Pro&owner=1&pattern=Circuit%20Board&stargazers=1&theme=Dark)

## ✨ Features

### 🚀 Currently Implemented

| LSP Feature | Description | Status |
|-------------|-------------|---------|
| **Completions** | Smart autocompletion for accounts, payees, dates, narration, tags, links, and transaction types | ✅ |
| **Diagnostics** | Real-time error checking and validation via beancount Python integration | ✅ |
| **Formatting** | Document formatting compatible with `bean-format`, with support for prefix-width, num-width, and currency-column options | ✅ |
| **Rename** | Rename symbols across files | ✅ |
| **References** | Find all references to accounts, payees, etc. | ✅ |
| **Semantic Highlighting** | Advanced syntax highlighting with semantic information | Partial |

### 📋 Completion Types

- **Accounts**: Autocomplete account names with hierarchy support (`Assets:Checking`)
- **Payees**: Previously used payee names
- **Dates**: Smart date completion (today, this month, previous month, next month)
- **Narration**: Previously used transaction descriptions
- **Tags**: Complete hashtags (`#vacation`)
- **Links**: Complete links (`^receipt-123`)
- **Transaction Types**: `txn`, `balance`, `open`, `close`, etc.

### 🔮 Planned Features

| LSP Feature | Description | Priority |
|-------------|-------------|----------|
| **Hover** | Show account balances, transaction details, account metadata | High |
| **Go to Definition** | Jump to account/payee/commodity definitions | High |
| **Document Symbols** | Outline view showing accounts, transactions, and structure | High |
| **Folding Ranges** | Fold transactions, account hierarchies, and multi-line entries | Medium |
| **Code Actions** | Quick fixes, refactoring, auto-balance transactions | Medium |
| **Inlay Hints** | Show computed balances, exchange rates, running totals | Low |
| **Signature Help** | Help with transaction syntax and directive parameters | Low |
| **Workspace Symbols** | Find accounts, payees, commodities across all files | Low |

## 📦 Installation

### Method 1: Cargo (Recommended)

```bash
cargo install beancount-language-server
```

### Method 2: GitHub Releases (Pre-built Binaries)

Download the latest release for your platform from the [releases page](https://github.com/polarmutex/beancount-language-server/releases).

**Supported Platforms:**
- Linux (x86_64, aarch64, loongarch64)
- macOS (x86_64, aarch64)  
- Windows (x86_64)

### Method 3: Homebrew (macOS/Linux)

```bash
brew install beancount-language-server
```

### Method 4: Nix

```bash
# Using nix-env
nix-env -iA nixpkgs.beancount-language-server

# Using nix shell
nix shell nixpkgs#beancount-language-server

# Development environment
nix develop
```

### Method 5: Build from Source

```bash
git clone https://github.com/polarmutex/beancount-language-server.git
cd beancount-language-server
cargo build --release
```

The binary will be available at `target/release/beancount-language-server`.

## 🔧 Requirements

### Required
- **Beancount**: Install the Python beancount package for diagnostics
  ```bash
  pip install beancount
  ```

### Optional
- **Bean-format**: The language server includes built-in formatting that's fully compatible with bean-format. Installing bean-format is optional for comparison or standalone use
  ```bash
  pip install bean-format
  ```

## ⚙️ Configuration

The language server accepts configuration via LSP initialization options:

```json
{
  "journal_file": "/path/to/main.beancount",
  "formatting": {
    "prefix_width": 30,
    "num_width": 10,
    "currency_column": 60,
    "account_amount_spacing": 2,
    "number_currency_spacing": 1
  }
}
```

### Configuration Options

| Option | Type | Description | Default |
|--------|------|-------------|---------|
| `journal_file` | string | Path to the main beancount journal file | None |

### Formatting Options

| Option | Type | Description | Default | Bean-format Equivalent |
|--------|------|-------------|---------|----------------------|
| `prefix_width` | number | Fixed width for account names (overrides auto-detection) | Auto-calculated | `--prefix-width` (`-w`) |
| `num_width` | number | Fixed width for number alignment (overrides auto-detection) | Auto-calculated | `--num-width` (`-W`) |
| `currency_column` | number | Align currencies at this specific column | None (right-align) | `--currency-column` (`-c`) |
| `account_amount_spacing` | number | Minimum spaces between account names and amounts | 2 | N/A |
| `number_currency_spacing` | number | Number of spaces between number and currency | 1 | N/A |

#### Formatting Modes

**Default Mode** (no `currency_column` specified):
- Accounts are left-aligned
- Numbers are right-aligned with consistent end positions
- Behaves like `bean-format` with no special options

**Currency Column Mode** (`currency_column` specified):
- Currencies are aligned at the specified column
- Numbers are positioned to place currencies at the target column
- Equivalent to `bean-format --currency-column N`

#### Examples

**Basic formatting with auto-detection:**
```json
{
  "formatting": {}
}
```

**Fixed prefix width (like `bean-format -w 25`):**
```json
{
  "formatting": {
    "prefix_width": 25
  }
}
```

**Currency column alignment (like `bean-format -c 60`):**
```json
{
  "formatting": {
    "currency_column": 60
  }
}
```

**Number-currency spacing control:**
```json
{
  "formatting": {
    "number_currency_spacing": 2
  }
}
```

This controls the whitespace between numbers and currency codes:
- `0`: No space (`100.00USD`)
- `1`: Single space (`100.00 USD`) - default
- `2`: Two spaces (`100.00  USD`)

**Combined options:**
```json
{
  "formatting": {
    "prefix_width": 30,
    "currency_column": 65,
    "account_amount_spacing": 3,
    "number_currency_spacing": 1
  }
}
```

## 🖥️ Editor Setup

### Visual Studio Code

1. Install the [Beancount extension](https://marketplace.visualstudio.com/items?itemName=polarmutex.beancount-langserver) from the marketplace
2. Configure in `settings.json`:
   ```json
   {
     "beancountLangServer.journalFile": "/path/to/main.beancount",
     "beancountLangServer.formatting": {
       "prefix_width": 30,
       "currency_column": 60,
       "number_currency_spacing": 1
     }
   }
   ```

### Neovim

Using [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig):

```lua
local lspconfig = require('lspconfig')

lspconfig.beancount.setup({
  init_options = {
    journal_file = "/path/to/main.beancount",
    formatting = {
      prefix_width = 30,
      currency_column = 60,
      number_currency_spacing = 1,
    },
  },
})
```

**File type detection**: Ensure beancount files are detected. Add to your config:
```lua
vim.filetype.add({
  extension = {
    beancount = "beancount",
    bean = "beancount",
  },
})
```

### Helix

Add to your `languages.toml`:

```toml
[language-server.beancount-language-server]
command = "beancount-language-server"
args = ["--stdio"]

[language-server.beancount-language-server.config]
journal_file = "/path/to/main.beancount"

[language-server.beancount-language-server.config.formatting]
prefix_width = 30
currency_column = 60
number_currency_spacing = 1

[[language]]
name = "beancount"
language-servers = [{ name = "beancount-language-server" }]
```

### Emacs

Using [lsp-mode](https://github.com/emacs-lsp/lsp-mode):

```elisp
(use-package lsp-mode
  :hook (beancount-mode . lsp-deferred)
  :config
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection "beancount-language-server")
    :major-modes '(beancount-mode)
    :server-id 'beancount-language-server
    :initialization-options
    (lambda () (list :journal_file "/path/to/main.beancount"
                     :formatting '(:prefix_width 30 :currency_column 60 :number_currency_spacing 1))))))
```

### Vim

Using [vim-lsp](https://github.com/prabirshrestha/vim-lsp):

```vim
if executable('beancount-language-server')
    au User lsp_setup call lsp#register_server({
        \ 'name': 'beancount-language-server',
        \ 'cmd': {server_info->['beancount-language-server']},
        \ 'allowlist': ['beancount'],
        \ 'initialization_options': {
        \   'journal_file': '/path/to/main.beancount',
        \   'formatting': {
        \     'prefix_width': 30,
        \     'currency_column': 60,
        \     'number_currency_spacing': 1
        \   }
        \ }
    \ })
endif
```

### Sublime Text

Using [LSP](https://packagecontrol.io/packages/LSP):

Add to LSP settings:
```json
{
  "clients": {
    "beancount-language-server": {
      "enabled": true,
      "command": ["beancount-language-server"],
      "selector": "source.beancount",
      "initializationOptions": {
        "journal_file": "/path/to/main.beancount",
        "formatting": {
          "prefix_width": 30,
          "currency_column": 60,
          "number_currency_spacing": 1
        }
      }
    }
  }
}
```

## 🏗️ Architecture

### High-Level Overview

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│     Editor      │◄──►│  LSP Server     │◄──►│   Beancount     │
│                 │    │                 │    │   (Python)      │
│ - VSCode        │    │ - Completion    │    │ - Validation    │
│ - Neovim        │    │ - Formatting    │    │ - Parsing       │
│ - Helix         │    │ - Diagnostics   │    │ - Bean-check    │
│ - Emacs         │    │ - Tree-sitter   │    │                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Core Components

- **LSP Server**: Main Rust application handling LSP protocol
- **Tree-sitter Parser**: Fast, incremental parsing of Beancount syntax
- **Completion Engine**: Smart autocompletion with context awareness
- **Diagnostic Provider**: Integration with beancount Python for validation
- **Formatter**: Code formatting fully compatible with bean-format, supporting prefix-width, num-width, and currency-column options

### Project Structure

```
beancount-language-server/
├── crates/lsp/           # Main LSP server implementation
│   ├── src/
│   │   ├── handlers.rs   # LSP request/notification handlers
│   │   ├── providers/    # Feature providers (completion, diagnostics, etc.)
│   │   └── server.rs     # Core LSP server logic
├── vscode/               # VS Code extension
├── python/               # Python integration utilities
└── flake.nix            # Nix development environment
```

## 🛠️ Development

### Prerequisites

- **Rust** (stable toolchain)
- **Python** with beancount
- **Node.js** (for VS Code extension)

### Development Environment

**Using Nix (Recommended):**
```bash
nix develop
```

**Manual Setup:**
```bash
# Install Rust dependencies
cargo build

# Install Node.js dependencies (for VS Code extension)
cd vscode && pnpm install

# Install development tools
cargo install cargo-watch
```

### Running Tests

```bash
# Run all tests
cargo test

# Run with coverage
cargo llvm-cov --all-features --locked --workspace --lcov --output-path lcov.info

# Run specific test
cargo test test_completion
```

### Code Quality

```bash
# Format code
cargo fmt

# Lint code
cargo clippy --all-targets --all-features

# Check formatting
cargo fmt -- --check
```

### Development Workflow

1. **Make changes** to the Rust code
2. **Test locally** with `cargo test`
3. **Run LSP server** in development mode:
   ```bash
   cargo run --bin beancount-language-server
   ```
4. **Test with editor** by configuring it to use the local binary

### VS Code Extension Development

```bash
cd vscode
pnpm run build      # Build extension
pnpm run watch      # Watch for changes
pnpm run package    # Package extension
```

### Release Process

The project uses [cargo-dist](https://opensource.axo.dev/cargo-dist/) for automated releases:

1. **Tag a release**: `git tag v1.0.0 && git push --tags`
2. **GitHub Actions** automatically builds and publishes:
   - Binaries for all supported platforms
   - Crates.io release
   - GitHub release with assets

## 🤝 Contributing

Contributions are welcome! Here are some ways to help:

### 🐛 Bug Reports
- Search existing issues first
- Include beancount file examples that trigger the bug
- Provide editor and OS information

### 💡 Feature Requests
- Check the [planned features](#-planned-features) list
- Describe the use case and expected behavior
- Consider the LSP specification constraints

### 🔧 Code Contributions

1. **Fork** the repository
2. **Create** a feature branch (`git checkout -b feature/amazing-feature`)
3. **Make** your changes with tests
4. **Ensure** code quality: `cargo fmt && cargo clippy && cargo test`
5. **Commit** your changes (`git commit -m 'Add amazing feature'`)
6. **Push** to the branch (`git push origin feature/amazing-feature`)
7. **Open** a Pull Request

### 🎯 Good First Issues

Look for issues labeled `good-first-issue`:
- Add new completion types
- Improve error messages
- Add editor configuration examples
- Improve documentation

## 📚 Resources

- **[Beancount Documentation](https://beancount.github.io/)**
- **[Language Server Protocol Specification](https://microsoft.github.io/language-server-protocol/)**
- **[Tree-sitter Beancount Grammar](https://github.com/polarmutex/tree-sitter-beancount)**
- **[VSCode Extension API](https://code.visualstudio.com/api)**

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- **[Beancount](https://github.com/beancount/beancount)** - The amazing double-entry bookkeeping language
- **[Tree-sitter](https://tree-sitter.github.io/)** - Incremental parsing framework
- **[LSP](https://microsoft.github.io/language-server-protocol/)** - Language Server Protocol specification
- **[Twemoji](https://github.com/twitter/twemoji)** - Emoji graphics used in the icon

---

<p align="center">
  <strong>Happy Beancounting! 📊✨</strong>
</p>
