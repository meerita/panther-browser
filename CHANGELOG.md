# Changelog

This file records user-visible changes to Panther.

Each entry describes the engineering impact on a user or a contributor. Internal refactors, test
maintenance, and tooling changes have no entry.

The project follows Semantic Versioning. Before `1.0.0`, a breaking change increments the minor version
and a fix or an additive change increments the patch version.

## Unreleased

### Added

- Render a static local document through the Purr pipeline into the content viewport.
- Render an interactive tab strip driven by the tab model.
- Make the address bar functional.
- Render localized chrome text labels.
- Select a graphics backend at startup, preferring hardware acceleration and falling back to the deterministic software backend when no adapter is available.
- Open one native window and present a frame on Linux, macOS, and Windows.

### Fixed

- Colorize glyph text and composite premultiplied alpha correctly.
- Render chrome and content correctly on high-density displays.

### Known limitations

- There is no scripting runtime. Panther executes no JavaScript.
- Only a static local document renders. There is no navigation to a remote document.
- Incremental invalidation is not implemented. Each frame rebuilds its display list.
- Text layout supports left-to-right horizontal text with one font family per run. There is no bidirectional reordering and no font fallback across scripts.
- The hardware backend is not verified on every supported platform.
