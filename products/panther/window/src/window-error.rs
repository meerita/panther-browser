// @file products/panther/window/src/window-error.rs
// @description Defines the typed failures the windowing layer can return.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Windowing layer error type.
//!
//! Each variant translates a lower-layer failure into the vocabulary of the
//! windowing layer. The `winit` sources are preserved for diagnostics. The
//! backend source is the Panther-owned interface error, never a raw backend or
//! native-API type.

use purr_graphics::GraphicsError;
use winit::error::{EventLoopError, OsError};

/// Failure the windowing layer reports to the application binary.
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    #[error("failed to build the window event loop")]
    EventLoop(#[source] EventLoopError),
    #[error("failed to create the window")]
    Window(#[source] OsError),
    #[error("the window did not expose a usable handle")]
    WindowHandleUnavailable,
    #[error("the window reported an invalid surface size")]
    InvalidSurfaceExtent,
    #[error("the graphics backend failed")]
    Backend(#[source] GraphicsError),
}
