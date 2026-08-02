// @file products/panther/window/src/window-loop.rs
// @description Runs the window event loop and presents the shell-driven frame.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Window event loop and frame presentation.
//!
//! `run_window` opens one native window with `winit`, selects a backend, creates
//! a presentation target from the window handle, and drives the `panther-shell`
//! chrome. It forwards pointer and keyboard input to the shell, relays resize,
//! and presents the shell command list on redraw. A redraw is requested only when
//! the shell state changes (D5). It handles close and returns the first failure it
//! meets.
//!
//! A native surface can report a transient state right after the window appears
//! (the drawable is outdated or the window is not yet visible). The backend maps
//! that state to `SubmissionRejected`. The loop recovers by reconfiguring the
//! surface and asking for another redraw, bounded by a deadline so a persistent
//! failure still surfaces as an error.
//!
//! Isolation: `winit` and backend types stay inside this crate. Only the neutral
//! window handle crosses the `purr-graphics` seam, through `WindowSurface`, and
//! only neutral input values and the neutral command list cross to the shell. The
//! window outlives the backend, so the handle the backend borrowed stays valid
//! across later presents.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::time::{Duration, Instant};

use panther_shell::{KeyInput, PointerPosition, Shell, ShellRegion};
use purr_embedding::{
    DocumentFrame, DocumentHandle, DocumentSession, ViewportGeometry, m2_demonstration_fixture,
};
use purr_graphics::{
    AlphaMode, DrawCommand, Extent2d, FrameSubmission, FrameToken, GpuResourceIdentity,
    GraphicsError, MAX_TEXTURE_EXTENT, PresentationTargetDescriptor, Rect, SceneGeneration,
    SceneId, SceneIdentity, SurfaceIdentity, TextureDescriptor, TextureFormatClass,
    WindowDisplayHandle, WindowSurface,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use crate::active_backend::{ActiveBackend, create_active_backend};
use crate::compositor::{CompositedDocument, composite_document, merge_submission};
use crate::window_error::WindowError;

/// Title shown on the window.
const WINDOW_TITLE: &str = "Panther";

/// Initial logical window width, in logical pixels.
const INITIAL_WIDTH: f64 = 1024.0;

/// Initial logical window height, in logical pixels.
const INITIAL_HEIGHT: f64 = 768.0;

/// Time a recoverable present failure may persist before it is treated as fatal.
///
/// A fresh surface can be transiently outdated or occluded for a few frames after
/// the window appears. Beyond this window the failure is no longer transient.
const PRESENT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of one redraw attempt.
enum FrameOutcome {
    /// The frame was submitted and presented.
    Presented,
    /// The present failed with a recoverable surface state. The surface was
    /// reconfigured and another redraw was requested.
    Recovered,
}

/// Opens a window and drives the shell chrome through the selected backend.
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

/// One live window bound to a backend, a presentation target, and the shell.
///
/// The window is shared through an `Rc` so the software backend can retain a
/// clone of the neutral handle for its on-screen present while the loop keeps its
/// own reference for redraw and resize.
///
/// It also drives the document-attachment seam. It owns the engine session, the
/// handle to the attached document, and the last produced frame. The frame is
/// produced synchronously on initialize and on resize only (D6); the compositor
/// offsets and clips it into the viewport on each redraw. The frame is absent
/// until the first successful produce, which never happens for a degenerate
/// (zero-area) viewport.
struct Presentation {
    window: Rc<Window>,
    backend: ActiveBackend,
    surface: SurfaceIdentity,
    extent: Extent2d,
    shell: Shell,
    last_cursor: Option<PointerPosition>,
    session: DocumentSession,
    document: DocumentHandle,
    document_frame: Option<DocumentFrame>,
    textures: Vec<(TextureDescriptor, GpuResourceIdentity)>,
}

impl Presentation {
    /// Submits and presents the current shell frame.
    ///
    /// A successful present clears the shell dirty flag, so the next redraw runs
    /// only after another state change (D5). A recoverable present failure
    /// reconfigures the surface and requests another redraw, so the caller retries
    /// on the next frame instead of failing.
    fn render(&mut self) -> Result<FrameOutcome, WindowError> {
        let content = match self.composite_content() {
            Some(content) => Some(self.realize_content(content)?),
            None => None,
        };
        let submission = shell_frame(&self.shell, self.surface, self.extent, content);
        self.backend
            .submit(self.surface, &submission)
            .map_err(WindowError::Backend)?;

        match self.backend.present(self.surface) {
            Ok(()) => {
                self.shell.clear_dirty();
                Ok(FrameOutcome::Presented)
            }
            Err(GraphicsError::SubmissionRejected) => {
                self.reconfigure()?;
                self.window.request_redraw();
                Ok(FrameOutcome::Recovered)
            }
            Err(other) => Err(WindowError::Backend(other)),
        }
    }

    /// Resizes the presentation target and relays the extent to the shell.
    ///
    /// A zero or over-bound size (for example a minimized window) is skipped, so
    /// the target and the layout keep their last valid extent.
    fn resize(&mut self, size: PhysicalSize<u32>) -> Result<(), WindowError> {
        let Some(extent) = valid_extent(size) else {
            return Ok(());
        };

        self.backend
            .resize_presentation_target(self.surface, extent)
            .map_err(WindowError::Backend)?;
        self.extent = extent;
        self.shell.resize(extent);
        self.produce_document()?;
        self.window.request_redraw();
        Ok(())
    }

    /// Produces the document frame for the current viewport geometry (D6).
    ///
    /// The engine lays out in document-local space; only the content extent and
    /// device pixel ratio cross the seam (D5). A degenerate (zero-area) viewport
    /// produces no frame and keeps the last one, so a minimized window does not
    /// discard a valid render. Runs on initialize and on resize only, never per
    /// continuous frame (S1).
    fn produce_document(&mut self) -> Result<(), WindowError> {
        let Some(geometry) = viewport_geometry(self.viewport_rect()) else {
            return Ok(());
        };

        let frame = self
            .session
            .produce(&self.document, geometry)
            .map_err(WindowError::Document)?;
        self.document_frame = Some(frame);
        Ok(())
    }

    /// Offsets and clips the last document frame into the viewport (D5).
    ///
    /// Returns `None` when no frame has been produced or the stored frame names a
    /// superseded generation, so a stale frame never paints (S5).
    fn composite_content(&self) -> Option<CompositedDocument> {
        let frame = self.document_frame.as_ref()?;
        composite_document(frame, self.viewport_rect(), self.document.generation())
    }

    /// Realizes the document uploads on the backend and remaps their identities.
    ///
    /// The seam hands back a document-local display list, not a texture (D1). Each
    /// engine upload names a synthetic resource identity in the engine namespace,
    /// but the backend draws only from a texture it allocated. This allocates one
    /// texture per upload descriptor, rewrites the upload to the allocated
    /// identity, and rewrites every glyph quad that referenced the engine identity,
    /// so the submission names only live backend resources. Allocation is cached by
    /// descriptor: the glyph atlas is stable across renders and resizes, so the M2
    /// fixture allocates one atlas texture for the life of the window.
    fn realize_content(
        &mut self,
        content: CompositedDocument,
    ) -> Result<CompositedDocument, WindowError> {
        let CompositedDocument {
            mut commands,
            uploads,
        } = content;

        let mut realized = Vec::with_capacity(uploads.len());
        for mut upload in uploads {
            let engine_resource = upload.resource;
            let backend_resource = self.ensure_texture(&upload.descriptor)?;
            remap_texture(&mut commands, engine_resource, backend_resource);
            upload.resource = backend_resource;
            realized.push(upload);
        }

        Ok(CompositedDocument {
            commands,
            uploads: realized,
        })
    }

    /// Returns a live backend texture for the descriptor, allocating on first use.
    ///
    /// A cached texture with an equal descriptor is reused, so a repeated render or
    /// a resize does not allocate again. The backend exposes no free, so the cache
    /// is the allocation bound: it grows only when a genuinely new descriptor
    /// appears, which the stable M2 atlas never does.
    fn ensure_texture(
        &mut self,
        descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, WindowError> {
        if let Some((_, resource)) = self
            .textures
            .iter()
            .find(|(cached, _)| cached == descriptor)
        {
            return Ok(*resource);
        }

        let resource = self
            .backend
            .allocate_texture(descriptor)
            .map_err(WindowError::Backend)?;
        self.textures.push((descriptor.clone(), resource));
        Ok(resource)
    }

    /// Current viewport rectangle in surface pixel space.
    fn viewport_rect(&self) -> Rect {
        self.shell.layout().rect(ShellRegion::Viewport)
    }

    /// Forwards a pointer move to the shell and redraws only on a state change.
    fn pointer_moved(&mut self, position: PhysicalPosition<f64>) {
        let position = to_pointer_position(position);
        self.last_cursor = Some(position);
        self.shell.pointer_moved(position);
        self.request_redraw_if_dirty();
    }

    /// Forwards a pointer press at the last known position and redraws on change.
    ///
    /// A press with no prior cursor position has no location to hit-test, so it is
    /// ignored.
    fn pointer_pressed(&mut self) {
        let Some(position) = self.last_cursor else {
            return;
        };

        self.shell.pointer_pressed(position);
        self.request_redraw_if_dirty();
    }

    /// Forwards a key to the focused region. It is a no-op at M1 and never
    /// requests a redraw.
    fn key_pressed(&self, key: KeyInput) {
        let _ = self.shell.deliver_key(key);
    }

    /// Requests a redraw only when the shell changed since the last paint (D5).
    fn request_redraw_if_dirty(&self) {
        if self.shell.is_dirty() {
            self.window.request_redraw();
        }
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
    recovery_deadline: Option<Instant>,
}

impl WindowApplication {
    fn new() -> Self {
        Self {
            presentation: None,
            error: None,
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
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(WindowError::Window)?,
        );

        ensure_handles(&window)?;

        let extent = valid_extent(window.inner_size()).ok_or(WindowError::InvalidSurfaceExtent)?;

        let mut backend = create_active_backend()?;
        let descriptor = PresentationTargetDescriptor {
            extent,
            format: TextureFormatClass::Bgra8Unorm,
            alpha_mode: AlphaMode::Opaque,
        };

        let handle: Rc<dyn WindowDisplayHandle> = window.clone();
        let surface = backend
            .create_presentation_target(WindowSurface::new_owned(handle, extent), descriptor)
            .map_err(WindowError::Backend)?;

        window.request_redraw();

        let mut session = DocumentSession::new();
        let document = session
            .attach(m2_demonstration_fixture())
            .map_err(WindowError::Document)?;

        let mut presentation = Presentation {
            window,
            backend,
            surface,
            extent,
            shell: Shell::new(extent),
            last_cursor: None,
            session,
            document,
            document_frame: None,
            textures: Vec::new(),
        };
        presentation.produce_document()?;

        self.presentation = Some(presentation);
        Ok(())
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
            WindowEvent::CursorMoved { position, .. } => {
                presentation.pointer_moved(position);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => {
                presentation.pointer_pressed();
            }
            WindowEvent::KeyboardInput { event: key, .. } if key.state == ElementState::Pressed => {
                presentation.key_pressed(to_key_input(key.physical_key));
            }
            WindowEvent::RedrawRequested => match presentation.render() {
                Ok(FrameOutcome::Presented) => {
                    self.recovery_deadline = None;
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

/// Repoints every glyph quad from the engine resource identity to the backend one.
///
/// The paint stage tags each glyph quad with the engine-namespace atlas identity;
/// once the atlas is allocated on the backend, those quads must sample the
/// allocated texture instead. A quad that names a different resource is left
/// unchanged, so several uploads remap independently.
fn remap_texture(commands: &mut [DrawCommand], from: GpuResourceIdentity, to: GpuResourceIdentity) {
    for command in commands.iter_mut() {
        if let DrawCommand::TexturedQuad { texture, .. } = command
            && *texture == from
        {
            *texture = to;
        }
    }
}

/// Merges the shell chrome and the composited document into one submission.
///
/// The shell builds the ordered `Clear` and `FillRect` list for its current
/// chrome state (D1); the compositor offsets and clips the document into the
/// viewport (D5). This function merges both into one submission for the owned
/// surface and the current extent. The document paints over the chrome viewport
/// fill, so it is visible without removing the chrome region.
fn shell_frame(
    shell: &Shell,
    surface: SurfaceIdentity,
    extent: Extent2d,
    content: Option<CompositedDocument>,
) -> FrameSubmission {
    let scene = SceneIdentity::new(
        SceneId::new(1),
        SceneGeneration::new(1),
        surface.surface_id(),
        surface.surface_generation(),
    );
    let target = PresentationTargetDescriptor {
        extent,
        format: TextureFormatClass::Bgra8Unorm,
        alpha_mode: AlphaMode::Opaque,
    };

    merge_submission(
        shell.build_commands(),
        content,
        FrameToken::new(1),
        scene,
        target,
    )
}

/// Derives the viewport geometry for one render from the viewport rectangle.
///
/// The content extent is the rounded viewport size in surface pixels; the device
/// pixel ratio is 1.0 on the M2 path. A rectangle that rounds to a zero width or
/// height has no content box and yields `None`, so the caller keeps the last
/// produced frame instead of producing an empty one.
fn viewport_geometry(viewport: Rect) -> Option<ViewportGeometry> {
    let width = viewport.width.round();
    let height = viewport.height.round();

    if width < 1.0 || height < 1.0 {
        return None;
    }

    Some(ViewportGeometry {
        content_extent: Extent2d::new(width as u32, height as u32),
        device_pixel_ratio: 1.0,
    })
}

/// Converts a native pointer position into the neutral shell pointer position.
///
/// The shell works in surface pixel space and names no windowing type, so the
/// native `f64` position is narrowed to the shell `f32` value at this seam.
fn to_pointer_position(position: PhysicalPosition<f64>) -> PointerPosition {
    PointerPosition::new(position.x as f32, position.y as f32)
}

/// Converts a native physical key into the neutral shell key value.
///
/// The shell carries a key as an uninterpreted `u32` (D3). The physical key is a
/// stable identity for a key position, so its hash gives a neutral value the shell
/// forwards without interpreting it at M1.
fn to_key_input(physical_key: PhysicalKey) -> KeyInput {
    let mut hasher = DefaultHasher::new();
    physical_key.hash(&mut hasher);
    KeyInput::new(hasher.finish() as u32)
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
