<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->
**Table of Contents**

- [vscode-beancount-langserver](#vscode-beancount-langserver)
  - [Requirements](#requirements)
  - [Configuration](#configuration)
  - [Features](#features)
  - [Contributing](#contributing)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

# vscode-beancount-langserver
A VS Code Extension for the benacount language server

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->
**Table of Contents**

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Requirements

You will need to install `beancount` to get all diagnostics.

```sh
pip install -g beancount
```

## Configuration

 - journalFile: Path to main journal file
 - pythonPath: Path to python executable that has beancount installed

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

## Contributing

Please do :)
