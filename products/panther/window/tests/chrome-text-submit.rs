// @file products/panther/window/tests/chrome-text-submit.rs
// @description Asserts the chrome atlas realizes, the shell chrome quads remap, and the merged frame submits with the chrome upload included.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Chrome text realization and submission assertion.
//!
//! The shell paints its chrome labels as textured quads that name the chrome
//! atlas engine identity, but a backend draws only from a texture it allocated.
//! The window must realize the chrome atlas (allocate a texture and remap the
//! chrome quads) and carry the chrome atlas upload on the submission alongside
//! the document uploads. This test drives the deterministic software backend
//! headlessly and asserts that contract: the chrome atlas and the document atlas
//! realize as two distinct backend textures, a chrome text quad references the
//! realized chrome texture, the submission carries the chrome atlas upload, and
//! the merged frame submits and presents.
//!
//! The realization recipe here mirrors `Presentation::realize_chrome` and
//! `merge_submission` in the window loop. The software backend never reads a
//! native handle, so a headless window stand-in keeps the test free of any
//! platform handle and free of `unsafe`.

use locale::Locale;
use panther_chrome_text::ChromeText;
use panther_localization::{ActiveLocaleState, LocaleRequest, LocaleResolver, MessageCatalog};
use panther_shell::{LabelView, ScaleFactor, Shell};
use purr_embedding::{DocumentSession, ViewportGeometry, m2_demonstration_fixture};
use purr_graphics::{
    AlphaMode, BackendKind, DrawCommand, Extent2d, FrameSubmission, FrameToken,
    GpuResourceIdentity, GraphicsBackend, PresentationTargetDescriptor, ResourceUpload,
    SceneGeneration, SceneId, SceneIdentity, SurfaceIdentity, TextureFormatClass, WindowSurface,
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

/// Whether any command samples the given texture identity.
fn samples(commands: &[DrawCommand], texture: GpuResourceIdentity) -> bool {
    commands.iter().any(
        |command| matches!(command, DrawCommand::TexturedQuad { texture: t, .. } if *t == texture),
    )
}

/// Builds a chrome text producer that resolves at the reference locale.
///
/// The resolver ships only `en`, so the test is deterministic and does not read
/// the host locale preference.
fn producer() -> ChromeText {
    let english = Locale::parse("en").expect("the reference locale is valid");
    let resolver = LocaleResolver::new(vec![english.clone()], vec![english]);
    let state = ActiveLocaleState::new(resolver, LocaleRequest::new());
    ChromeText::new(state, MessageCatalog::load(), ScaleFactor::ONE).expect("the producer builds")
}

#[test]
fn the_chrome_atlas_realizes_and_the_merged_frame_submits_with_its_upload() {
    // The shell paints the producer's labels as quads naming the chrome atlas.
    let producer = producer();
    let mut shell = Shell::new(EXTENT, ScaleFactor::ONE);
    shell.set_labels(LabelView::new(producer.view().runs().to_vec()));
    let mut chrome_commands = shell.build_commands();

    let chrome_engine = producer.identity();
    assert!(
        samples(&chrome_commands, chrome_engine),
        "the shell paints at least one chrome text quad naming the chrome atlas"
    );

    // The document frame carries its own glyph atlas under the engine namespace.
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
    let document_engine = frame.uploads[0].resource;

    // The chrome atlas and the document atlas carry distinct engine identities.
    assert_ne!(
        chrome_engine, document_engine,
        "the chrome atlas and the document atlas never share an engine identity"
    );

    let mut backend = SoftwareBackend::create(BackendKind::Software).expect("software backend");
    let surface = backend
        .create_presentation_target(
            WindowSurface::new(&HeadlessWindow, EXTENT),
            presentation_descriptor(),
        )
        .expect("target creation succeeds");

    // Realize the chrome atlas: allocate a texture and remap the chrome quads.
    let mut chrome_upload = producer.upload().clone();
    let chrome_backend = backend
        .allocate_texture(&chrome_upload.descriptor)
        .expect("the chrome atlas texture allocates");
    remap(&mut chrome_commands, chrome_engine, chrome_backend);
    chrome_upload.resource = chrome_backend;

    // Realize each document upload the same way.
    let mut document_commands = frame.commands.clone();
    let mut document_uploads = Vec::with_capacity(frame.uploads.len());
    for mut upload in frame.uploads.clone() {
        let engine_resource = upload.resource;
        let backend_resource = backend
            .allocate_texture(&upload.descriptor)
            .expect("the document atlas texture allocates");
        remap(&mut document_commands, engine_resource, backend_resource);
        upload.resource = backend_resource;
        document_uploads.push(upload);
    }
    let document_backend = document_uploads[0].resource;

    // The two atlases realize as two distinct backend textures.
    assert_ne!(
        chrome_backend, document_backend,
        "the distinct engine identities allocate two distinct backend textures"
    );

    // Merge: chrome paints first, its upload leads, then the document follows.
    let mut commands = chrome_commands;
    commands.extend(document_commands);
    let mut uploads = vec![chrome_upload];
    uploads.extend(document_uploads);
    let merged = submission(surface, commands, uploads);

    // The chrome atlas upload is included and a chrome text quad names it.
    assert!(
        merged
            .uploads
            .iter()
            .any(|upload| upload.resource == chrome_backend),
        "the submission carries the realized chrome atlas upload"
    );
    assert!(
        samples(&merged.commands, chrome_backend),
        "a chrome text quad references the realized backend texture"
    );

    backend
        .submit(surface, &merged)
        .expect("the realized frame is accepted");
    backend
        .present(surface)
        .expect("the realized frame presents");
}
