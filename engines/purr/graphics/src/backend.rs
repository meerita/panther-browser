// @file engines/purr/graphics/src/backend.rs
// @description Defines the backend contract and the neutral window-handle seam.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Backend contract and window-handle seam.
//!
//! `GraphicsBackend` is the single contract the engine and the product use to
//! reach the GPU. Every method speaks in Panther identities and descriptors and
//! never returns a backend or native-API handle. The two M0 backends (hardware
//! and software) implement this one contract behind the interface.
//!
//! `WindowSurface` is the only place a native window reaches the interface. It
//! carries a handle through the neutral `raw-window-handle` traits, so the
//! interface names no `wgpu` or platform window type. The wrapper either borrows
//! the handle for the duration of one call, or holds a shared owned handle a
//! backend can retain past the call; it never owns or stores a backend type.
//!
//! A backend that presents on a later frame (the software backend blits through
//! `softbuffer`) needs a handle that outlives the creating call. The owned form
//! carries a shared, cloneable neutral handle for that purpose. The handle stays
//! backend-neutral: it is only the `raw-window-handle` traits behind an `Rc`.
//!
//! The runtime methods take `&self` or `&mut self` and use no generics, so a
//! backend is reachable through a trait object where a later phase needs runtime
//! selection. The window handle stays object-safe because `WindowSurface` is a
//! concrete type; its generic constructor keeps the generic off the trait.

use std::rc::Rc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::descriptor::{Extent2d, PresentationTargetDescriptor, TextureDescriptor};
use crate::graphics_error::GraphicsError;
use crate::identity::{DeviceGeneration, GpuResourceIdentity, SurfaceIdentity};
use crate::submission::FrameSubmission;

/// Neutral window handle a backend can share and retain.
///
/// The trait unites the two `raw-window-handle` traits so one trait object
/// carries both the window and the display side of a native window. A backend
/// that must keep the handle past the creating call retains it behind an `Rc`,
/// so ownership is shared with the window owner and no backend or platform type
/// crosses the seam.
pub trait WindowDisplayHandle: HasWindowHandle + HasDisplayHandle {}

impl<T: HasWindowHandle + HasDisplayHandle + ?Sized> WindowDisplayHandle for T {}

/// Backend a caller asks the interface to create.
///
/// A closed set. `Hardware` is the GPU backend; `Software` is the CPU backend.
/// This enum is the selection mechanism only. The policy that chooses between
/// them is a later phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Hardware,
    Software,
}

/// Handle a `WindowSurface` carries to the backend.
///
/// The borrowed form lasts for one call and suits a backend that reads the
/// handle at creation time. The owned form is a shared neutral handle a backend
/// can retain for a later present.
enum HandleSource<'window> {
    Borrowed {
        window_handle: &'window dyn HasWindowHandle,
        display_handle: &'window dyn HasDisplayHandle,
    },
    Owned(Rc<dyn WindowDisplayHandle>),
}

/// Native window a backend presents to.
///
/// The wrapper carries a handle that implements the neutral `raw-window-handle`
/// traits, together with the initial target extent. A backend reads the handle
/// through the safe traits only; the interface names no platform window type. A
/// backend that must retain the handle for a later present reads the shared owned
/// handle through `owned_handle`.
pub struct WindowSurface<'window> {
    source: HandleSource<'window>,
    extent: Extent2d,
}

impl<'window> WindowSurface<'window> {
    /// Borrows one window handle and the target extent.
    ///
    /// The handle must implement both neutral window-handle traits. A `winit`
    /// window satisfies both, so one borrow supplies the window and the display
    /// side of the seam. A backend that keeps nothing past the call uses this
    /// form.
    pub fn new<H>(handle: &'window H, extent: Extent2d) -> Self
    where
        H: HasWindowHandle + HasDisplayHandle,
    {
        Self {
            source: HandleSource::Borrowed {
                window_handle: handle,
                display_handle: handle,
            },
            extent,
        }
    }

    /// Carries a shared owned window handle and the target extent.
    ///
    /// A backend that presents on a later frame clones the shared handle through
    /// `owned_handle` and retains it, so the window stays alive as long as the
    /// backend needs it. The handle stays neutral: it is only the
    /// `raw-window-handle` traits behind an `Rc`.
    pub fn new_owned(handle: Rc<dyn WindowDisplayHandle>, extent: Extent2d) -> Self {
        Self {
            source: HandleSource::Owned(handle),
            extent,
        }
    }

    pub fn window_handle(&self) -> &dyn HasWindowHandle {
        match &self.source {
            HandleSource::Borrowed { window_handle, .. } => *window_handle,
            HandleSource::Owned(handle) => handle,
        }
    }

    pub fn display_handle(&self) -> &dyn HasDisplayHandle {
        match &self.source {
            HandleSource::Borrowed { display_handle, .. } => *display_handle,
            HandleSource::Owned(handle) => handle,
        }
    }

    /// Returns the shared owned handle when the surface carries one.
    ///
    /// A borrowed surface returns `None`, so a backend that needs to retain the
    /// handle fails to find one and stays headless instead of borrowing a handle
    /// it cannot keep.
    pub fn owned_handle(&self) -> Option<Rc<dyn WindowDisplayHandle>> {
        match &self.source {
            HandleSource::Owned(handle) => Some(Rc::clone(handle)),
            HandleSource::Borrowed { .. } => None,
        }
    }

    pub fn extent(&self) -> Extent2d {
        self.extent
    }
}

/// Contract both M0 backends implement.
///
/// Every method returns Panther identities and interface errors, never a backend
/// handle or a native-API error. A backend adapter confines its dependency types
/// and translates its failures into `GraphicsError` at this boundary.
pub trait GraphicsBackend: Sized {
    /// Creates a backend of the requested kind.
    ///
    /// Fails with `Unsupported` when the platform cannot provide the requested
    /// kind.
    fn create(selection: BackendKind) -> Result<Self, GraphicsError>;

    /// Reports the current device generation.
    ///
    /// A device loss advances this generation, so a caller can detect a resource
    /// stamped by an older device.
    fn device_generation(&self) -> DeviceGeneration;

    /// Creates a presentation target for a native window.
    ///
    /// The backend owns the target and keys it by the returned identity. The
    /// caller holds only the identity, never a backend handle.
    fn create_presentation_target(
        &mut self,
        surface: WindowSurface<'_>,
        descriptor: PresentationTargetDescriptor,
    ) -> Result<SurfaceIdentity, GraphicsError>;

    /// Resizes an existing presentation target.
    ///
    /// Fails with `ResourceNotFound` when the identity does not name a live
    /// target.
    fn resize_presentation_target(
        &mut self,
        surface: SurfaceIdentity,
        extent: Extent2d,
    ) -> Result<(), GraphicsError>;

    /// Allocates a texture from a descriptor.
    ///
    /// The descriptor is validated for bounds before any allocation.
    fn allocate_texture(
        &mut self,
        descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, GraphicsError>;

    /// Submits one frame of logical work against a presentation target.
    ///
    /// The submission is validated before the backend consumes it.
    fn submit(
        &mut self,
        surface: SurfaceIdentity,
        submission: &FrameSubmission,
    ) -> Result<(), GraphicsError>;

    /// Presents the last submitted frame of a target.
    fn present(&mut self, surface: SurfaceIdentity) -> Result<(), GraphicsError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{DisplayHandle, HandleError, WindowHandle};

    use crate::descriptor::{
        AlphaMode, Color, ColorSpace, PresentationTargetDescriptor, TextureDescriptor,
        TextureFormatClass,
    };
    use crate::identity::{
        FrameToken, ProducerNamespace, ResourceGeneration, ResourceId, ResourceKind,
        SceneGeneration, SceneId, SceneIdentity, SurfaceGeneration, SurfaceId,
    };
    use crate::submission::{DrawCommand, FrameSubmission};

    /// Window stand-in that reports no handle.
    ///
    /// The seam only needs a type that implements the neutral traits. Reporting
    /// `Unavailable` keeps the test free of any real platform handle and free of
    /// `unsafe`.
    struct HeadlessWindow;

    impl HasWindowHandle for HeadlessWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            Err(HandleError::Unavailable)
        }
    }

    impl HasDisplayHandle for HeadlessWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Err(HandleError::Unavailable)
        }
    }

    #[derive(Debug)]
    struct RecordingBackend {
        selection: BackendKind,
        next_surface: u64,
        next_resource: u64,
        created_targets: Vec<SurfaceIdentity>,
        resized: Vec<(SurfaceIdentity, Extent2d)>,
        allocated: Vec<GpuResourceIdentity>,
        submitted: Vec<(SurfaceIdentity, FrameSubmission)>,
        presented: Vec<SurfaceIdentity>,
    }

    impl GraphicsBackend for RecordingBackend {
        fn create(selection: BackendKind) -> Result<Self, GraphicsError> {
            Ok(Self {
                selection,
                next_surface: 1,
                next_resource: 1,
                created_targets: Vec::new(),
                resized: Vec::new(),
                allocated: Vec::new(),
                submitted: Vec::new(),
                presented: Vec::new(),
            })
        }

        fn device_generation(&self) -> DeviceGeneration {
            DeviceGeneration::new(1)
        }

        fn create_presentation_target(
            &mut self,
            _surface: WindowSurface<'_>,
            _descriptor: PresentationTargetDescriptor,
        ) -> Result<SurfaceIdentity, GraphicsError> {
            let identity = SurfaceIdentity::new(
                SurfaceId::new(self.next_surface),
                SurfaceGeneration::new(1),
                ProducerNamespace::new(1),
            );
            self.next_surface += 1;
            self.created_targets.push(identity);
            Ok(identity)
        }

        fn resize_presentation_target(
            &mut self,
            surface: SurfaceIdentity,
            extent: Extent2d,
        ) -> Result<(), GraphicsError> {
            if !self.created_targets.contains(&surface) {
                return Err(GraphicsError::ResourceNotFound);
            }

            self.resized.push((surface, extent));
            Ok(())
        }

        fn allocate_texture(
            &mut self,
            descriptor: &TextureDescriptor,
        ) -> Result<GpuResourceIdentity, GraphicsError> {
            descriptor.validate()?;

            let identity = GpuResourceIdentity::new(
                ProducerNamespace::new(1),
                ResourceId::new(self.next_resource),
                ResourceGeneration::new(1),
                ResourceKind::Texture,
                self.device_generation(),
            );
            self.next_resource += 1;
            self.allocated.push(identity);
            Ok(identity)
        }

        fn submit(
            &mut self,
            surface: SurfaceIdentity,
            submission: &FrameSubmission,
        ) -> Result<(), GraphicsError> {
            if !self.created_targets.contains(&surface) {
                return Err(GraphicsError::ResourceNotFound);
            }

            submission.validate()?;
            self.submitted.push((surface, submission.clone()));
            Ok(())
        }

        fn present(&mut self, surface: SurfaceIdentity) -> Result<(), GraphicsError> {
            let submitted = self.submitted.iter().any(|(target, _)| *target == surface);
            if !submitted {
                return Err(GraphicsError::SubmissionRejected);
            }

            self.presented.push(surface);
            Ok(())
        }
    }

    fn presentation_descriptor() -> PresentationTargetDescriptor {
        PresentationTargetDescriptor {
            extent: Extent2d::new(640, 480),
            format: TextureFormatClass::Bgra8Unorm,
            alpha_mode: AlphaMode::Opaque,
        }
    }

    fn clear_submission(surface: SurfaceIdentity) -> FrameSubmission {
        FrameSubmission {
            frame_token: FrameToken::new(1),
            scene: SceneIdentity::new(
                SceneId::new(1),
                SceneGeneration::new(1),
                surface.surface_id(),
                surface.surface_generation(),
            ),
            target: presentation_descriptor(),
            uploads: Vec::new(),
            commands: vec![DrawCommand::Clear {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            }],
        }
    }

    #[test]
    fn create_presentation_target_returns_identity() {
        let window = HeadlessWindow;
        let mut backend =
            RecordingBackend::create(BackendKind::Software).expect("software backend creates");

        let surface = WindowSurface::new(&window, Extent2d::new(640, 480));
        let identity = backend
            .create_presentation_target(surface, presentation_descriptor())
            .expect("target creation succeeds");

        assert_eq!(backend.selection, BackendKind::Software);
        assert_eq!(backend.created_targets, vec![identity]);
    }

    #[test]
    fn submit_records_the_commands() {
        let window = HeadlessWindow;
        let mut backend =
            RecordingBackend::create(BackendKind::Hardware).expect("hardware backend creates");
        let surface = backend
            .create_presentation_target(
                WindowSurface::new(&window, Extent2d::new(640, 480)),
                presentation_descriptor(),
            )
            .expect("target creation succeeds");

        let submission = clear_submission(surface);

        backend
            .submit(surface, &submission)
            .expect("valid submission succeeds");

        assert_eq!(backend.submitted, vec![(surface, submission)]);
    }

    #[test]
    fn present_succeeds_after_submit() {
        let window = HeadlessWindow;
        let mut backend =
            RecordingBackend::create(BackendKind::Hardware).expect("hardware backend creates");
        let surface = backend
            .create_presentation_target(
                WindowSurface::new(&window, Extent2d::new(640, 480)),
                presentation_descriptor(),
            )
            .expect("target creation succeeds");
        backend
            .submit(surface, &clear_submission(surface))
            .expect("valid submission succeeds");

        backend.present(surface).expect("present succeeds");

        assert_eq!(backend.presented, vec![surface]);
    }

    #[test]
    fn present_without_submit_is_rejected() {
        let window = HeadlessWindow;
        let mut backend =
            RecordingBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = backend
            .create_presentation_target(
                WindowSurface::new(&window, Extent2d::new(640, 480)),
                presentation_descriptor(),
            )
            .expect("target creation succeeds");

        assert_eq!(
            backend.present(surface),
            Err(GraphicsError::SubmissionRejected)
        );
    }

    #[test]
    fn allocate_texture_returns_identity() {
        let mut backend =
            RecordingBackend::create(BackendKind::Software).expect("software backend creates");
        let descriptor = TextureDescriptor {
            extent: Extent2d::new(16, 16),
            format: TextureFormatClass::Rgba8Unorm,
            color_space: ColorSpace::Srgb,
            alpha_mode: AlphaMode::Premultiplied,
            label: None,
        };

        let identity = backend
            .allocate_texture(&descriptor)
            .expect("valid descriptor allocates");

        assert_eq!(identity.resource_kind(), ResourceKind::Texture);
        assert_eq!(backend.allocated, vec![identity]);
    }

    #[test]
    fn resize_unknown_target_is_rejected() {
        let mut backend =
            RecordingBackend::create(BackendKind::Software).expect("software backend creates");
        let unknown = SurfaceIdentity::new(
            SurfaceId::new(99),
            SurfaceGeneration::new(1),
            ProducerNamespace::new(1),
        );

        assert_eq!(
            backend.resize_presentation_target(unknown, Extent2d::new(320, 240)),
            Err(GraphicsError::ResourceNotFound)
        );
    }
}
