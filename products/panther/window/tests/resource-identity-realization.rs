// @file products/panther/window/tests/resource-identity-realization.rs
// @description Asserts the window realizes one backend texture per engine resource identity, not per descriptor.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Resource-identity realization assertion.
//!
//! The window caches realized backend textures by the engine resource identity,
//! not by the `TextureDescriptor`. Two producers (the engine document atlas and
//! the chrome atlas) can hand back the same descriptor (same extent and format)
//! yet carry different pixels under different producer namespaces. Keying by the
//! descriptor would return one backend texture for both and alias their pixels;
//! keying by the full identity keeps them apart.
//!
//! This drives the deterministic software backend headlessly and mirrors the
//! identity-keyed recipe in `Presentation::ensure_texture` and
//! `Presentation::realize_content`: it caches by the engine identity, allocates a
//! backend texture on a miss, and remaps each producer's glyph quads to its own
//! backend texture. It asserts the two uploads allocate two distinct backend
//! textures and that no quad aliases the other producer's texture. The realized
//! frame then submits and presents, so both backend textures are proven live.

use purr_graphics::{
    AlphaMode, BackendKind, Color, ColorSpace, DeviceGeneration, DrawCommand, Extent2d,
    FrameSubmission, FrameToken, GpuResourceIdentity, GraphicsBackend,
    PresentationTargetDescriptor, ProducerNamespace, Rect, ResourceGeneration, ResourceId,
    ResourceKind, ResourceUpload, SceneGeneration, SceneId, SceneIdentity, SurfaceIdentity,
    TextureDescriptor, TextureFormatClass, WindowSurface,
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

/// Shared atlas extent for both producers.
const ATLAS: Extent2d = Extent2d {
    width: 2,
    height: 2,
};

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

/// Atlas descriptor shared by both producers.
///
/// Both uploads carry this identical descriptor; only their engine resource
/// identities differ. A descriptor-keyed cache would collapse them onto one
/// texture.
fn atlas_descriptor() -> TextureDescriptor {
    TextureDescriptor {
        extent: ATLAS,
        format: FORMAT,
        color_space: ColorSpace::Srgb,
        alpha_mode: AlphaMode::Premultiplied,
        label: None,
    }
}

/// Engine resource identity in a producer namespace.
fn engine_identity(namespace: u32) -> GpuResourceIdentity {
    GpuResourceIdentity::new(
        ProducerNamespace::new(namespace),
        ResourceId::new(1),
        ResourceGeneration::new(1),
        ResourceKind::Texture,
        DeviceGeneration::new(1),
    )
}

/// One atlas upload for a producer namespace, with the shared descriptor.
fn atlas_upload(namespace: u32) -> ResourceUpload {
    let bytes = (ATLAS.width * ATLAS.height * 4) as usize;
    ResourceUpload {
        resource: engine_identity(namespace),
        descriptor: atlas_descriptor(),
        pixels: vec![0u8; bytes],
    }
}

/// One glyph quad sampling a producer's engine atlas identity.
fn atlas_quad(namespace: u32) -> DrawCommand {
    DrawCommand::TexturedQuad {
        rect: Rect::new(0.0, 0.0, 2.0, 2.0),
        texture: engine_identity(namespace),
        source: Rect::new(0.0, 0.0, 2.0, 2.0),
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
///
/// Mirrors `Presentation::remap_texture`: a quad that names a different resource
/// is left unchanged, so several uploads remap independently.
fn remap(commands: &mut [DrawCommand], from: GpuResourceIdentity, to: GpuResourceIdentity) {
    for command in commands.iter_mut() {
        if let DrawCommand::TexturedQuad { texture, .. } = command
            && *texture == from
        {
            *texture = to;
        }
    }
}

/// Returns the texture identity a textured quad samples.
fn quad_texture(command: &DrawCommand) -> GpuResourceIdentity {
    match command {
        DrawCommand::TexturedQuad { texture, .. } => *texture,
        _ => panic!("the command is a textured quad"),
    }
}

#[test]
fn two_same_descriptor_uploads_with_distinct_identities_do_not_alias() {
    let mut backend = SoftwareBackend::create(BackendKind::Software).expect("software backend");
    let surface = backend
        .create_presentation_target(
            WindowSurface::new(&HeadlessWindow, EXTENT),
            presentation_descriptor(),
        )
        .expect("target creation succeeds");

    // Two producers hand back the same atlas descriptor under different namespaces.
    let uploads = vec![atlas_upload(10), atlas_upload(20)];
    let mut commands = vec![atlas_quad(10), atlas_quad(20)];

    // Mirror the identity-keyed realization: cache by the engine identity, allocate
    // on a miss, and remap each producer's quad to its own backend texture.
    let mut cache: Vec<(GpuResourceIdentity, GpuResourceIdentity)> = Vec::new();
    let mut realized = Vec::with_capacity(uploads.len());
    for mut upload in uploads {
        let engine_resource = upload.resource;
        let backend_resource = match cache.iter().find(|(cached, _)| *cached == engine_resource) {
            Some((_, resource)) => *resource,
            None => {
                let resource = backend
                    .allocate_texture(&upload.descriptor)
                    .expect("the atlas texture allocates");
                cache.push((engine_resource, resource));
                resource
            }
        };
        remap(&mut commands, engine_resource, backend_resource);
        upload.resource = backend_resource;
        realized.push(upload);
    }

    // The identical descriptor did not collapse the two uploads: each engine
    // identity got its own backend texture.
    assert_eq!(
        cache.len(),
        2,
        "each engine identity realizes its own texture"
    );
    let first_backend = cache[0].1;
    let second_backend = cache[1].1;
    assert_ne!(
        first_backend, second_backend,
        "distinct engine identities never share one backend texture"
    );

    // Each producer's quad samples its own backend texture; neither aliases the
    // other producer's texture.
    let first_quad = quad_texture(&commands[0]);
    let second_quad = quad_texture(&commands[1]);
    assert_eq!(
        first_quad, first_backend,
        "the first quad samples its own texture"
    );
    assert_eq!(
        second_quad, second_backend,
        "the second quad samples its own texture"
    );
    assert_ne!(
        first_quad, second_quad,
        "the quads do not alias one texture"
    );

    // Both backend textures are live: the realized frame submits and presents.
    let mut frame_commands = vec![DrawCommand::Clear {
        color: Color::new(0.0, 0.0, 0.0, 1.0),
    }];
    frame_commands.extend(commands);
    let frame = submission(surface, frame_commands, realized);
    backend
        .submit(surface, &frame)
        .expect("the realized frame is accepted");
    backend
        .present(surface)
        .expect("the realized frame presents");
}
