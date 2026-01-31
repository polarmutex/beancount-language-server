# Beancount Language Server

![License](https://img.shields.io/github/license/polarmutex/beancount-language-server)
![GitHub release (latest by date)](https://img.shields.io/github/v/release/polarmutex/beancount-language-server)
![Crates.io](https://img.shields.io/crates/v/beancount-language-server)

A [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) (LSP) implementation for [Beancount](https://beancount.github.io/), the double-entry bookkeeping language. This provides rich editing features like completions, diagnostics, formatting, and more for Beancount files in your favorite editor.

![nixos](https://socialify.git.ci/polarmutex/beancount-language-server/image?description=1&font=Source%20Code%20Pro&owner=1&pattern=Circuit%20Board&stargazers=1&theme=Dark)

## ✨ Features

### 🚀 Currently Implemented

| LSP Feature               | Description                                                                                                              | Status |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------ | ------ |
| **Completions**           | Smart autocompletion for accounts, payees, dates, narration, tags, links, and transaction types                          | ✅     |
| **Diagnostics**           | Real-time error checking and validation via beancount Python integration                                                 | ✅     |
| **Formatting**            | Document formatting compatible with `bean-format`, with support for prefix-width, num-width, and currency-column options | ✅     |
| **Rename**                | Rename symbols across files                                                                                              | ✅     |
| **References**            | Find all references to accounts, payees, etc.                                                                            | ✅     |
| **Semantic Highlighting** | Advanced syntax highlighting with semantic information                                                                   | ✅     |
| **Inlay Hints**           | Show calculated balancing amounts and unbalanced transaction warnings                                                    | ✅     |

### 📋 Completion Types

- **Accounts**: Autocomplete account names with hierarchy support (`Assets:Checking`)
- **Payees**: Previously used payee names
- **Dates**: Smart date completion (today, this month, previous month, next month)
- **Narration**: Previously used transaction descriptions
- **Tags**: Complete hashtags (`#vacation`)
- **Links**: Complete links (`^receipt-123`)
- **Transaction Types**: `txn`, `balance`, `open`, `close`, etc.

### 💡 Inlay Hints

Non-intrusive inline annotations that help visualize implicit information:

- **Calculated Balancing Amounts**: When a posting omits an amount, shows the implicit balancing amount at the end of that posting line, aligned with other amounts
- **Unbalanced Transaction Warnings**: When all postings have explicit amounts but don't balance to zero, shows a warning with the unbalanced total on the transaction line

**Examples:**

```beancount
2024-01-15 * "Grocery Store"
  Expenses:Food:Groceries              45.23 USD
  Assets:Bank:Checking                         -45.23 USD  ; ← Shown as inlay hint

2024-01-15 * "Unbalanced Transfer"  /* = 500.00 USD ⚠ */  ; ← Warning shown
  Assets:Savings                     1000.00 USD
  Assets:Checking                    -500.00 USD
```

### 🔮 Planned Features

| LSP Feature           | Description                                                    | Priority |
| --------------------- | -------------------------------------------------------------- | -------- |
| **Hover**             | Show account balances, transaction details, account metadata   | High     |
| **Go to Definition**  | Jump to account/payee/commodity definitions                    | High     |
| **Document Symbols**  | Outline view showing accounts, transactions, and structure     | High     |
| **Folding Ranges**    | Fold transactions, account hierarchies, and multi-line entries | Medium   |
| **Code Actions**      | Quick fixes, refactoring, auto-balance transactions            | Medium   |
| **Signature Help**    | Help with transaction syntax and directive parameters          | Low      |
| **Workspace Symbols** | Find accounts, payees, commodities across all files            | Low      |

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

# Standard build (includes PyO3 embedded Python support by default)
cargo build --release

# Build without PyO3 (minimal binary, requires external bean-check/python)
cargo build --release --no-default-features
```

The binary will be available at `target/release/beancount-language-server`.

## 🔧 Requirements

### For Diagnostics (Bean-check)

The language server requires **one** of the following for validation and diagnostics:

**Option 1: PyO3 Embedded (Default - Recommended)**

- **Python 3.8+** installed on your system
- **beancount** Python package
  ```bash
  pip install beancount
  ```
- **Pre-built binaries** from GitHub releases include PyO3 support by default
- **Performance**: 60-66x faster than subprocess-based methods (~838μs vs ~50ms per check)
- **Note**: If beancount is not available, automatically falls back to other methods

**Option 2: System Python (Fallback)**

- **Python** with beancount library
- Used automatically if PyO3 checker is unavailable
- Invokes Python via subprocess for validation

**Option 3: Bean-check Binary (Fallback)**

- Traditional `bean-check` command-line tool
- Install via: `pip install beancount` (includes bean-check)
- Used if Python methods are unavailable

### Performance Comparison

Based on comprehensive benchmarks with a 30-line beancount file:

| Method                      | Average Time | Relative Speed    | Availability                     |
| --------------------------- | ------------ | ----------------- | -------------------------------- |
| **PyO3 Embedded** (default) | **~838μs**   | **1x (baseline)** | Requires Python 3.8+ + beancount |
| System Python               | ~50.1ms      | 60x slower        | Requires Python + beancount      |
| Bean-check Binary           | ~55.2ms      | 66x slower        | Requires bean-check binary       |

**Recommendation**: Use PyO3 embedded checker (default in pre-built binaries) for optimal performance.

## ⚙️ Configuration

The language server accepts configuration via LSP initialization options:

```json
{
  // Optional: Only needed for multi-file projects with include directives
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

**Note**: All configuration is optional. The language server will auto-detect the best checker method (PyO3 → System Python → Bean-check).

### Configuration Options

| Option         | Type   | Description                                                                                                                                                                                   | Default |
| -------------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| `journal_file` | string | Path to the main beancount journal file. **Optional**: Only required if your beancount files use `include` directives to span multiple files. Single-file projects work without this setting. | None    |

### Workspace-Specific Configuration

The `journal_file` setting is **workspace-specific**. Each editor workspace (project folder) can have its own journal file configured. This means:

- **Completions are scoped**: Account names, payees, currencies, tags, and links are loaded only from the configured journal and its included files
- **Separate ledgers**: If you work on multiple beancount projects (personal finances, business, etc.), each workspace uses its own configuration
- **No cross-contamination**: Accounts from one ledger won't appear as completions in another

**Example workflow with multiple ledgers:**

```
~/finances/personal/     # Workspace 1: journal_file = "main.beancount"
  ├── main.beancount     # Includes accounts/*.beancount
  └── accounts/
      └── assets.beancount

~/finances/business/     # Workspace 2: journal_file = "ledger.beancount"
  ├── ledger.beancount   # Includes 2024/*.beancount
  └── 2024/
      └── transactions.beancount
```

When editing files in `~/finances/personal/`, completions only show accounts like `Assets:Personal:Checking`. When editing in `~/finances/business/`, completions show `Assets:Business:Operating`.

### Bean-check Configuration

| Option                      | Type   | Description                                                        | Default |
| --------------------------- | ------ | ------------------------------------------------------------------ | ------- |
| `bean_check.method`         | string | Validation method: "system", "python-system", or "python-embedded" | None    |
| `bean_check.bean_check_cmd` | string | Path to bean-check binary (for "system" method)                    | None    |
| `bean_check.python_cmd`     | string | Path to Python executable (for Python methods)                     | None    |

**Preferred checker order (when `bean_check.method` is not set):**

1. `python-embedded` (if built with the feature and available)
2. `python-system` (if a compatible Python with beancount is available)
3. `system` (if bean-check is available)

#### Configuration Examples

**Default (no configuration needed):**

The language server automatically selects the best available checker method:

1. PyO3 Embedded (if Python 3.8+ with beancount is available)
2. System Python (if Python with beancount is available)
3. System Call (if bean-check binary is available)

No configuration required! Just install Python and beancount.

**Override to force a specific method:**

Only configure `bean_check.method` if you need to override auto-detection:

```json
{
  "bean_check": {
    "method": "system", // Force bean-check binary
    "bean_check_cmd": "/usr/local/bin/bean-check"
  }
}
```

```json
{
  "bean_check": {
    "method": "python-system", // Force Python subprocess
    "python_cmd": "/usr/bin/python3"
  }
}
```

```json
{
  "bean_check": {
    "method": "python-embedded" // Force PyO3 (already default)
  }
}
```

#### Troubleshooting PyO3 Checker

If the PyO3 embedded checker is not working:

1. **Verify Python installation**:

   ```bash
   python3 --version  # Should be 3.8 or higher
   ```

2. **Verify beancount installation**:

   ```bash
   python3 -c "import beancount.loader; print('Beancount OK')"
   ```

3. **Check language server logs** for PyO3-related messages:
   - VSCode: View → Output → Select "Beancount Language Server"
   - Neovim: `:LspLog`
   - Look for messages like "PyO3EmbeddedChecker: failed to import beancount.loader"

4. **Install beancount if missing**:

   ```bash
   # System-wide
   pip3 install beancount

   # User installation (no sudo required)
   pip3 install --user beancount

   # Virtual environment (recommended)
   python3 -m venv ~/.beancount-env
   source ~/.beancount-env/bin/activate
   pip install beancount
   ```

5. **Fallback methods**: If PyO3 checker fails, the language server automatically tries:
   - System Python method (python -c with beancount)
   - System Call method (bean-check binary)

   Check your configuration if you need to explicitly set a method.

### Formatting Options

| Option                    | Type   | Description                                                 | Default            | Bean-format Equivalent     |
| ------------------------- | ------ | ----------------------------------------------------------- | ------------------ | -------------------------- |
| `prefix_width`            | number | Fixed width for account names (overrides auto-detection)    | Auto-calculated    | `--prefix-width` (`-w`)    |
| `num_width`               | number | Fixed width for number alignment (overrides auto-detection) | Auto-calculated    | `--num-width` (`-W`)       |
| `currency_column`         | number | Align currencies at this specific column                    | None (right-align) | `--currency-column` (`-c`) |
| `account_amount_spacing`  | number | Minimum spaces between account names and amounts            | 2                  | N/A                        |
| `number_currency_spacing` | number | Number of spaces between number and currency                | 1                  | N/A                        |

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
2. Configure in `settings.json` (optional):
   ```json
   {
     // Optional: Only needed for multi-file projects with include directives
     "beancountLangServer.journalFile": "/path/to/main.beancount",
     "beancountLangServer.formatting": {
       "prefix_width": 30,
       "currency_column": 60,
       "number_currency_spacing": 1
     }
   }
   ```

**Workspace-specific configuration**: Create a `.vscode/settings.json` in each project folder:

```json
{
  "beancountLangServer.journalFile": "${workspaceFolder}/main.beancount"
}
```

This ensures each workspace uses its own journal file for completions and diagnostics.

### Neovim

Using `nvim.lsp` (nvim > 0.11)

`lsp/beancount.lua`

```lua
return {
    commands = { "beancount-language-server", "--stdio" },
    root_markers = { "main.bean", ".git" },
    -- init_options are optional
    init_options = {
        -- Optional: Only needed for multi-file projects with include directives
        journal_file = "main.bean",
    },
    settings = {
        beancount = {
            formatting = {
                prefix_width = 30,
                currency_column = 60,
                number_currency_spacing = 1,
            }
        }
    }
}
```

Using [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig):

```lua
local lspconfig = require('lspconfig')

lspconfig.beancount.setup({
  -- All init_options are optional
  init_options = {
    -- Optional: Only needed for multi-file projects with include directives
    -- journal_file = "/path/to/main.beancount",
    formatting = {
      prefix_width = 30,
      currency_column = 60,
      number_currency_spacing = 1,
    },
  },
})

-- To override auto-detected checker method:
-- lspconfig.beancount.setup({
--   init_options = {
--     bean_check = {
--       method = "system",  -- Force specific method: "python-embedded", "python-system", or "system"
--     },
--   },
-- })
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

**Workspace-specific configuration**: Use `.nvim.lua` or `exrc` for per-project settings:

```lua
-- .nvim.lua in your beancount project root
vim.lsp.config.beancount = {
  init_options = {
    journal_file = vim.fn.getcwd() .. "/main.beancount",
  },
}
```

Or with nvim-lspconfig, use `on_new_config` to dynamically set the journal file:

```lua
lspconfig.beancount.setup({
  on_new_config = function(new_config, new_root_dir)
    new_config.init_options = new_config.init_options or {}
    new_config.init_options.journal_file = new_root_dir .. "/main.beancount"
  end,
})
```

### Helix

Add to your `languages.toml`:

```toml
[language-server.beancount-language-server]
command = "beancount-language-server"
args = ["--stdio"]

# Configuration is optional
[language-server.beancount-language-server.config]
# Optional: Only needed for multi-file projects with include directives
# journal_file = "/path/to/main.beancount"

# Optional: bean_check config (uses python-embedded by default)
# [language-server.beancount-language-server.config.bean_check]
# method = "python-embedded"  # or "python-system" or "system"

# Optional: formatting configuration
[language-server.beancount-language-server.config.formatting]
prefix_width = 30
currency_column = 60
number_currency_spacing = 1

[[language]]
name = "beancount"
language-servers = [{ name = "beancount-language-server" }]
```

### Zed

Add to your `settings.json` (access via `Zed > Settings > Open Settings`):

```json
{
  "lsp": {
    "beancount-language-server": {
      "binary": {
        "path": "beancount-language-server",
        "arguments": ["--stdio"]
      },
      "initialization_options": {
        // Optional: Only needed for multi-file projects with include directives
        "journal_file": "/path/to/main.beancount",
        "formatting": {
          "prefix_width": 30,
          "currency_column": 60,
          "number_currency_spacing": 1
        }
      }
    }
  },
  "languages": {
    "Beancount": {
      "language_servers": ["beancount-language-server"]
    }
  }
}
```

For **workspace-specific configuration**, create a `.zed/settings.json` in your project root:

```json
{
  "lsp": {
    "beancount-language-server": {
      "initialization_options": {
        "journal_file": "main.beancount"
      }
    }
  }
}
```

**Note**: Zed may require a [Beancount extension](https://zed.dev/extensions) for syntax highlighting. The language server provides completions, diagnostics, and formatting regardless of syntax highlighting support.

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
    ;; All options are optional
    (lambda () (list
                ;; Optional: Only needed for multi-file projects with include directives
                ;; :journal_file "/path/to/main.beancount"
                ;; Optional: bean_check config (uses python-embedded by default)
                ;; :bean_check '(:method "python-embedded")
                :formatting '(:prefix_width 30 :currency_column 60 :number_currency_spacing 1))))))
```

**Workspace-specific configuration**: Use `.dir-locals.el` in your project root:

```elisp
;; .dir-locals.el
((beancount-mode
  . ((lsp-clients-beancount-langserver-init-options
      . (:journal_file "./main.beancount")))))
```

Or dynamically set based on project root:

```elisp
(defun my/beancount-lsp-init-options ()
  "Generate init options with project-local journal file."
  (let ((journal-file (expand-file-name "main.beancount" (project-root (project-current)))))
    (when (file-exists-p journal-file)
      (list :journal_file journal-file))))

;; Use in lsp-register-client with :initialization-options #'my/beancount-lsp-init-options
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
        \   'formatting': {
        \     'prefix_width': 30,
        \     'currency_column': 60,
        \     'number_currency_spacing': 1
        \   }
        \ }
    \ })
    " Optional: For multi-file projects with include directives, add:
    " \   'journal_file': '/path/to/main.beancount',
    " Optional: To override default checker method, add:
    " \   'bean_check': {'method': 'python-embedded'},
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
      // All initializationOptions are optional
      "initializationOptions": {
        // Optional: Only needed for multi-file projects with include directives
        // "journal_file": "/path/to/main.beancount",
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
- **Diagnostic Provider**: Multi-method validation system with pluggable checkers
- **Bean-check Integration**: Three validation methods (system, python-embedded)
- **Formatter**: Code formatting fully compatible with bean-format, supporting prefix-width, num-width, and currency-column options

### Project Structure

```
beancount-language-server/
├── crates/lsp/           # Main LSP server implementation
│   ├── src/
│   │   ├── handlers.rs   # LSP request/notification handlers
│   │   ├── providers/    # Feature providers (completion, diagnostics, etc.)
│   │   ├── checkers/     # Bean-check validation implementations
│   │   │   ├── mod.rs    # Strategy trait and factory pattern
│   │   │   ├── system_call.rs     # Traditional bean-check binary
│   │   │   ├── pyo3_embedded.rs   # PyO3 embedded Python
│   │   │   └── types.rs           # Shared data structures
│   │   └── server.rs     # Core LSP server logic
├── vscode/               # VS Code extension
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

# Run tests with PyO3 feature
cargo test --features python-embedded

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
