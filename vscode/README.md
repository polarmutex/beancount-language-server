# Beancount for VS Code

VS Code extension that ships the Beancount language server for fast, accurate editing of Beancount journals. Bundles the binaries from the polarmutex/beancount-language-server project: https://github.com/polarmutex/beancount-language-server

![License](https://img.shields.io/github/license/polarmutex/beancount-language-server)
![Marketplace](https://img.shields.io/visual-studio-marketplace/v/polarmutex.beancount-langserver)

## What you get

- Completions for accounts, payees, dates, narration, tags, links, and transaction types
- Diagnostics powered by the `beancount` Python package
- Formatting aligned with `bean-format` options (prefix width, number width, currency column)
- Rename, references, and semantic highlighting

## Requirements

- Python with the `beancount` package installed in the environment VS Code uses:

  ```bash
  pip install beancount
  ```

## Install

1. Install from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=polarmutex.beancount-langserver).
2. Open a Beancount file; the extension will start the bundled server binary for your platform.

## Configure

Add the settings you need to `settings.json`:

```jsonc
{
  "beancountLangServer.journalFile": "/path/to/main.beancount",
  "beancountLangServer.formatting": {},
  // "beancountLangServer.serverPath": "/custom/path/to/beancount-language-server",
}
```

Key options:

- `serverPath` (optional): Use a custom server binary path instead of the bundled one (set if you want to run your own build)
- `journalFile`: Path to your primary Beancount journal
- `formatting.*`: formatting related option, check https://github.com/polarmutex/beancount-language-server?tab=readme-ov-file#%EF%B8%8F-configuration for more details

## Troubleshooting

- Make sure Python and the `beancount` package are available in the same environment VS Code uses.
- If diagnostics do not appear, run `bean-check` (or your configured command) in a VS Code terminal to confirm it works there.

## Develop the extension

```bash
pnpm install
pnpm run build   # build the extension
```

Server development and contribution guidelines live in the repository root README.
