// @file products/panther/window/src/window-loop.rs
// @description Runs the window event loop and presents the demonstration frame.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Window event loop and frame presentation.
//!
//! `run_window` opens one native window with `winit`, selects a backend, creates
//! a presentation target from the window handle, and presents a fixed
//! demonstration frame on redraw. It handles resize and close and returns the
//! first failure it meets.
//!
//! A native surface can report a transient state right after the window appears
//! (the drawable is outdated or the window is not yet visible). The backend maps
//! that state to `SubmissionRejected`. The loop recovers by reconfiguring the
//! surface and asking for another redraw, bounded by a deadline so a persistent
//! failure still surfaces as an error.
//!
//! Isolation: `winit` and backend types stay inside this crate. Only the neutral
//! window handle crosses the `purr-graphics` seam, through `WindowSurface`. The
//! window outlives the backend, so the handle the backend borrowed stays valid
//! across later presents.

use std::time::{Duration, Instant};

use purr_graphics::{
    AlphaMode, Color, DrawCommand, Extent2d, FrameSubmission, FrameToken, GraphicsError,
    MAX_TEXTURE_EXTENT, PresentationTargetDescriptor, Rect, SceneGeneration, SceneId,
    SceneIdentity, SurfaceIdentity, TextureFormatClass, WindowSurface,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::active_backend::{ActiveBackend, create_active_backend};
use crate::window_error::WindowError;

/// Title shown on the window.
const WINDOW_TITLE: &str = "Panther";

/// Initial logical window width, in logical pixels.
const INITIAL_WIDTH: f64 = 1024.0;

/// Initial logical window height, in logical pixels.
const INITIAL_HEIGHT: f64 = 768.0;

/// Background clear color of the demonstration frame.
const CLEAR_COLOR: Color = Color::new(0.09, 0.09, 0.11, 1.0);

/// Fill color of the demonstration rectangle.
const RECT_COLOR: Color = Color::new(0.20, 0.55, 0.95, 1.0);

/// Time a recoverable present failure may persist before it is treated as fatal.
///
/// A fresh surface can be transiently outdated or occluded for a few frames after
/// the window appears. Beyond this window the failure is no longer transient.
const PRESENT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of one redraw attempt.
enum FrameOutcome {
    /// The frame was presented. The flag reports the software backend, so the
    /// caller can note the headless software path once.
    Presented { software: bool },
    /// The present failed with a recoverable surface state. The surface was
    /// reconfigured and another redraw was requested.
    Recovered,
}

/// Opens a window and presents a demonstration frame through the selected backend.
///
/// The call blocks until the window closes. It returns the first initialization
/// or presentation failure, or `Ok(())` on a clean exit.
pub fn run_window() -> Result<(), WindowError> {
    let event_loop = EventLoop::new().map_err(WindowError::EventLoop)?;
    let mut application = WindowApplication::new();
    event_loop
        .run_app(&mut application)
        .map_err(WindowError::EventLoop)?;
    application.into_result()
}

/// One live window bound to a backend and a presentation target.
struct Presentation {
    window: Window,
    backend: ActiveBackend,
    surface: SurfaceIdentity,
    extent: Extent2d,
}

impl Presentation {
    /// Submits and presents the demonstration frame.
    ///
    /// A recoverable present failure reconfigures the surface and requests another
    /// redraw, so the caller retries on the next frame instead of failing.
    fn render(&mut self) -> Result<FrameOutcome, WindowError> {
        let submission = demonstration_frame(self.surface, self.extent);
        self.backend
            .submit(self.surface, &submission)
            .map_err(WindowError::Backend)?;

        match self.backend.present(self.surface) {
            Ok(()) => Ok(FrameOutcome::Presented {
                software: self.backend.is_software(),
            }),
            Err(GraphicsError::SubmissionRejected) => {
                self.reconfigure()?;
                self.window.request_redraw();
                Ok(FrameOutcome::Recovered)
            }
            Err(other) => Err(WindowError::Backend(other)),
        }
    }

    /// Resizes the presentation target to a new window size.
    ///
    /// A zero or over-bound size (for example a minimized window) is skipped, so
    /// the target keeps its last valid extent.
    fn resize(&mut self, size: PhysicalSize<u32>) -> Result<(), WindowError> {
        let Some(extent) = valid_extent(size) else {
            return Ok(());
        };

        self.backend
            .resize_presentation_target(self.surface, extent)
            .map_err(WindowError::Backend)?;
        self.extent = extent;
        self.window.request_redraw();
        Ok(())
    }

    /// Reconfigures the surface to the current window size.
    ///
    /// This clears a transient outdated surface before the next present. A zero or
    /// over-bound size is skipped.
    fn reconfigure(&mut self) -> Result<(), WindowError> {
        let Some(extent) = valid_extent(self.window.inner_size()) else {
            return Ok(());
        };

        self.backend
            .resize_presentation_target(self.surface, extent)
            .map_err(WindowError::Backend)?;
        self.extent = extent;
        Ok(())
    }
}

/// Application state driven by the `winit` event loop.
struct WindowApplication {
    presentation: Option<Presentation>,
    error: Option<WindowError>,
    software_frame_noted: bool,
    recovery_deadline: Option<Instant>,
}

impl WindowApplication {
    fn new() -> Self {
        Self {
            presentation: None,
            error: None,
            software_frame_noted: false,
            recovery_deadline: None,
        }
    }

    /// Returns the captured failure, if any.
    fn into_result(self) -> Result<(), WindowError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Builds the window, backend, and presentation target.
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowError> {
        let attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT));
        let window = event_loop
            .create_window(attributes)
            .map_err(WindowError::Window)?;

        ensure_handles(&window)?;

        let extent = valid_extent(window.inner_size()).ok_or(WindowError::InvalidSurfaceExtent)?;

        let mut backend = create_active_backend()?;
        let descriptor = PresentationTargetDescriptor {
            extent,
            format: TextureFormatClass::Bgra8Unorm,
            alpha_mode: AlphaMode::Opaque,
        };

        let surface = backend
            .create_presentation_target(WindowSurface::new(&window, extent), descriptor)
            .map_err(WindowError::Backend)?;

        window.request_redraw();

        self.presentation = Some(Presentation {
            window,
            backend,
            surface,
            extent,
        });
        Ok(())
    }

    /// Notes the headless software path once.
    fn note_software_frame(&mut self, software: bool) {
        if software && !self.software_frame_noted {
            self.software_frame_noted = true;
            eprintln!(
                "panther-window: software backend produced a framebuffer; on-screen display of the software path is later work"
            );
        }
    }

    /// Returns whether the recovery window has elapsed, starting it on first use.
    fn recovery_expired(&mut self, now: Instant) -> bool {
        let deadline = *self
            .recovery_deadline
            .get_or_insert(now + PRESENT_RECOVERY_TIMEOUT);
        now >= deadline
    }
}

impl ApplicationHandler for WindowApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.presentation.is_some() {
            return;
        }

        if let Err(error) = self.initialize(event_loop) {
            self.error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(presentation) = self.presentation.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Err(error) = presentation.resize(size) {
                    self.error = Some(error);
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => match presentation.render() {
                Ok(FrameOutcome::Presented { software }) => {
                    self.recovery_deadline = None;
                    self.note_software_frame(software);
                }
                Ok(FrameOutcome::Recovered) => {
                    if self.recovery_expired(Instant::now()) {
                        self.error = Some(WindowError::Backend(GraphicsError::SubmissionRejected));
                        event_loop.exit();
                    }
                }
                Err(error) => {
                    self.error = Some(error);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }
}

/// Builds the fixed demonstration frame for a target.
///
/// The frame clears to a background color and fills one centered rectangle. The
/// command set exercises the `Clear` and `FillRect` paths of both backends.
fn demonstration_frame(surface: SurfaceIdentity, extent: Extent2d) -> FrameSubmission {
    FrameSubmission {
        frame_token: FrameToken::new(1),
        scene: SceneIdentity::new(
            SceneId::new(1),
            SceneGeneration::new(1),
            surface.surface_id(),
            surface.surface_generation(),
        ),
        target: PresentationTargetDescriptor {
            extent,
            format: TextureFormatClass::Bgra8Unorm,
            alpha_mode: AlphaMode::Opaque,
        },
        uploads: Vec::new(),
        commands: vec![
            DrawCommand::Clear { color: CLEAR_COLOR },
            DrawCommand::FillRect {
                rect: centered_rect(extent),
                color: RECT_COLOR,
            },
        ],
    }
}

/// Returns a rectangle covering the center quarter of the target.
fn centered_rect(extent: Extent2d) -> Rect {
    let width = extent.width as f32;
    let height = extent.height as f32;
    let rect_width = width / 2.0;
    let rect_height = height / 2.0;

    Rect::new(
        (width - rect_width) / 2.0,
        (height - rect_height) / 2.0,
        rect_width,
        rect_height,
    )
}

/// Validates a window size into a nonzero, in-bounds surface extent.
///
/// Returns `None` for a zero dimension (a minimized window) or a dimension above
/// the M0 texture bound, so the caller skips or rejects it before any target is
/// created or resized.
fn valid_extent(size: PhysicalSize<u32>) -> Option<Extent2d> {
    if size.width == 0 || size.height == 0 {
        return None;
    }

    if size.width > MAX_TEXTURE_EXTENT || size.height > MAX_TEXTURE_EXTENT {
        return None;
    }

    Some(Extent2d::new(size.width, size.height))
}

/// Confirms the window exposes both neutral handles before a target is built.
///
/// The check fails closed with a clear error, so a window without a usable handle
/// does not reach the backend as a generic unsupported failure.
fn ensure_handles(window: &Window) -> Result<(), WindowError> {
    window
        .window_handle()
        .map_err(|_| WindowError::WindowHandleUnavailable)?;
    window
        .display_handle()
        .map_err(|_| WindowError::WindowHandleUnavailable)?;
    Ok(())
}
