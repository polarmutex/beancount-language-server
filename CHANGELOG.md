# Changelog

All notable changes to this project will be documented in this file.

## [1.4.0] - 2025-07-21

### Bug Fixes

- Do not insert ending quote if already present
- Errors with file paths on windows
- Switch default logging to info
- Improve formatting logic and added tests
- Refactor and improve to be closer to bean-format
- Typing capital should show all under Assets, Liabl, etc
- Handle bean-check global errors without line numbers
- Fixes #639 handles include files with no journal root
- Already present end quote on completions
- Improve logging
- Forest generator processing
- Account completion oddness

### Documentation

- Add claude.md file
- Update readme

### Features

- Impl rename and references
- Add aarch64-darwin to flake
- Support relative path in journal_root
- Improved completions supporting colons and fixing upper case
- Make formatting behave like bean-format
- Add formatting option to control number currency spacing
- Add formatting optional option to normalize indents
- Switch to nucleo-matcher for fuzzy searching completions

### Refactor

- To put logic in the providers folder
- Completion with some new completions added
- Diagnostics

## [1.3.7] - 2025-01-26

### Bug Fixes

- Make enum to not be hidden in blink-cmp

### Features

- Add version command to cli

## [1.3.6] - 2024-10-27

### Bug Fixes

- Update req_queue call to match API change
- Don't panic on invalid JSON inputs to Config

## [1.3.5] - 2024-08-05

### Bug Fixes

- Change message prefix from mun to beancount
- Clippy

### Documentation

- Document installation with Homebrew
- Update docs for Helix 23.10 and later

## [1.3.4] - 2024-01-16

### Bug Fixes

- Clippy warnings/errors
- Read the log cmd flag
- Upgrade to latest tree-sitter-beancount

### Features

- Improve, simplify, and test completion
- Tag and link completion

### Refactor

- Initial date completion tests
- Completion handler to handle params in handler

### Testing

- Add tests for date completion logic

## [1.3.2] - 2023-10-08

### Bug Fixes

- Handle tilde in journal_file
- Add some debug to journal file load to hopefully find user issues

### Documentation

- Add docs for use with Helix

## [1.3.1] - 2023-03-31

### Bug Fixes

- Cli not handling stdio option correctly

## [1.3.0] - 2023-03-18

### Bug Fixes

- Token on pr-lint
- Nix flake checks to pass
- Make journal path optional
- Beancount data not being updated for current file
- Clippy errors

### Documentation

- Fix run command in README

### Features

- Impl progress

### Refactor

- Move to multiple crate repo
- Remove Arc and logging
- Error
- Remove logger
- Beancount_data
- Document
- Move to multiple crate repo
- Simplify logging
- Switch to lsp-server

## [1.2.5] - 2022-06-21

### Bug Fixes

- Release workflow issues

## [beancount-language-server-v1.2.4] - 2022-06-21

### Bug Fixes

- Release workflow issues

## [beancount-language-server-v1.2.3] - 2022-06-21

### Bug Fixes

- Release workflow errors

## [beancount-language-server-v1.2.2] - 2022-06-21

### Bug Fixes

- Windows github workflow error on release
- Vsce publish error

## [beancount-language-server-v1.2.1] - 2022-06-21

### Bug Fixes

- Github workflow errors on release

## [beancount-language-server-v1.2.0] - 2022-06-21

### Bug Fixes

- Typo in formatting log

### Features

- Initial work on vscode ext
- Reboot vscode ext
- Support glob in include statements

## [beancount-language-server-v1.1.2] - 2022-06-19

### Bug Fixes

- Vscode version sync
- Make release artifacts uncompressed
- Release-please manifest
- Link README and CHANGELOG to vscode ext

## [1.1.1] - 2022-05-02

### Bug Fixes

- Release-please error

## [1.1.0] - 2022-05-02

### Bug Fixes

- Update nix flake to build lsp
- Github release binaries hopefully

### Documentation

- Fix redme typo

### Features

- Switch to tower-lsp (lspower archived)

## [1.0.2] - 2022-04-28

### Bug Fixes

- Rust compiler warnings
- Cargo doc warnings
- Formatting errors
- Clippy warnings
- Cargo deny errors
- Fixes #143 - add stdio option to keep options silimar to typescript
- Fixes #53 only log with specified as an option

### Documentation

- Fix to have old changes
- Update README to rust version

## [1.0.1] - 2022-01-21

### Bug Fixes

- Activate document formatting by default

## [1.0.0] - 2021-11-12

### Bug Fixes

- Tree-sitter v0.20 fix
- Github funding
- Ext before testing
- Diagnostics not being cleared when going to no diagnostics
- Txn_string completion
- Nil compare for diagnostics
- Completion node handling
- Do ci only on PR
- Some clippy warnings
- Invalid date error

### Documentation

- Update TOC

### Features

- Added warning for flagged entries
- Add ability to call bean-check
- Add diagnostics from bean-check
- Add start of document formatting
- Tree-sitter parse on open files
- Restructure add lerna
- Initialize tests
- Initial lsp tests, impl didOpen
- Reorg, added TS parsing on launch
- Switching to injection
- Switch to injection
- Added Data completion
- Addded initial basic completions
- Updated tree-sitter wasm to v2
- Base version of completion provider
- Add start of formatting tests
- Basic doc formatting test done
- Basic doc formatting is good shape
- Initial README
- Add ability to change python path to lsp
- Import recursion on load to populate forest
- Editing tree on save done
- Successfully calling bean-check
- Added bean-check diagnostics
- Completion framework
- Add on save
- Completion of date
- Account completion
- Txn_string completion
- Formatting
- Add initial set of rust ci
- Initial vs code ext from release
- Support diagnostics for flagged entries
- Added flag entries to diagnostics

### Bug

- Fix uri
- Bug fixes to get working
- Fixed tree not updating properly on content changes
- Fixed the server init not parsing the journal file
- Tweak completions
- Fix README
- Allow unknown options for node inspect
- Prevent resolve errors

### Build

- Bump node-notifier from 8.0.0 to 8.0.1

### Remove

- Old typescript stuff

<!-- generated by git-cliff -->
