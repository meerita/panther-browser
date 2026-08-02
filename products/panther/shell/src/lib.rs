// @file products/panther/shell/src/lib.rs
// @description Library root for the Panther minimal shell chrome model.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther minimal shell chrome model.
//!
//! This product crate owns the shell chrome as a set of laid-out colored
//! regions. It maps a window extent to a rectangle for each region and exposes
//! the placeholder colors that paint them. It uses `purr-graphics` for neutral
//! geometry and color types and `capability-system` for the read-only startup
//! reports; it names no `winit` type and no graphics backend type, so the shell
//! logic stays free of the windowing seam.
//!
//! At M1 the chrome is colored rectangles with no text (D1). The pointer
//! hit-test resolves a pointer position to a region, the interaction router
//! turns pointer and key input into hover and focus state, and the draw builder
//! turns that state into an ordered `Clear` and `FillRect` command list. Startup
//! reports the effective state of the foundational capabilities as canonical
//! diagnostics (D6), since M1 has no in-window text primitive to show them.

#[path = "capability-report.rs"]
mod capability_report;
#[path = "draw-command-builder.rs"]
mod draw_command_builder;
#[path = "pointer-hit-test.rs"]
mod pointer_hit_test;
#[path = "region-layout.rs"]
mod region_layout;
#[path = "shell.rs"]
mod shell;
#[path = "shell-region.rs"]
mod shell_region;

pub use capability_report::report_startup_capabilities;
pub use draw_command_builder::build_commands;
pub use pointer_hit_test::{PointerPosition, hit_test};
pub use region_layout::{RegionLayout, layout};
pub use shell::{KeyInput, Shell};
pub use shell_region::{CLEAR_COLOR, ShellRegion};
