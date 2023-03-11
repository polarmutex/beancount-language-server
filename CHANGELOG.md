## Unreleased

### Feat

- impl progress

### Fix

- beancount data not being updated for current file
- make journal path optional
- nix flake checks to pass
- token on pr-lint

### Refactor

- switch to lsp-server
- simplify logging
- move to multiple crate repo
- document
- beancount_data
- remove logger
- error
- remove Arc and logging
- move to multiple crate repo

## v1.2.5 (2022-06-21)

### Fix

- release workflow issues

## beancount-language-server-v1.2.4 (2022-06-21)

### Fix

- release workflow issues

## beancount-language-server-v1.2.3 (2022-06-21)

### Fix

- release workflow errors

## beancount-language-server-v1.2.2 (2022-06-21)

### Fix

- vsce publish error
- windows github workflow error on release

## beancount-language-server-v1.2.1 (2022-06-20)

### Fix

- github workflow errors on release

## beancount-language-server-v1.2.0 (2022-06-20)

### Feat

- support glob in include statements
- reboot vscode ext
- initial work on vscode ext

### Fix

- typo in formatting log

## beancount-language-server-v1.1.2 (2022-06-19)

### Fix

- link README and CHANGELOG to vscode ext
- release-please manifest
- make release artifacts uncompressed
- vscode version sync

## v1.1.1 (2022-05-02)

### Fix

- release-please error

## v1.1.0 (2022-05-02)

### Feat

- switch to tower-lsp (lspower archived)

### Fix

- github release binaries hopefully
- update nix flake to build lsp

## v1.0.2 (2022-04-28)

### Fix

- fixes #53 only log with specified as an option
- fixes #143 - add stdio option to keep options silimar to typescript
- cargo deny errors
- clippy warnings
- formatting errors
- cargo doc warnings
- rust compiler warnings

## v1.0.1 (2022-01-21)

### Fix

- activate document formatting by default

## v1.0.0 (2021-11-12)

### Feat

- added flag entries to diagnostics
- support diagnostics for flagged entries
- initial vs code ext from release
- add initial set of rust ci
- formatting
- txn_string completion
- account completion
- completion of date
- add on save
- completion framework
- added bean-check diagnostics
- successfully calling bean-check
- editing tree on save done
- import recursion on load to populate forest
- add ability to change python path to lsp
- initial README
- basic doc formatting is good shape
- basic doc formatting test done
- add start of formatting tests
- updated tree-sitter wasm to v2
- addded initial basic completions
- added Data completion
- switch to injection
- switching to injection
- reorg, added TS parsing on launch
- initial lsp tests, impl didOpen
- initialize tests
- restructure add lerna
- tree-sitter parse on open files
- add start of document formatting
- add diagnostics from bean-check
- add ability to call bean-check
- added warning for flagged entries

### Fix

- invalid date error
- some clippy warnings
- do ci only on PR
- completion node handling
- Nil compare for diagnostics
- txn_string completion
- diagnostics not being cleared when going to no diagnostics
- ext before testing
- github funding
- tree-sitter v0.20 fix
