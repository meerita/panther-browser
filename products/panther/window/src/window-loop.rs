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
//! The window is a presenter of the injected product core. It holds the
//! [`TabModel`] built by the composition root, produces the active tab's frame
//! through it, and composites that frame into the shell viewport. The window owns
//! no engine session and no document handle; the product core owns them.
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

use std::rc::Rc;
use std::time::{Duration, Instant};

use panther_browser::TabModel;
use panther_chrome_text::{ChromeRefresh, ChromeText};
use panther_shell::{
    KeyInput, LabelView, PointerPosition, ScaleFactor, Shell, ShellAction, ShellRegion,
    TabStripView,
};
use purr_embedding::{DocumentFrame, ViewportGeometry};
use purr_graphics::{
    AlphaMode, DrawCommand, Extent2d, FrameSubmission, FrameToken, GpuResourceIdentity,
    GraphicsError, MAX_TEXTURE_EXTENT, PresentationTargetDescriptor, Rect, ResourceUpload,
    SceneGeneration, SceneId, SceneIdentity, SurfaceIdentity, TextureDescriptor,
    TextureFormatClass, WindowDisplayHandle, WindowSurface,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
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

/// Upper bound on open tabs, enforced before a new tab is created.
///
/// The window is the sole tab creator and a pointer press is the only path to
/// `open_tab`, so bounding tab creation here keeps the resource bounded at its
/// creation point (security: bounded resources, D8).
const MAX_TABS: usize = 8;

/// Result of one redraw attempt.
enum FrameOutcome {
    /// The frame was submitted and presented.
    Presented,
    /// The present failed with a recoverable surface state. The surface was
    /// reconfigured and another redraw was requested.
    Recovered,
}

/// The shell chrome ready to submit: its draw commands and the realized chrome
/// atlas upload.
///
/// The commands are the shell chrome list with the chrome text quads already
/// remapped to the backend atlas identity. The upload carries the chrome atlas
/// pixels under that same identity, so the merge names only a live backend
/// resource.
struct RealizedChrome {
    commands: Vec<DrawCommand>,
    uploads: Vec<ResourceUpload>,
}

/// Opens a window and drives the shell chrome through the selected backend.
///
/// The composition root injects both the product core and the chrome text
/// producer: the window presents the core and realizes the producer's atlas, but
/// it owns no locale policy. The call blocks until the window closes. It returns
/// the first initialization or presentation failure, or `Ok(())` on a clean exit.
pub fn run_window(tab_model: TabModel, chrome_text: ChromeText) -> Result<(), WindowError> {
    let event_loop = EventLoop::new().map_err(WindowError::EventLoop)?;
    let mut application = WindowApplication::new(tab_model, chrome_text);
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
/// It presents the injected product core. It holds the [`TabModel`] and the last
/// frame produced for the active tab. The frame is produced synchronously on
/// initialize, on resize, and on a tab action only (D6, D9); the compositor
/// offsets and clips it into the viewport on each redraw. The frame is absent
/// until the first successful produce, which never happens for a degenerate
/// (zero-area) viewport or an empty active tab.
///
/// The window pushes a neutral tab-strip view to the shell whenever the tab state
/// changes, and it is the sole translator from a neutral [`ShellAction`] to a
/// [`TabModel`] operation. It maps a slot index to a `TabId` and enforces the
/// `MAX_TABS` bound; the shell never names a tab identity (D3).
///
/// The window also holds the injected chrome text producer. It pushes the
/// producer's placed-run view to the shell and realizes the chrome atlas into a
/// backend texture, but it owns no locale policy: the producer encapsulates the
/// catalog and the active locale, so the window never names the localization
/// crate. The chrome atlas realizes through the same identity-keyed texture cache
/// as the document atlas, so their distinct identities never alias.
///
/// The window holds the display scale factor. It keeps `extent` in physical
/// surface pixels for the backend and the NDC math, and hands the shell a logical
/// extent, so the chrome lays out in logical pixels and paints at physical
/// resolution. The document renders at physical resolution with the scale as its
/// device pixel ratio (D4). The scale is a render-side detail only; it never
/// reaches web content (I4).
struct Presentation {
    window: Rc<Window>,
    backend: ActiveBackend,
    surface: SurfaceIdentity,
    extent: Extent2d,
    scale: ScaleFactor,
    shell: Shell,
    last_cursor: Option<PointerPosition>,
    tab_model: TabModel,
    chrome_text: ChromeText,
    document_frame: Option<DocumentFrame>,
    textures: Vec<(GpuResourceIdentity, GpuResourceIdentity)>,
}

impl Presentation {
    /// Submits and presents the current shell frame.
    ///
    /// A successful present clears the shell dirty flag, so the next redraw runs
    /// only after another state change (D5). A recoverable present failure
    /// reconfigures the surface and requests another redraw, so the caller retries
    /// on the next frame instead of failing.
    fn render(&mut self) -> Result<FrameOutcome, WindowError> {
        self.refresh_chrome_labels()?;
        let content = match self.composite_content() {
            Some(content) => Some(self.realize_content(content)?),
            None => None,
        };
        let chrome = self.realize_chrome()?;
        let submission = shell_frame(chrome, content, self.surface, self.extent);
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
        self.shell.resize(self.scale.to_logical_extent(extent));
        self.produce_document()?;
        self.window.request_redraw();
        Ok(())
    }

    /// Applies a runtime display-scale change (a monitor move) without a restart.
    ///
    /// A `ScaleFactorChanged` event carries the new scale; the physical surface
    /// extent is unchanged by this event alone (winit pairs it with a `Resized`
    /// when the surface size also changes). Only a real change does work: it stores
    /// the new scale, relays it to the shell and the chrome producer, recomputes the
    /// logical extent from the current physical extent, re-produces the document at
    /// the new resolution, and requests a redraw. The chrome producer rebuilds the
    /// atlas at the new physical size on the real change (S1: never per frame), so
    /// the placed runs are re-pushed to the shell to name the fresh atlas identity
    /// the next realize allocates. It fails closed with the producer error.
    fn apply_scale_change(&mut self, scale_factor: f64) -> Result<(), WindowError> {
        let scale = ScaleFactor::from_winit(scale_factor);
        if scale == self.scale {
            return Ok(());
        }

        self.scale = scale;
        self.shell.set_scale(scale);
        self.chrome_text
            .set_scale(scale)
            .map_err(WindowError::ChromeText)?;
        self.refresh_address_labels()?;
        self.shell.resize(self.scale.to_logical_extent(self.extent));
        self.produce_document()?;
        self.window.request_redraw();
        Ok(())
    }

    /// Produces the active tab's frame for the current viewport geometry (D6).
    ///
    /// The product core produces the frame through the seam; only the content
    /// extent and device pixel ratio cross it (D5). A degenerate (zero-area)
    /// viewport produces no frame and keeps the last one, so a minimized window
    /// does not discard a valid render. An active tab with no document yields no
    /// frame. Runs on initialize and on resize only, never per continuous frame
    /// (S1).
    fn produce_document(&mut self) -> Result<(), WindowError> {
        let Some(geometry) = viewport_geometry(self.physical_viewport_rect(), self.scale) else {
            return Ok(());
        };

        self.document_frame = self
            .tab_model
            .produce_active(geometry)
            .map_err(WindowError::Content)?;
        Ok(())
    }

    /// Offsets and clips the last document frame into the viewport (D5).
    ///
    /// Returns `None` when there is no active document, no frame has been produced,
    /// or the stored frame names a superseded generation, so a stale frame never
    /// paints (S5).
    fn composite_content(&self) -> Option<CompositedDocument> {
        let frame = self.document_frame.as_ref()?;
        let generation = self.tab_model.active_generation()?;
        composite_document(frame, self.physical_viewport_rect(), generation)
    }

    /// Realizes the document uploads on the backend and remaps their identities.
    ///
    /// The seam hands back a document-local display list, not a texture (D1). Each
    /// engine upload names a synthetic resource identity in the engine namespace,
    /// but the backend draws only from a texture it allocated. This allocates one
    /// texture per engine resource identity, rewrites the upload to the allocated
    /// identity, and rewrites every glyph quad that referenced the engine identity,
    /// so the submission names only live backend resources. Allocation is cached by
    /// engine identity: two producers with matching descriptors but distinct
    /// identities never alias onto one backend texture, and the M2 fixture atlas is
    /// stable across renders and resizes, so it allocates one atlas texture for the
    /// life of the window.
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
            let backend_resource = self.ensure_texture(engine_resource, &upload.descriptor)?;
            remap_texture(&mut commands, engine_resource, backend_resource);
            upload.resource = backend_resource;
            realized.push(upload);
        }

        Ok(CompositedDocument {
            commands,
            uploads: realized,
        })
    }

    /// Returns a live backend texture for the engine resource identity, allocating
    /// on first use.
    ///
    /// A cached texture for an equal engine identity is reused, so a repeated render
    /// or a resize does not allocate again. Keying by the full engine identity, not
    /// the descriptor, keeps two producers with matching descriptors on separate
    /// backend textures, so their pixels never alias. The backend exposes no free,
    /// so the cache is the allocation bound: it grows only when a genuinely new
    /// engine identity appears, which the stable M2 atlas never does.
    fn ensure_texture(
        &mut self,
        engine: GpuResourceIdentity,
        descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, WindowError> {
        if let Some((_, resource)) = self.textures.iter().find(|(cached, _)| *cached == engine) {
            return Ok(*resource);
        }

        let resource = self
            .backend
            .allocate_texture(descriptor)
            .map_err(WindowError::Backend)?;
        self.textures.push((engine, resource));
        Ok(resource)
    }

    /// Rebuilds the chrome labels when the producer reports a locale-generation
    /// change, and re-pushes the view to the shell.
    ///
    /// The producer keeps its built atlas while the locale generation is
    /// unchanged, so this only clones and re-pushes the view on a real rebuild
    /// (S1: the atlas builds only on a rebuild, never per frame). A rebuild
    /// advances the atlas resource generation, so the identity-keyed cache
    /// realizes the fresh atlas on the next paint. Runtime has no language control
    /// yet, so the generation advances only when a test drives it.
    fn refresh_chrome_labels(&mut self) -> Result<(), WindowError> {
        if self
            .chrome_text
            .refresh()
            .map_err(WindowError::ChromeText)?
            == ChromeRefresh::Rebuilt
        {
            self.refresh_address_labels()?;
        }
        Ok(())
    }

    /// Composes the chrome label view and pushes it to the shell.
    ///
    /// The three navigation runs come from the producer unchanged. The address
    /// field shows the catalogue placeholder run when the edit buffer is empty and
    /// the field is unfocused, and the live-shaped buffer otherwise (D5, D7). The
    /// shell marks itself dirty only on a real change, so a redundant recompose
    /// drives no repaint. Recomposing happens per input event, not per frame, so
    /// the atlas is never rebuilt from a keystroke (D9).
    fn refresh_address_labels(&mut self) -> Result<(), WindowError> {
        let labels = self.compose_labels()?;
        self.shell.set_labels(labels);
        Ok(())
    }

    /// Builds the neutral label view for the current chrome and address state.
    fn compose_labels(&self) -> Result<LabelView, WindowError> {
        let address_focused = self.shell.focused() == Some(ShellRegion::AddressField);
        compose_labels(
            &self.chrome_text,
            self.shell.address_text(),
            address_focused,
        )
    }

    /// Realizes the chrome atlas and remaps the shell chrome text quads to it.
    ///
    /// The shell paints its chrome text as quads that name the chrome atlas engine
    /// identity, but the backend draws only from a texture it allocated. This
    /// allocates one backend texture for the chrome atlas identity (cached, so a
    /// repeated paint does not allocate again), rewrites the shell chrome quads to
    /// the allocated identity, and rewrites the upload to the same identity, so
    /// the submission names only a live backend resource. The chrome atlas keeps a
    /// distinct producer namespace from the document atlas, so the two never alias
    /// on one backend texture.
    fn realize_chrome(&mut self) -> Result<RealizedChrome, WindowError> {
        let mut commands = self.shell.build_commands();
        let engine = self.chrome_text.identity();
        let mut upload = self.chrome_text.upload().clone();

        let backend = self.ensure_texture(engine, &upload.descriptor)?;
        remap_texture(&mut commands, engine, backend);
        upload.resource = backend;

        Ok(RealizedChrome {
            commands,
            uploads: vec![upload],
        })
    }

    /// Current viewport rectangle in physical surface pixels.
    ///
    /// The shell lays out the viewport in logical pixels; the document renders and
    /// composites at physical resolution, so the content extent and the compositor
    /// use the rectangle scaled by the active display scale.
    fn physical_viewport_rect(&self) -> Rect {
        self.scale
            .scale_rect(self.shell.layout().rect(ShellRegion::Viewport))
    }

    /// Forwards a pointer move to the shell and redraws only on a state change.
    fn pointer_moved(&mut self, position: PhysicalPosition<f64>) {
        let position = to_pointer_position(position, self.scale);
        self.last_cursor = Some(position);
        self.shell.pointer_moved(position);
        self.request_redraw_if_dirty();
    }

    /// Forwards a pointer press and applies any returned action.
    ///
    /// A press with no prior cursor position has no location to hit-test, so it is
    /// ignored. A strip press returns a neutral [`ShellAction`] the window applies
    /// to the model; every other press only updates focus. A redraw is requested
    /// only when the shell changed since the last paint (D5).
    fn pointer_pressed(&mut self) -> Result<(), WindowError> {
        let Some(position) = self.last_cursor else {
            return Ok(());
        };

        if let Some(action) = self.shell.pointer_pressed(position) {
            self.apply_shell_action(action)?;
        }
        self.refresh_address_labels()?;
        self.request_redraw_if_dirty();
        Ok(())
    }

    /// Applies a shell action to the model, refreshes the strip and the committed
    /// address, and re-produces the active frame.
    ///
    /// A tab action carries a slot index the window maps to a `TabId`, enforcing
    /// `MAX_TABS` before a new tab (D8); a submit action carries the raw typed text
    /// the model parses (D3, D4). Either can change the active tab or its content,
    /// so the window rebuilds the neutral strip view, pushes the active tab's
    /// committed address (so a rejected submit visibly reverts, D7), and re-produces
    /// the active frame (D9). The set views mark the shell dirty only on a real
    /// change, so an unchanged push drives no repaint.
    fn apply_shell_action(&mut self, action: ShellAction) -> Result<(), WindowError> {
        apply_shell_action(&mut self.tab_model, action)?;
        self.shell.set_tabs(tab_strip_view(&self.tab_model));
        self.push_committed_address();
        self.produce_document()
    }

    /// Pushes the active tab's committed address into the shell.
    ///
    /// A tab with no committed address (never navigated) pushes an empty string, so
    /// the field shows the placeholder (D5). The shell marks itself dirty only on a
    /// real change.
    fn push_committed_address(&mut self) {
        let text = self
            .tab_model
            .active_address_text()
            .unwrap_or_default()
            .to_owned();
        self.shell.set_committed_address(text);
    }

    /// Forwards a key to the shell and applies any returned submit action.
    ///
    /// The shell mutates its address edit buffer and reports a submit action only on
    /// Enter while the address field is focused (D1). A redraw is requested only
    /// when the shell changed since the last paint (D5).
    fn key_pressed(&mut self, key: KeyInput) -> Result<(), WindowError> {
        if let Some(action) = self.shell.deliver_key(key) {
            self.apply_shell_action(action)?;
        }
        self.refresh_address_labels()?;
        self.request_redraw_if_dirty();
        Ok(())
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
///
/// The injected product core and chrome text producer are held until the window
/// is built, then moved into the presentation. They are `None` before the loop
/// resumes and after that move.
struct WindowApplication {
    presentation: Option<Presentation>,
    tab_model: Option<TabModel>,
    chrome_text: Option<ChromeText>,
    error: Option<WindowError>,
    recovery_deadline: Option<Instant>,
}

impl WindowApplication {
    fn new(tab_model: TabModel, chrome_text: ChromeText) -> Self {
        Self {
            presentation: None,
            tab_model: Some(tab_model),
            chrome_text: Some(chrome_text),
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
    ///
    /// The product core is injected: the composition root already opened and
    /// attached the first tab, so this only moves the core into the presentation
    /// and produces its first frame.
    fn initialize(
        &mut self,
        event_loop: &ActiveEventLoop,
        tab_model: TabModel,
        mut chrome_text: ChromeText,
    ) -> Result<(), WindowError> {
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
        let scale = ScaleFactor::from_winit(window.scale_factor());

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

        // Drive the injected chrome producer at the real display scale before the
        // first realize, so the atlas rasterizes at the physical size (D2).
        chrome_text
            .set_scale(scale)
            .map_err(WindowError::ChromeText)?;

        let mut presentation = Presentation {
            window,
            backend,
            surface,
            extent,
            scale,
            shell: Shell::new(scale.to_logical_extent(extent), scale),
            last_cursor: None,
            tab_model,
            chrome_text,
            document_frame: None,
            textures: Vec::new(),
        };
        presentation
            .shell
            .set_tabs(tab_strip_view(&presentation.tab_model));
        presentation.push_committed_address();
        presentation.refresh_address_labels()?;
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

        let (Some(tab_model), Some(chrome_text)) = (self.tab_model.take(), self.chrome_text.take())
        else {
            return;
        };

        if let Err(error) = self.initialize(event_loop, tab_model, chrome_text) {
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
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Err(error) = presentation.apply_scale_change(scale_factor) {
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
                if let Err(error) = presentation.pointer_pressed() {
                    self.error = Some(error);
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } if key.state == ElementState::Pressed => {
                if let Some(input) = to_key_input(&key)
                    && let Err(error) = presentation.key_pressed(input)
                {
                    self.error = Some(error);
                    event_loop.exit();
                }
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

/// Builds the neutral tab-strip view from the model.
///
/// The tab count is the model tab count and the active slot is the position of
/// the active tab in insertion order, so the shell works in slot indices only and
/// never names a `TabId` (D3).
fn tab_strip_view(model: &TabModel) -> TabStripView {
    let active = model
        .active_tab()
        .and_then(|id| model.tabs().iter().position(|tab| tab.id() == id));
    TabStripView::new(model.tabs().len(), active)
}

/// Applies one neutral shell action to the model.
///
/// The window is the sole index-to-`TabId` mapper (D3) and the sole enforcer of
/// the `MAX_TABS` bound (D8): a `NewTab` at the bound is ignored, and an
/// `ActivateTab` or `CloseTab` for a slot index that names no tab is a no-op. A
/// slot index resolves to a tab through insertion order, so it never confuses one
/// tab with another. A `SubmitAddress` carries raw typed text the model parses and
/// resolves (D3, D4); the model reports rejection as an outcome the field revert
/// already handles, so the window drops the outcome here. The model owns its
/// errors; a rejected activate is mapped to a content error.
fn apply_shell_action(model: &mut TabModel, action: ShellAction) -> Result<(), WindowError> {
    match action {
        ShellAction::ActivateTab(index) => {
            let Some(id) = model.tabs().get(index).map(|tab| tab.id()) else {
                return Ok(());
            };
            model.activate(id).map_err(WindowError::Content)?;
        }
        ShellAction::NewTab => {
            if model.tabs().len() < MAX_TABS {
                model.open_tab();
            }
        }
        ShellAction::CloseTab(index) => {
            let Some(id) = model.tabs().get(index).map(|tab| tab.id()) else {
                return Ok(());
            };
            model.close_tab(id);
        }
        ShellAction::SubmitAddress(text) => {
            model.submit_address(&text).map_err(WindowError::Content)?;
        }
    }

    Ok(())
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

/// Merges the realized shell chrome and the composited document into one
/// submission.
///
/// The shell chrome carries its ordered fills and its chrome text quads, already
/// remapped to the backend atlas identity (D1); the compositor offsets and clips
/// the document into the viewport (D5). This function merges both into one
/// submission for the owned surface and the current extent. The chrome atlas
/// upload leads the submission uploads and the document upload follows. The
/// document paints over the chrome viewport fill, so it is visible without
/// removing the chrome region.
fn shell_frame(
    chrome: RealizedChrome,
    content: Option<CompositedDocument>,
    surface: SurfaceIdentity,
    extent: Extent2d,
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
        chrome.commands,
        chrome.uploads,
        content,
        FrameToken::new(1),
        scene,
        target,
    )
}

/// Composes the neutral label view from the producer view and the address state.
///
/// It keeps every non-address run from the producer view unchanged and selects the
/// address run: the catalogue placeholder run when the edit buffer is empty and the
/// field is unfocused, else the live run shaped against the pre-packed chrome atlas
/// (D5, D7, D9). It never rebuilds the atlas, so shaping stays cheap per event.
fn compose_labels(
    chrome_text: &ChromeText,
    address_text: &str,
    address_focused: bool,
) -> Result<LabelView, WindowError> {
    let show_placeholder = address_text.is_empty() && !address_focused;

    let view = chrome_text.view();
    let mut runs = Vec::with_capacity(view.runs().len());
    for (region, run) in view.runs() {
        if *region == ShellRegion::AddressField && !show_placeholder {
            continue;
        }
        runs.push((*region, run.clone()));
    }

    if !show_placeholder {
        let live = chrome_text
            .shape_address(address_text)
            .map_err(WindowError::ChromeText)?;
        runs.push((ShellRegion::AddressField, live));
    }

    Ok(LabelView::new(runs))
}

/// Derives the viewport geometry for one render from the physical viewport
/// rectangle and the display scale.
///
/// The content extent is the rounded physical viewport size, so the document
/// renders at physical resolution; the device pixel ratio is the display scale, so
/// the engine sizes content correctly for the density (D4). A rectangle that rounds
/// to a zero width or height has no content box and yields `None`, so the caller
/// keeps the last produced frame instead of producing an empty one. The scale is a
/// render-side value only; it is never exposed to web content (I4).
fn viewport_geometry(physical_viewport: Rect, scale: ScaleFactor) -> Option<ViewportGeometry> {
    let width = physical_viewport.width.round();
    let height = physical_viewport.height.round();

    if width < 1.0 || height < 1.0 {
        return None;
    }

    Some(ViewportGeometry {
        content_extent: Extent2d::new(width as u32, height as u32),
        device_pixel_ratio: scale.get(),
    })
}

/// Converts a native physical pointer position into the neutral shell position.
///
/// The shell works in logical pixel space and names no windowing type, so the
/// native physical `f64` position divides by the display scale and narrows to the
/// shell `f32` value at this seam. The scale is validated in a bounded positive
/// range, so the divisor is never zero.
fn to_pointer_position(position: PhysicalPosition<f64>, scale: ScaleFactor) -> PointerPosition {
    let factor = scale.get();
    PointerPosition::new(position.x as f32 / factor, position.y as f32 / factor)
}

/// Converts a native key event into the neutral shell key, if it carries one.
///
/// The window reads the typed text (layout and shift aware) and the named edit
/// keys, never the physical key position (D2). It returns `None` for a key the
/// address field does not consume, so only a real character or an edit key reaches
/// the shell.
fn to_key_input(event: &KeyEvent) -> Option<KeyInput> {
    key_input_from(&event.logical_key, event.text.as_deref())
}

/// Maps a logical key and its typed text to the neutral shell key.
///
/// The three edit keys map from the named key, so they win over any control text
/// they also report. Every other key maps from a single typed character, so a
/// layout- and shift-aware character (including space) is forwarded. A
/// multi-character text value (an IME composition signal) is not forwarded (D2),
/// and a key with no usable text (an arrow or a modifier) yields `None`.
fn key_input_from(logical_key: &Key, text: Option<&str>) -> Option<KeyInput> {
    if let Key::Named(named) = logical_key {
        match named {
            NamedKey::Backspace => return Some(KeyInput::Backspace),
            NamedKey::Enter => return Some(KeyInput::Enter),
            NamedKey::Escape => return Some(KeyInput::Escape),
            _ => {}
        }
    }

    let text = text?;
    let mut characters = text.chars();
    let first = characters.next()?;
    if characters.next().is_some() {
        return None;
    }

    Some(KeyInput::Character(first))
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

#[cfg(test)]
mod tests {
    use super::*;
    use locale::Locale;
    use panther_localization::{ActiveLocaleState, LocaleRequest, LocaleResolver, MessageCatalog};

    fn chrome_text() -> ChromeText {
        let english = Locale::parse("en").expect("valid identifier");
        let resolver = LocaleResolver::new(vec![english.clone()], vec![english]);
        let state = ActiveLocaleState::new(resolver, LocaleRequest::new());
        ChromeText::new(state, MessageCatalog::load(), ScaleFactor::ONE)
            .expect("the producer builds")
    }

    #[test]
    fn viewport_geometry_uses_the_scale_as_the_device_pixel_ratio() {
        let scale = ScaleFactor::from_winit(2.0);
        let physical = scale.scale_rect(Rect::new(0.0, 40.0, 400.0, 300.0));

        let geometry = viewport_geometry(physical, scale).expect("a non-degenerate viewport");

        assert_eq!(geometry.device_pixel_ratio, 2.0);
        assert_eq!(geometry.content_extent, Extent2d::new(800, 600));
    }

    #[test]
    fn viewport_geometry_rejects_a_degenerate_rectangle() {
        assert!(viewport_geometry(Rect::new(0.0, 0.0, 0.0, 300.0), ScaleFactor::ONE).is_none());
    }

    #[test]
    fn pointer_position_converts_physical_to_logical() {
        let scale = ScaleFactor::from_winit(2.0);

        let logical = to_pointer_position(PhysicalPosition::new(200.0, 80.0), scale);

        assert_eq!(logical, PointerPosition::new(100.0, 40.0));
    }

    fn active_id(model: &TabModel) -> Option<u64> {
        model
            .active_tab()
            .and_then(|id| model.tabs().iter().position(|tab| tab.id() == id))
            .map(|index| index as u64)
    }

    #[test]
    fn tab_strip_view_reports_the_count_and_the_active_slot_index() {
        let mut model = TabModel::new();
        model.open_tab();
        let second = model.open_tab();
        model.open_tab();
        model.activate(second).expect("activate succeeds");

        let view = tab_strip_view(&model);

        assert_eq!(view.tab_count(), 3);
        assert_eq!(view.active(), Some(1));
    }

    #[test]
    fn tab_strip_view_of_an_empty_model_has_no_tabs_and_no_active_slot() {
        let model = TabModel::new();

        let view = tab_strip_view(&model);

        assert_eq!(view.tab_count(), 0);
        assert_eq!(view.active(), None);
    }

    #[test]
    fn activate_action_activates_the_tab_at_the_slot_index() {
        let mut model = TabModel::new();
        let first = model.open_tab();
        model.open_tab();
        model.open_tab();

        apply_shell_action(&mut model, ShellAction::ActivateTab(0)).expect("activate applies");

        assert_eq!(model.active_tab(), Some(first));
    }

    #[test]
    fn activate_action_for_an_unknown_slot_is_a_no_op() {
        let mut model = TabModel::new();
        let only = model.open_tab();

        apply_shell_action(&mut model, ShellAction::ActivateTab(5))
            .expect("out-of-range is a no-op");

        assert_eq!(model.active_tab(), Some(only));
        assert_eq!(model.tabs().len(), 1);
    }

    #[test]
    fn new_tab_action_opens_a_tab_up_to_the_bound() {
        let mut model = TabModel::new();

        for _ in 0..MAX_TABS {
            apply_shell_action(&mut model, ShellAction::NewTab).expect("new tab applies");
        }
        assert_eq!(model.tabs().len(), MAX_TABS);

        apply_shell_action(&mut model, ShellAction::NewTab).expect("new tab at the bound applies");
        assert_eq!(model.tabs().len(), MAX_TABS);
    }

    #[test]
    fn close_action_closes_and_reactivates_next_then_previous_then_none() {
        let mut model = TabModel::new();
        let first = model.open_tab();
        let second = model.open_tab();
        let third = model.open_tab();
        model.activate(second).expect("activate succeeds");

        apply_shell_action(&mut model, ShellAction::CloseTab(1)).expect("close applies");
        assert_eq!(model.active_tab(), Some(third));

        apply_shell_action(&mut model, ShellAction::CloseTab(1)).expect("close applies");
        assert_eq!(model.active_tab(), Some(first));

        apply_shell_action(&mut model, ShellAction::CloseTab(0)).expect("close applies");
        assert_eq!(model.active_tab(), None);
    }

    #[test]
    fn close_action_for_an_unknown_slot_is_a_no_op() {
        let mut model = TabModel::new();
        model.open_tab();

        apply_shell_action(&mut model, ShellAction::CloseTab(9)).expect("out-of-range is a no-op");

        assert_eq!(model.tabs().len(), 1);
        assert_eq!(active_id(&model), Some(0));
    }

    #[test]
    fn key_input_maps_a_single_typed_character() {
        let input = key_input_from(&Key::Character("a".into()), Some("a"));

        assert_eq!(input, Some(KeyInput::Character('a')));
    }

    #[test]
    fn key_input_maps_the_three_named_edit_keys() {
        assert_eq!(
            key_input_from(&Key::Named(NamedKey::Backspace), Some("\u{8}")),
            Some(KeyInput::Backspace)
        );
        assert_eq!(
            key_input_from(&Key::Named(NamedKey::Enter), Some("\r")),
            Some(KeyInput::Enter)
        );
        assert_eq!(
            key_input_from(&Key::Named(NamedKey::Escape), None),
            Some(KeyInput::Escape)
        );
    }

    #[test]
    fn key_input_does_not_forward_a_multi_character_text_value() {
        let input = key_input_from(&Key::Character("ab".into()), Some("ab"));

        assert_eq!(input, None);
    }

    #[test]
    fn submit_address_action_attaches_the_fixture_and_commits_the_address() {
        let mut model = TabModel::new();
        model.open_tab();

        apply_shell_action(
            &mut model,
            ShellAction::SubmitAddress("panther:demo".to_owned()),
        )
        .expect("submit applies");

        assert_eq!(model.active_address_text(), Some("panther:demo"));
    }

    #[test]
    fn a_rejected_submit_action_leaves_the_committed_address_unchanged() {
        let mut model = TabModel::new();
        model.open_tab();

        apply_shell_action(
            &mut model,
            ShellAction::SubmitAddress("https://example.com".to_owned()),
        )
        .expect("submit applies");

        assert_eq!(model.active_address_text(), None);
    }

    const NAVIGATION_REGIONS: [ShellRegion; 3] = [
        ShellRegion::NavigationBack,
        ShellRegion::NavigationForward,
        ShellRegion::NavigationReload,
    ];

    #[test]
    fn an_empty_unfocused_address_composes_the_catalogue_placeholder_run() {
        let chrome = chrome_text();

        let labels = compose_labels(&chrome, "", false).expect("the labels compose");

        let placeholder = chrome
            .view()
            .run(ShellRegion::AddressField)
            .expect("the catalogue placeholder run");
        assert_eq!(labels.run(ShellRegion::AddressField), Some(placeholder));
        for region in NAVIGATION_REGIONS {
            assert_eq!(labels.run(region), chrome.view().run(region));
        }
    }

    #[test]
    fn a_non_empty_buffer_composes_the_live_run() {
        let chrome = chrome_text();

        let labels = compose_labels(&chrome, "panther:demo", true).expect("the labels compose");

        let live = chrome.shape_address("panther:demo").expect("the live run");
        assert_eq!(labels.run(ShellRegion::AddressField), Some(&live));
        let placeholder = chrome
            .view()
            .run(ShellRegion::AddressField)
            .expect("the catalogue placeholder run");
        assert_ne!(labels.run(ShellRegion::AddressField), Some(placeholder));
        for region in NAVIGATION_REGIONS {
            assert_eq!(labels.run(region), chrome.view().run(region));
        }
    }
}
