// @file products/panther/window/tests/document-submit.rs
// @description Asserts a produced document frame submits only after its uploads are realized on the backend.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Document realization and submission assertion.
//!
//! The seam hands back a document-local display list whose uploads name synthetic
//! engine-namespace resource identities (D1). A backend draws only from a texture
//! it allocated, so the product must realize each upload (allocate a texture and
//! remap the glyph quads) before it submits. This test drives the deterministic
//! software backend headlessly and asserts both halves of that contract: the raw
//! frame is rejected with `ResourceNotFound`, and the realized frame is accepted.
//!
//! The realization recipe here mirrors `Presentation::realize_content` in the
//! window loop. The software backend never reads a native handle, so a headless
//! window stand-in keeps the test free of any platform handle and free of
//! `unsafe`.

use purr_embedding::{DocumentSession, ViewportGeometry, m2_demonstration_fixture};
use purr_graphics::{
    AlphaMode, BackendKind, DrawCommand, Extent2d, FrameSubmission, FrameToken,
    GpuResourceIdentity, GraphicsBackend, GraphicsError, PresentationTargetDescriptor,
    ResourceUpload, SceneGeneration, SceneId, SceneIdentity, SurfaceIdentity, TextureFormatClass,
    WindowSurface,
};
use purr_graphics_software::SoftwareBackend;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

const EXTENT: Extent2d = Extent2d {
    width: 800,
    height: 600,
};
const FORMAT: TextureFormatClass = TextureFormatClass::Bgra8Unorm;

/// Window stand-in that reports no handle.
///
/// The software backend never reads the handle, so an `Unavailable` result keeps
/// the test free of any real platform handle and free of `unsafe`.
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

fn presentation_descriptor() -> PresentationTargetDescriptor {
    PresentationTargetDescriptor {
        extent: EXTENT,
        format: FORMAT,
        alpha_mode: AlphaMode::Opaque,
    }
}

fn submission(
    surface: SurfaceIdentity,
    commands: Vec<DrawCommand>,
    uploads: Vec<ResourceUpload>,
) -> FrameSubmission {
    FrameSubmission {
        frame_token: FrameToken::new(1),
        scene: SceneIdentity::new(
            SceneId::new(1),
            SceneGeneration::new(1),
            surface.surface_id(),
            surface.surface_generation(),
        ),
        target: presentation_descriptor(),
        uploads,
        commands,
    }
}

/// Repoints every glyph quad from one resource identity to another.
fn remap(commands: &mut [DrawCommand], from: GpuResourceIdentity, to: GpuResourceIdentity) {
    for command in commands.iter_mut() {
        if let DrawCommand::TexturedQuad { texture, .. } = command
            && *texture == from
        {
            *texture = to;
        }
    }
}

#[test]
fn a_produced_frame_submits_only_after_its_uploads_are_realized() {
    let mut session = DocumentSession::new();
    let handle = session
        .attach(m2_demonstration_fixture())
        .expect("the bundled fixture attaches");
    let frame = session
        .produce(
            &handle,
            ViewportGeometry {
                content_extent: EXTENT,
                device_pixel_ratio: 1.0,
            },
        )
        .expect("the fixture produces a frame");

    assert_eq!(
        frame.uploads.len(),
        1,
        "the fixture produces the glyph atlas upload"
    );

    let mut backend = SoftwareBackend::create(BackendKind::Software).expect("software backend");
    let surface = backend
        .create_presentation_target(
            WindowSurface::new(&HeadlessWindow, EXTENT),
            presentation_descriptor(),
        )
        .expect("target creation succeeds");

    // The raw frame names a synthetic engine resource the backend never allocated.
    let raw = submission(surface, frame.commands.clone(), frame.uploads.clone());
    assert_eq!(
        backend.submit(surface, &raw),
        Err(GraphicsError::ResourceNotFound),
        "an un-realized frame is rejected because its atlas resource does not exist"
    );

    // Realize each upload: allocate a texture and remap the glyph quads to it.
    let mut commands = frame.commands.clone();
    let mut uploads = Vec::with_capacity(frame.uploads.len());
    for mut upload in frame.uploads.clone() {
        let engine_resource = upload.resource;
        let backend_resource = backend
            .allocate_texture(&upload.descriptor)
            .expect("the atlas texture allocates");
        remap(&mut commands, engine_resource, backend_resource);
        upload.resource = backend_resource;
        uploads.push(upload);
    }

    let realized = submission(surface, commands, uploads);
    backend
        .submit(surface, &realized)
        .expect("the realized frame is accepted");
    backend
        .present(surface)
        .expect("the realized frame presents");
}
