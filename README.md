# ![nixos](https://socialify.git.ci/polarmutex/beancount-language-server/image?description=1&font=Source%20Code%20Pro&owner=1&pattern=Circuit%20Board&stargazers=1&theme=Dark)

## Installation

The server can be installed via `cargo` (or from source).

```sh
cargo install beancount-langserver-server
```

Then, you should be able to run the language server with the following command:

```sh
beancount-langserver --stdio
```

Follow the instructions below to integrate the language server into your editor.

### Alternative: Compile and install from source

First, clone this repo and compile it.

```sh
git clone git@github.com/polarmutex/beancount-langserver.git
cd beancount-langserver
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

| Feature          | Description                                               |
| ---------------- | ----------------------------------------------------------|
| diagnostics      | Provided via `beancount`                                  |
| formatting       | Should generate edits silimar to `bean-format`            |
| completions      | Show completions for Payees, Accounts, Date               |
| definitions      | Planned for future release                                |
| folding          | Planned for future release                                |
| hover            | Planned for future release                                |
| rename           | Planned for future release                                |

### Future

* updated vscode extension to use the rust version

## Editor Support

### Neovim

The settings for the language server are in the lspconfig repo

1. Install the beancount language server

    ```sh
    cargo install beancount-language-server
    ```

    However you install it, you need to remember how to access the binary

2. Create a lua lspconfig for the beancount LSP [example in my dotfiles](https://github.com/polarmutex/dotfiles/blob/master/neovim/lua/polarmutex/lsp/beancount.lua)
    * add the following code to your lspconfig

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

## Contributing

Please do :)

## Previous Versions

### Typescript

not currently maintained, unless there is interest

[branch](https://github.com/polarmutex/beancount-language-server/tree/typescript)

### Python

no longer maintained

[branch](https://github.com/polarmutex/beancount-language-server/tree/python)
