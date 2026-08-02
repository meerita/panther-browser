// @file products/panther/shell/tests/headless-render.rs
// @description Asserts the shell command list renders to the expected region pixels.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Headless software-backend render assertion (D7).
//!
//! The test runs the shell draw-command list through the deterministic software
//! backend and asserts the pixels at the center of chosen regions, so the paint
//! path is verified without a window. The software backend never reads a native
//! handle, so a headless window stand-in keeps the test free of any platform
//! handle and free of `unsafe`.

use purr_graphics::{
    AlphaMode, BackendKind, Color, DrawCommand, Extent2d, FrameSubmission, FrameToken,
    GraphicsBackend, PresentationTargetDescriptor, Rect, SceneGeneration, SceneId, SceneIdentity,
    SurfaceIdentity, TextureFormatClass, WindowSurface,
};
use purr_graphics_software::SoftwareBackend;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use panther_shell::{PointerPosition, Shell, ShellRegion};

/// Fixed target the test paints into.
///
/// The extent is large enough that every chosen region center falls well inside
/// its rectangle after edge rounding. The format matches the window path so the
/// test mirrors the real channel order (D7).
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
const FORMAT: TextureFormatClass = TextureFormatClass::Bgra8Unorm;

fn extent() -> Extent2d {
    Extent2d::new(WIDTH, HEIGHT)
}

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
        extent: extent(),
        format: FORMAT,
        alpha_mode: AlphaMode::Opaque,
    }
}

fn frame(surface: SurfaceIdentity, commands: Vec<DrawCommand>) -> FrameSubmission {
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
        commands,
    }
}

fn center(rect: Rect) -> PointerPosition {
    PointerPosition::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Quantizes one color channel the same way the deterministic backend does.
///
/// The value is clamped to `[0.0, 1.0]`, scaled by 255, and rounded half away
/// from zero by adding 0.5 before the truncating cast.
fn quantize_channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Expected `Bgra8Unorm` texel bytes for a color.
///
/// The channel order matches the target format the window uses: blue, green,
/// red, then alpha.
fn expected_texel(color: Color) -> [u8; 4] {
    [
        quantize_channel(color.b),
        quantize_channel(color.g),
        quantize_channel(color.r),
        quantize_channel(color.a),
    ]
}

/// Reads the center texel of a region from the framebuffer.
///
/// The framebuffer holds tightly packed rows in top-to-bottom order, four bytes
/// per texel. The center pixel is interior to every chosen region, so it is not
/// overpainted by a later region in the fixed paint order.
fn center_texel(framebuffer: &[u8], rect: Rect) -> [u8; 4] {
    let width = WIDTH as usize;
    let x = (rect.x + rect.width / 2.0) as usize;
    let y = (rect.y + rect.height / 2.0) as usize;
    let offset = (y * width + x) * 4;

    [
        framebuffer[offset],
        framebuffer[offset + 1],
        framebuffer[offset + 2],
        framebuffer[offset + 3],
    ]
}

#[test]
fn shell_commands_render_expected_region_pixels() {
    let mut backend = SoftwareBackend::create(BackendKind::Software).expect("software backend");
    let surface = backend
        .create_presentation_target(
            WindowSurface::new(&HeadlessWindow, extent()),
            presentation_descriptor(),
        )
        .expect("target creation succeeds");

    let mut shell = Shell::new(extent());
    shell.pointer_moved(center(shell.layout().rect(ShellRegion::AddressField)));
    shell.pointer_pressed(center(shell.layout().rect(ShellRegion::Viewport)));

    assert_eq!(shell.hovered(), Some(ShellRegion::AddressField));
    assert_eq!(shell.focused(), Some(ShellRegion::Viewport));

    let submission = frame(surface, shell.build_commands());
    backend
        .submit(surface, &submission)
        .expect("submit succeeds");
    let framebuffer = backend.read_framebuffer(surface).expect("framebuffer");

    let base = ShellRegion::NavigationBack;
    let base_rect = shell.layout().rect(base);
    assert_eq!(
        center_texel(framebuffer, base_rect),
        expected_texel(base.display_color(false, false)),
        "a region with no interaction keeps its base color"
    );

    let hovered_rect = shell.layout().rect(ShellRegion::AddressField);
    assert_eq!(
        center_texel(framebuffer, hovered_rect),
        expected_texel(ShellRegion::AddressField.display_color(true, false)),
        "the hovered region shows the hover color"
    );

    let focused_rect = shell.layout().rect(ShellRegion::Viewport);
    assert_eq!(
        center_texel(framebuffer, focused_rect),
        expected_texel(ShellRegion::Viewport.display_color(false, true)),
        "the focused region shows the focus color"
    );
}
