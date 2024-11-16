# ![nixos](https://socialify.git.ci/polarmutex/beancount-language-server/image?description=1&font=Source%20Code%20Pro&owner=1&pattern=Circuit%20Board&stargazers=1&theme=Dark)

Thank you to the [Twemoji project](https://github.com/twitter/twemoji) for the usage of their emoji for the icon.

## Installation

The server can be installed via `cargo` (or from source).

```sh
cargo install beancount-language-server
```

Alternatively, it can be installed via Homebrew.

```sh
brew install beancount-language-server
```

Then, you should be able to run the language server with the following command:

```sh
beancount-language-server
```

Follow the instructions below to integrate the language server into your editor.

### Alternative: Compile and install from source

First, clone this repo and compile it.

```sh
git clone git@github.com:polarmutex/beancount-language-server.git
cd beancount-language-server
cargo build
```

## Requirements

You will need to install `beancount` to get all diagnostics.

```sh
pip install -g beancount
```

## Configuration

TODO

## Features

Supports Beancount v2

| Feature     | Description                                    |
| ----------- | ---------------------------------------------- |
| diagnostics | Provided via `beancount`                       |
| formatting  | Should generate edits silimar to `bean-format` |
| completions | Show completions for Payees, Accounts, Date    |
| definitions | Planned for future release                     |
| folding     | Planned for future release                     |
| hover       | Planned for future release                     |
| rename      | Planned for future release                     |

### Future

- updated vscode extension to use the rust version

## Editor Support

### Neovim

The settings for the language server are in the lspconfig repo

1. Install the beancount language server

   ```sh
   cargo install beancount-language-server
   ```

   However you install it, you need to remember how to access the binary

2. Create a lua lspconfig for the beancount LSP [example in my dotfiles](https://github.com/polarmutex/dotfiles/blob/master/neovim/lua/polarmutex/lsp/beancount.lua)

   - add the following code to your lspconfig

   ```lua
   local lspconfig = require 'lspconfig'
   lspconfig.beancount.setup= {
       init_options = {
           journal_file = "<path to journal file>",
       };
   };
   ```

3. Open a beancount file and verify LSP connected with the LSPInfo command

#### Troubleshooting

#### beancount file type not detected

If you notice beancount files not having the "beancount" type, you need a
neovim v0.5 or master built after Feb 17, 2021

If not the following in a file named `beancount.vim` in the `ftdetect` folder

```vim
function! s:setf(filetype) abort
    if &filetype !=# a:filetype
        let &filetype = a:filetype
    endif
endfunction

au BufNewFile,BufRead *.bean,*.beancount call s:setf('beancount')
```

### VS Code

Plan to make a VS Code extesion in the future

### Vim

Tested and Developed on Neovim v0.5 (master branch)

SETUP TODO

### Emacs

TODO

### Helix

1. Install beancount-language-server with `cargo install beancount-language-server`.
2. Add the following snippet to your [`languages.toml` file](https://docs.helix-editor.com/languages.html#languagestoml-files):
   ```toml
   [language-server.beancount-language-server]
   command = "beancount-language-server"
   args = ["--stdio"]
   config.journal_file = "<path to journal file>"

   [[language]]
   name = "beancount"
   language-servers = [{ name = "beancount-language-server" }]
   ```
3. Verify beancount-language-server shows as available in the output of `hx --health`.

## Contributing

Please do :)

## Previous Versions

### Typescript

not currently maintained, unless there is interest

[branch](https://github.com/polarmutex/beancount-language-server/tree/typescript)

### Python

no longer maintained

[branch](https://github.com/polarmutex/beancount-language-server/tree/python)
