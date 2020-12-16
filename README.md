# beancount-langserver
A Language Server Protocol (LSP) for beancount files

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->
**Table of Contents**

- [Installation](#installation)
  - [Alternative: Compile and install from source](#alternative-compile-and-install-from-source)
  - [Alternative: Install with Nix](#alternative-install-with-nix)
- [Requirements](#requirements)
- [Configuration](#configuration)
- [Features](#features)
- [Server Settings](#server-settings)
- [Editor Support](#editor-support)
  - [VSCode](#vscode)
  - [Vim](#vim)
    - [coc.nvim](#cocnvim)
    - [ALE](#ale)
    - [LanguageClient](#languageclient)
  - [Kakoune](#kakoune)
    - [kak-lsp](#kak-lsp)
  - [Emacs](#emacs)
    - [Emacs Doom](#emacs-doom)
  - [Sublime](#sublime)
- [Awesome libraries this is based on](#awesome-libraries-this-is-based-on)
- [Contributing](#contributing)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Installation

TODO: create VS Code extension to run this language server

The server can be installed via `npm` (or from source).

```sh
npm install -g @bryall/beancount-langserver
```

Then, you should be able to run the language server with the following command:

```sh
beancount-langserver
```

Follow the instructions below to integrate the language server into your editor.

### Alternative: Compile and install from source

First, clone this repo and compile it. `npm link` will add `beancount-langserver` to the `PATH`.

```sh
git clone git@github.com:bryall/beancount-langserver.git
cd beancount-langserver
npm install
npm run compile
npm link
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

| Feature          | Description                                                                                                                                          |
| ---------------- | ----------------------------------------------------------|
| diagnostics      | Provided via `beancoubt`                                  |
| formatting       | Should generate edits silimar to `bean-format`            |
| completions      | Show completions for Payees, Accounts, Date               |
| definitions      | Planned for future release                                |
| folding          | Planned for future release                                |
| hover            | Planned for future release                                |
| rename           | Planned for future release                                |

## Server Settings

This server contributes the following settings:

Settings may need a restart to be applied.

## Editor Support

### VS Code

Plan to make a VS Code extesion in the future

### Vim

Tested and Developed on Neovim v0.5 (master branch)

SETUP TODO

### Emacs

TODO

## Contributing

Please do :)
