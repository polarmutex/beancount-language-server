# Welcome to GitHub docs contributing guide <!-- omit in toc -->

Thank you for investing your time in contributing to our project!

In this guide you will get an overview of the contribution workflow from opening
an issue, creating a PR, reviewing, and merging the PR.

Use the table of contents icon on the top left corner of this document to get to
a specific section of this guide quickly.

## Getting started

### Issues

#### Create a new issue

### Make Changes

1. Fork the repository.

2. Create a working branch and start with your changes!

3. Run npm install in the root directory of the local repo to initialize husky
and commitlint

4. After making your changes, ensure the following:
    * cargo build runs successfully
    * cargo test runs successfully
    * formatted the code with cargo fmt --all --
    * linted VS code extension with npm run fix

### Commit your update

This repo follows the [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/#summary)
specification when writing commit messages. It's important any pull requests
submitted have commit messages which follow this standard.

### Pull Request

When you're finished with the changes, create a pull request(PR) against the
develop branch
