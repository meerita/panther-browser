<div align="center">

# Panther

**A native, GPU-first web browser for the desktop.**

Panther is a graphical browser built on the Purr engine. GPU-first rendering,
written in Rust, one native application on Linux, macOS, and Windows.

[![CI](https://github.com/meerita/panther-browser/actions/workflows/ci.yml/badge.svg)](https://github.com/meerita/panther-browser/actions/workflows/ci.yml)
[![Security audit](https://github.com/meerita/panther-browser/actions/workflows/security-audit.yml/badge.svg)](https://github.com/meerita/panther-browser/actions/workflows/security-audit.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-000000?logo=rust)](https://www.rust-lang.org)
[![GitHub stars](https://img.shields.io/github/stars/meerita/panther-browser?style=flat&logo=github)](https://github.com/meerita/panther-browser/stargazers)

</div>

```
HTML → DOM → Style → Layout → Display list → Scene → GPU
```

Panther is the browser product. Purr is the rendering engine that powers it.
Panther owns the browser experience (windows, tabs, profiles, settings, and
process orchestration). Purr owns the web platform (HTML, DOM, CSS, style
resolution, layout, and painting). A companion terminal browser, Puma, uses the
same engine.

## Why Panther

- **GPU-first rendering.** Purr drives the page through an incremental pipeline
  (tokenizer, parser, DOM, style, layout, display list, scene) into a GPU render
  graph. The design targets stable frame times, not one-time throughput.
- **One shared engine.** Panther and Puma render from the same semantic document
  model. The engine owns web-platform behavior once, and each product owns only
  its own interface.
- **Explicit architecture.** Panther and Purr keep separate ownership. A
  capability system declares, for every subsystem, who owns it, whether it is
  mandatory or optional, when it starts, what it depends on, and how it releases
  its resources.
- **Correct before fast.** The project treats correctness and clear ownership as
  the base, and applies optimization only where measurement justifies it.
- **Portable and native.** A single Rust application on Linux, macOS, and Windows,
  with no embedded browser engine and no scripting runtime.

## Status

Early development. Panther is at its foundation milestone (M0). The Rust workspace
is in place, the capability system is implemented and wired between Purr and
Panther, and the continuous integration gates run on every change. There is no
browser window, event loop, or rendering path yet.

The application binary runs the capability bootstrap at startup and prints one
diagnostics line derived from the returned reports:

```
$ cargo run
capability bootstrap: N of M capabilities available
```

The interfaces, features, and architecture in this document can change without
notice.

### What works today

- **Capability foundation.** Stable capability identifiers, Panther and Purr
  ownership, a capability catalogue, maturity levels, requested and effective
  states, dependency validation, policy precedence, lazy activation,
  deactivation, and a diagnostic reason for every state.
- **Workspace and gates.** A Cargo workspace with lockstep versioning, plus format,
  lint, type-check, test, and security-audit gates in continuous integration.

## Roadmap

None of the milestones below are complete. They describe where the project is
headed. Each Purr pipeline stage begins with the smallest coherent subset needed
to show a page, and later milestones deepen those stages instead of replacing
them.

| Milestone | Goal |
| --- | --- |
| **M0 Foundations** | Workspace, capability system, and the GPU, scheduler, and memory direction. |
| **M1 Minimal shell** | Open a native window, process events, paint browser chrome, and hold an empty content viewport. |
| **M2 First pixels** | Render one static local HTML document through a thin Purr pipeline into the viewport. |
| **M3 Product UI** | Grow the shell into the intended interface: toolbar, address bar, tabs, command palette, and dockable panels. |
| **M4 Real pages** | Load remote pages, handle resources and fonts, expand CSS, persist state, and render incrementally. |

Planned product surface includes a command palette, dockable panels, a
keyboard-first workflow, and a native, GPU-rendered interface.

## Install

Panther requires Rust stable. The toolchain is pinned in `rust-toolchain.toml`.

```bash
git clone https://github.com/meerita/panther-browser
cd panther-browser
cargo build --release
```

Cargo writes the binary to `target/release/panther`. Run it with `cargo run`, or
run the binary directly. At this milestone it prints the capability bootstrap
summary and exits.

## Development

The Makefile wraps the common workspace commands:

```bash
make fmt          # Format all sources
make fmt-check    # Check formatting without writing changes
make clippy       # Lint the workspace (strict, -D warnings)
make check        # Type-check the workspace
make test         # Run the test suite
```

Without `make`, run the equivalent cargo commands:

```bash
cargo fmt --all                                              # fmt
cargo fmt --all --check                                      # fmt-check
cargo clippy --workspace --all-targets --all-features -- -D warnings   # clippy
cargo check --workspace --all-targets                        # check
cargo test --workspace --all-targets                         # test
```

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the branch
model, commit style, quality gates, and how to open a pull request. All
participants must follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Read [SECURITY.md](SECURITY.md) to report a security issue.

## License

Panther is released under the [MIT License](LICENSE).
