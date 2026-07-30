# Contributing to Panther

Thank you for your interest in this project. This document explains how to
contribute.

## Project Status

Panther is in early development. The architecture and conventions can change
often at this stage. Check open issues and discussions before you start work,
so that your effort matches the current direction of the project.

## Before You Start

- Search open and closed issues before you open a new one.
- Open an issue to discuss a new feature or a large change before you start
  work on it.
- Keep each issue and each pull request focused on one topic.

## Reporting a Bug

Open an issue and include:

- A short, clear title.
- The steps needed to reproduce the problem.
- The result that you expected.
- The result that you observed.
- Your operating system and version.

## Proposing a Change

- Open an issue first for any change that affects the architecture, the
  public API, or the user-visible behavior.
- Small fixes, such as typo corrections or dead code removal, do not need an
  issue first.

## Development Setup

The project does not yet publish build instructions, because the codebase is
still in its initial phase. This section will list the required toolchain and
the build steps once the first components are available.

## Coding Guidelines

- Write idiomatic Rust.
- Keep dependencies to a minimum.
- Add benchmarks for performance-sensitive code.
- Document every `unsafe` block with its safety reasoning.
- Record a decision document for architectural changes.

## Commit Messages

Use the [Conventional Commits](https://www.conventionalcommits.org) format for
every commit message. Examples:

```text
feat: add tab bar to the browser window
fix: correct scroll offset on window resize
chore: update the Rust toolchain version
docs: add build instructions to the README
```

## Versioning and Releases

- The project follows [Semantic Versioning](https://semver.org).
- A release is a git tag in the form `vMAJOR.MINOR.PATCH`, published as a
  GitHub Release.
- Once the Rust workspace exists, the workspace `Cargo.toml` holds the
  current version as the single source of truth, and every crate inherits it.
- The project does not publish a release yet.

## Pull Requests

- Keep one pull request focused on one logical change.
- Describe the purpose of the change in the pull request description.
- Describe how you tested or validated the change.
- Link the related issue when one exists.
- Expect changes to go through review before merge.

## License

By contributing to this project, you agree that your contributions are
licensed under the project [LICENSE](LICENSE).
