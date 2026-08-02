// @file products/panther/shell/src/lib.rs
// @description Library root for the Panther minimal shell chrome model.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther minimal shell chrome model.
//!
//! This product crate owns the shell chrome as a set of laid-out colored
//! regions. It maps a window extent to a rectangle for each region and exposes
//! the placeholder colors that paint them. It depends only on `purr-graphics`
//! for neutral geometry and color types; it names no `winit` type and no
//! graphics backend type, so the shell logic stays free of the windowing seam.
//!
//! At M1 the chrome is colored rectangles with no text (D1). The pointer
//! hit-test resolves a pointer position to a region; the interaction router and
//! the draw builder land in later phases.

#[path = "pointer-hit-test.rs"]
mod pointer_hit_test;
#[path = "region-layout.rs"]
mod region_layout;
#[path = "shell-region.rs"]
mod shell_region;

pub use pointer_hit_test::{PointerPosition, hit_test};
pub use region_layout::{RegionLayout, layout};
pub use shell_region::{CLEAR_COLOR, ShellRegion};
