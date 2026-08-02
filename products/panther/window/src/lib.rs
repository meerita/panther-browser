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
//! The crate drives the `panther-shell` chrome through the window loop. It
//! forwards pointer and keyboard input to the shell, relays resize, and presents
//! the shell command list on demand. `winit` and backend types stay inside this
//! crate; only neutral input values and the neutral command list cross to the
//! shell. Multi-window management belongs to the later windowing work.

#[path = "active-backend.rs"]
mod active_backend;
#[path = "window-error.rs"]
mod window_error;
#[path = "window-loop.rs"]
mod window_loop;

pub use window_error::WindowError;
pub use window_loop::run_window;
