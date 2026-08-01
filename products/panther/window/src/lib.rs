// @file products/panther/window/src/lib.rs
// @description Library root for the Panther windowing seam and window integration.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther windowing seam and window integration.
//!
//! This product crate opens native windows with `winit` and presents frames
//! through the Panther graphics interface. It owns the `winit` and backend
//! dependencies; `purr-graphics` names none of them. Only the neutral window
//! handle crosses the seam, through `WindowSurface`.
//!
//! At M0 the crate presents one fixed demonstration frame and handles resize and
//! close. The detailed event loop, input routing, resize policy, and multi-window
//! management belong to the later windowing work.

#[path = "active-backend.rs"]
mod active_backend;
#[path = "window-error.rs"]
mod window_error;
#[path = "window-loop.rs"]
mod window_loop;

pub use window_error::WindowError;
pub use window_loop::run_window;
