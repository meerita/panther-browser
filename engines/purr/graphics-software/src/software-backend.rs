// @file engines/purr/graphics-software/src/software-backend.rs
// @description Implements the deterministic software graphics backend on the CPU.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Deterministic software graphics backend.
//!
//! `SoftwareBackend` implements the Panther `GraphicsBackend` contract with CPU
//! framebuffers. It allocates a byte buffer per presentation target and per
//! texture from a validated descriptor, applies resource uploads, and rasterizes
//! the fixed M0 command set (`Clear`, `FillRect`, `TexturedQuad`) into the target
//! buffer during `submit`. `present` marks the buffer presented; there is no
//! display output at M0.
//!
//! # Determinism contract
//!
//! Identical inputs must produce identical bytes on every platform.
//!
//! * Rounding rule: round half away from zero. It applies to the color-channel
//!   quantization from the working range `[0.0, 1.0]` to an 8-bit component, and
//!   to every rectangle edge converted from logical pixels to an integer pixel
//!   boundary. A tie rounds away from zero.
//! * No platform-dependent floating point in the raster path. Every per-pixel
//!   operation (clipping, addressing, nearest-neighbor sampling, compositing) is
//!   integer arithmetic. The only floating-point work is the bounded
//!   quantization of colors and edges, done once per command with the basic
//!   IEEE-754 operations (multiply, add, compare, truncating cast) that are
//!   identical on every target. The path uses no transcendental function, no
//!   sRGB transfer function, and no fused-multiply-add contraction.
//! * Compositing: straight-alpha source-over, computed with premultiplied integer
//!   blend math in a fixed channel order:
//!   `out = (src * alpha + dst * (255 - alpha) + 127) / 255`. The `+ 127` rounds
//!   the division to nearest. `Clear` replaces every texel; `FillRect` and
//!   `TexturedQuad` composite, so a fully opaque source replaces and a translucent
//!   source blends.
//! * Color space: the backend performs no color-space conversion at M0. A
//!   component is stored directly in the target format channel order.

use std::collections::HashMap;

use purr_graphics::{
    BackendKind, Color, DeviceGeneration, DrawCommand, Extent2d, FrameSubmission,
    GpuResourceIdentity, GraphicsBackend, GraphicsError, PresentationTargetDescriptor,
    ProducerNamespace, Rect, ResourceGeneration, ResourceId, ResourceKind, ResourceUpload,
    SurfaceGeneration, SurfaceId, SurfaceIdentity, TextureDescriptor, TextureFormatClass,
    WindowSurface,
};

/// Producer namespace this backend stamps on the identities it creates.
///
/// A single-process backend has one producer, so the namespace is fixed.
const BACKEND_NAMESPACE: u32 = 1;

/// Bytes one texel occupies for the M0 format set. Every M0 format is four
/// bytes; `channel_order` guards the assumption with an exhaustive match.
const BYTES_PER_TEXEL: u32 = 4;

/// One CPU presentation target owned by the backend.
///
/// The `pixels` buffer holds tightly packed rows in top-to-bottom order, four
/// bytes per texel, in the target format channel order. The allocation is reused
/// across frames; a `Clear` command fills it in place.
#[derive(Debug)]
struct SoftwareFramebuffer {
    extent: Extent2d,
    order: [usize; 4],
    pixels: Vec<u8>,
    presented: bool,
}

/// One CPU texture owned by the backend.
///
/// The extent is kept so a `TexturedQuad` can map a source rectangle in texels
/// and clamp a sample to the texture edge.
#[derive(Debug)]
struct SoftwareTexture {
    extent: Extent2d,
    order: [usize; 4],
    pixels: Vec<u8>,
}

/// Software backend that rasterizes the M0 command set on the CPU.
#[derive(Debug)]
pub struct SoftwareBackend {
    device_generation: DeviceGeneration,
    targets: HashMap<SurfaceIdentity, SoftwareFramebuffer>,
    resources: HashMap<GpuResourceIdentity, SoftwareTexture>,
    next_surface_id: u64,
    next_resource_id: u64,
}

impl SoftwareBackend {
    /// Borrows the pixel buffer of a presentation target for inspection.
    ///
    /// The bytes are tightly packed rows in top-to-bottom order, four bytes per
    /// texel, in the target format channel order. Returns `None` when the
    /// identity does not name a live target. This accessor exposes no backend or
    /// native type; it supports deterministic pixel inspection.
    pub fn read_framebuffer(&self, surface: SurfaceIdentity) -> Option<&[u8]> {
        self.targets
            .get(&surface)
            .map(|target| target.pixels.as_slice())
    }

    /// Reports whether the target was presented since its last submission.
    ///
    /// Returns `None` when the identity does not name a live target.
    pub fn is_presented(&self, surface: SurfaceIdentity) -> Option<bool> {
        self.targets.get(&surface).map(|target| target.presented)
    }

    /// Returns the next surface identity and advances the counter.
    fn next_surface_identity(&mut self) -> SurfaceIdentity {
        let surface_id = self.next_surface_id;
        self.next_surface_id = self.next_surface_id.saturating_add(1);

        SurfaceIdentity::new(
            SurfaceId::new(surface_id),
            SurfaceGeneration::new(1),
            ProducerNamespace::new(BACKEND_NAMESPACE),
        )
    }

    /// Returns the next resource identity for the current device generation.
    fn next_resource_identity(&mut self) -> GpuResourceIdentity {
        let resource_id = self.next_resource_id;
        self.next_resource_id = self.next_resource_id.saturating_add(1);

        GpuResourceIdentity::new(
            ProducerNamespace::new(BACKEND_NAMESPACE),
            ResourceId::new(resource_id),
            ResourceGeneration::new(1),
            ResourceKind::Texture,
            self.device_generation,
        )
    }

    /// Rejects a submission that references a missing resource, a stale device
    /// generation, or an upload whose extent does not match its texture.
    ///
    /// A generation mismatch means the resource belongs to a device the backend
    /// no longer owns, so it is reported as a device loss. Every check runs before
    /// any upload or draw mutates state.
    fn validate_resources(&self, submission: &FrameSubmission) -> Result<(), GraphicsError> {
        for upload in &submission.uploads {
            self.check_generation(upload.resource)?;
            let texture = self
                .resources
                .get(&upload.resource)
                .ok_or(GraphicsError::ResourceNotFound)?;
            if upload.descriptor.extent != texture.extent {
                return Err(GraphicsError::InvalidDescriptor);
            }
        }

        for command in &submission.commands {
            if let DrawCommand::TexturedQuad { texture, .. } = command {
                self.check_generation(*texture)?;
                if !self.resources.contains_key(texture) {
                    return Err(GraphicsError::ResourceNotFound);
                }
            }
        }

        Ok(())
    }

    /// Returns a device-loss error when an identity carries a stale device
    /// generation.
    fn check_generation(&self, identity: GpuResourceIdentity) -> Result<(), GraphicsError> {
        if identity.device_generation() != self.device_generation {
            return Err(GraphicsError::DeviceLost);
        }
        Ok(())
    }

    /// Copies each upload into its target texture.
    ///
    /// The extent match and the pixel length are validated before this runs, so
    /// the copy addresses the whole texture. The length is re-checked as a
    /// defensive guard before the copy.
    fn apply_uploads(&mut self, uploads: &[ResourceUpload]) -> Result<(), GraphicsError> {
        for upload in uploads {
            let texture = self
                .resources
                .get_mut(&upload.resource)
                .ok_or(GraphicsError::ResourceNotFound)?;
            if upload.pixels.len() != texture.pixels.len() {
                return Err(GraphicsError::InvalidDescriptor);
            }
            texture.pixels.copy_from_slice(&upload.pixels);
        }

        Ok(())
    }
}

impl GraphicsBackend for SoftwareBackend {
    fn create(selection: BackendKind) -> Result<Self, GraphicsError> {
        if selection != BackendKind::Software {
            return Err(GraphicsError::Unsupported);
        }

        Ok(Self {
            device_generation: DeviceGeneration::new(1),
            targets: HashMap::new(),
            resources: HashMap::new(),
            next_surface_id: 1,
            next_resource_id: 1,
        })
    }

    fn device_generation(&self) -> DeviceGeneration {
        self.device_generation
    }

    fn create_presentation_target(
        &mut self,
        _surface: WindowSurface<'_>,
        descriptor: PresentationTargetDescriptor,
    ) -> Result<SurfaceIdentity, GraphicsError> {
        validate_extent(descriptor.extent)?;
        let length = buffer_length(descriptor.extent)?;
        let order = channel_order(descriptor.format);

        let identity = self.next_surface_identity();
        self.targets.insert(
            identity,
            SoftwareFramebuffer {
                extent: descriptor.extent,
                order,
                pixels: vec![0u8; length],
                presented: false,
            },
        );

        Ok(identity)
    }

    fn resize_presentation_target(
        &mut self,
        surface: SurfaceIdentity,
        extent: Extent2d,
    ) -> Result<(), GraphicsError> {
        validate_extent(extent)?;
        let length = buffer_length(extent)?;

        let target = self
            .targets
            .get_mut(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;

        target.extent = extent;
        target.pixels.clear();
        target.pixels.resize(length, 0);
        target.presented = false;

        Ok(())
    }

    fn allocate_texture(
        &mut self,
        descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, GraphicsError> {
        descriptor.validate()?;
        let length = buffer_length(descriptor.extent)?;
        let order = channel_order(descriptor.format);

        let identity = self.next_resource_identity();
        self.resources.insert(
            identity,
            SoftwareTexture {
                extent: descriptor.extent,
                order,
                pixels: vec![0u8; length],
            },
        );

        Ok(identity)
    }

    fn submit(
        &mut self,
        surface: SurfaceIdentity,
        submission: &FrameSubmission,
    ) -> Result<(), GraphicsError> {
        submission.validate()?;
        self.validate_resources(submission)?;

        if !self.targets.contains_key(&surface) {
            return Err(GraphicsError::ResourceNotFound);
        }

        self.apply_uploads(&submission.uploads)?;

        let resources = &self.resources;
        let target = self
            .targets
            .get_mut(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;
        render_commands(target, resources, &submission.commands)?;
        target.presented = false;

        Ok(())
    }

    fn present(&mut self, surface: SurfaceIdentity) -> Result<(), GraphicsError> {
        let target = self
            .targets
            .get_mut(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;
        target.presented = true;
        Ok(())
    }
}

/// Rejects a zero or over-bound extent.
///
/// A zero dimension cannot allocate a target, and a dimension above the M0
/// texture bound is refused before any allocation.
fn validate_extent(extent: Extent2d) -> Result<(), GraphicsError> {
    if extent.width == 0 || extent.height == 0 {
        return Err(GraphicsError::InvalidDescriptor);
    }

    if extent.width > purr_graphics::MAX_TEXTURE_EXTENT
        || extent.height > purr_graphics::MAX_TEXTURE_EXTENT
    {
        return Err(GraphicsError::InvalidDescriptor);
    }

    Ok(())
}

/// Returns the tightly packed byte length of a four-byte-per-texel buffer.
///
/// The extent passed validation, so width and height are within the M0 bound.
/// The product is still computed with checked arithmetic as a defensive guard,
/// and the result must fit `usize` to allocate.
fn buffer_length(extent: Extent2d) -> Result<usize, GraphicsError> {
    let bytes = u64::from(extent.width)
        .checked_mul(u64::from(extent.height))
        .and_then(|texels| texels.checked_mul(u64::from(BYTES_PER_TEXEL)))
        .ok_or(GraphicsError::InvalidDescriptor)?;

    usize::try_from(bytes).map_err(|_| GraphicsError::InvalidDescriptor)
}

/// Runs the command list against the target framebuffer in order.
fn render_commands(
    target: &mut SoftwareFramebuffer,
    resources: &HashMap<GpuResourceIdentity, SoftwareTexture>,
    commands: &[DrawCommand],
) -> Result<(), GraphicsError> {
    for command in commands {
        match command {
            DrawCommand::Clear { color } => clear_target(target, *color),
            DrawCommand::FillRect { rect, color } => fill_rect(target, *rect, *color),
            DrawCommand::TexturedQuad {
                rect,
                texture,
                source,
            } => {
                let texture = resources
                    .get(texture)
                    .ok_or(GraphicsError::ResourceNotFound)?;
                textured_quad(target, texture, *rect, *source);
            }
        }
    }

    Ok(())
}

/// Fills the whole target with one color, replacing every texel.
///
/// `Clear` does not composite; it writes the quantized color into every texel in
/// the target format order. The fill runs in place over the reused allocation.
fn clear_target(target: &mut SoftwareFramebuffer, color: Color) {
    let rgba = quantize_color(color);
    let mut texel = [0u8; 4];
    write_rgba(&mut texel, target.order, rgba);

    for chunk in target.pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&texel);
    }
}

/// Composites a solid color over a clipped rectangle.
///
/// The edges are rounded to integer pixel boundaries and clipped to the target.
/// Each covered texel is composited with straight-alpha source-over.
fn fill_rect(target: &mut SoftwareFramebuffer, rect: Rect, color: Color) {
    let rgba = quantize_color(color);
    let Some(bounds) = clip_rect(rect_to_pixels(rect), target.extent) else {
        return;
    };

    let width = target.extent.width as usize;
    let order = target.order;
    for y in bounds.y0..bounds.y1 {
        let row = y * width;
        for x in bounds.x0..bounds.x1 {
            let offset = (row + x) * 4;
            let texel = &mut target.pixels[offset..offset + 4];
            let dst = read_rgba(texel, order);
            let out = over_rgba(rgba, dst);
            write_rgba(texel, order, out);
        }
    }
}

/// Composites a texture region over a clipped destination rectangle.
///
/// The destination is rounded and clipped like a fill. The source region is
/// rounded to integer texels. Sampling is nearest neighbor computed in integer
/// arithmetic: a destination offset maps to a source offset by
/// `source_start + offset * source_span / destination_span`, so the mapping is
/// identical on every platform. A source coordinate outside the texture is
/// clamped to the edge, matching clamp-to-edge addressing. A degenerate source or
/// destination rectangle draws nothing.
fn textured_quad(
    target: &mut SoftwareFramebuffer,
    texture: &SoftwareTexture,
    rect: Rect,
    source: Rect,
) {
    let dest = rect_to_pixels(rect);
    let dest_width = dest.x1 - dest.x0;
    let dest_height = dest.y1 - dest.y0;
    if dest_width <= 0 || dest_height <= 0 {
        return;
    }

    let src = rect_to_pixels(source);
    let source_width = src.x1 - src.x0;
    let source_height = src.y1 - src.y0;
    if source_width <= 0 || source_height <= 0 {
        return;
    }

    let Some(bounds) = clip_rect(dest, target.extent) else {
        return;
    };

    let target_width = target.extent.width as usize;
    let target_order = target.order;
    let texture_width = i64::from(texture.extent.width);
    let texture_height = i64::from(texture.extent.height);
    let texture_stride = texture.extent.width as usize;
    let texture_order = texture.order;

    for y in bounds.y0..bounds.y1 {
        let local_y = y as i64 - dest.y0;
        let sample_y = clamp_index(
            src.y0 + local_y * source_height / dest_height,
            texture_height,
        );
        let texture_row = sample_y as usize * texture_stride;
        let target_row = y * target_width;
        for x in bounds.x0..bounds.x1 {
            let local_x = x as i64 - dest.x0;
            let sample_x = clamp_index(src.x0 + local_x * source_width / dest_width, texture_width);

            let source_offset = (texture_row + sample_x as usize) * 4;
            let source_texel = &texture.pixels[source_offset..source_offset + 4];
            let source_rgba = read_rgba(source_texel, texture_order);

            let target_offset = (target_row + x) * 4;
            let target_texel = &mut target.pixels[target_offset..target_offset + 4];
            let dst_rgba = read_rgba(target_texel, target_order);
            let out = over_rgba(source_rgba, dst_rgba);
            write_rgba(target_texel, target_order, out);
        }
    }
}

/// Integer pixel rectangle with half-open bounds `[x0, x1) x [y0, y1)`.
#[derive(Clone, Copy)]
struct PixelRect {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

/// Clipped half-open pixel rectangle inside a target extent.
struct ClippedRect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

/// Rounds a rectangle to a half-open integer pixel rectangle.
///
/// Each edge is rounded once with the round-half-away-from-zero rule. The
/// addition of the width and height happens before rounding, so the two edges of
/// an axis round independently.
fn rect_to_pixels(rect: Rect) -> PixelRect {
    PixelRect {
        x0: round_edge(rect.x),
        y0: round_edge(rect.y),
        x1: round_edge(rect.x + rect.width),
        y1: round_edge(rect.y + rect.height),
    }
}

/// Clips an integer pixel rectangle to the target extent.
///
/// Returns `None` when the rectangle is empty or lies fully outside the target,
/// so the caller draws nothing. The clip uses `i64` bounds, so a negative or
/// oversized edge is handled without wrapping before the cast to `usize`.
fn clip_rect(rect: PixelRect, extent: Extent2d) -> Option<ClippedRect> {
    let max_x = i64::from(extent.width);
    let max_y = i64::from(extent.height);

    let x0 = rect.x0.clamp(0, max_x);
    let y0 = rect.y0.clamp(0, max_y);
    let x1 = rect.x1.clamp(0, max_x);
    let y1 = rect.y1.clamp(0, max_y);

    if x0 >= x1 || y0 >= y1 {
        return None;
    }

    Some(ClippedRect {
        x0: x0 as usize,
        y0: y0 as usize,
        x1: x1 as usize,
        y1: y1 as usize,
    })
}

/// Rounds a rectangle edge in logical pixels to an integer pixel boundary.
///
/// The rule is round half away from zero, expressed with basic operations so the
/// result is identical on every platform: a nonnegative edge adds 0.5 and a
/// negative edge subtracts 0.5, then the truncating cast drops the fraction. The
/// cast saturates, so an out-of-range or `NaN` edge maps to a bounded value that
/// clipping then rejects.
fn round_edge(value: f32) -> i64 {
    if value >= 0.0 {
        (value + 0.5) as i64
    } else {
        (value - 0.5) as i64
    }
}

/// Clamps a source coordinate to the valid texel range of a texture axis.
///
/// A coordinate below zero maps to the first texel and a coordinate at or above
/// the extent maps to the last texel, matching clamp-to-edge addressing.
fn clamp_index(coordinate: i64, extent: i64) -> i64 {
    coordinate.clamp(0, extent - 1)
}

/// Quantizes the four channels of a color to 8-bit components.
fn quantize_color(color: Color) -> [u8; 4] {
    [
        quantize_channel(color.r),
        quantize_channel(color.g),
        quantize_channel(color.b),
        quantize_channel(color.a),
    ]
}

/// Quantizes one color channel from the working range to an 8-bit component.
///
/// The value is clamped to `[0.0, 1.0]` and scaled by 255. The round-half-away-
/// from-zero rule adds 0.5 before the truncating cast, so a tie rounds up. This
/// runs once per command, never per pixel.
fn quantize_channel(value: f32) -> u8 {
    let clamped = value.clamp(0.0, 1.0);
    (clamped * 255.0 + 0.5) as u8
}

/// Composites a straight-alpha source over a destination in canonical RGBA.
///
/// The color channels blend by the source alpha; the alpha channel follows the
/// source-over rule `out_a = src_a + dst_a * (255 - src_a) / 255`.
fn over_rgba(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let alpha = src[3];
    [
        over_channel(src[0], dst[0], alpha),
        over_channel(src[1], dst[1], alpha),
        over_channel(src[2], dst[2], alpha),
        over_alpha(alpha, dst[3]),
    ]
}

/// Composites one straight-alpha color channel over a destination channel.
///
/// The blend is premultiplied source-over in a fixed order:
/// `out = (src * alpha + dst * (255 - alpha) + 127) / 255`. The `+ 127` rounds
/// the integer division to nearest. An opaque source (`alpha == 255`) returns the
/// source exactly.
fn over_channel(src: u8, dst: u8, alpha: u8) -> u8 {
    let src = u32::from(src);
    let dst = u32::from(dst);
    let alpha = u32::from(alpha);
    let value = src * alpha + dst * (255 - alpha) + 127;
    (value / 255) as u8
}

/// Composites the source alpha over the destination alpha.
///
/// `out = (src * 255 + dst * (255 - src) + 127) / 255`, the source-over rule for
/// the alpha channel with rounded integer division.
fn over_alpha(src: u8, dst: u8) -> u8 {
    let src = u32::from(src);
    let dst = u32::from(dst);
    let value = src * 255 + dst * (255 - src) + 127;
    (value / 255) as u8
}

/// Byte position of each canonical RGBA channel within a four-byte texel.
///
/// The returned array maps a canonical index (0=R, 1=G, 2=B, 3=A) to a byte
/// offset. Every M0 format is four bytes; the sRGB class shares the byte layout
/// of its linear sibling because the transfer function does not reorder channels.
/// The exhaustive match forces a review when a new format class is added.
fn channel_order(format: TextureFormatClass) -> [usize; 4] {
    match format {
        TextureFormatClass::Rgba8Unorm | TextureFormatClass::Rgba8UnormSrgb => [0, 1, 2, 3],
        TextureFormatClass::Bgra8Unorm => [2, 1, 0, 3],
    }
}

/// Reads a texel into canonical RGBA order.
fn read_rgba(texel: &[u8], order: [usize; 4]) -> [u8; 4] {
    [
        texel[order[0]],
        texel[order[1]],
        texel[order[2]],
        texel[order[3]],
    ]
}

/// Writes canonical RGBA into a texel in the format channel order.
fn write_rgba(texel: &mut [u8], order: [usize; 4], rgba: [u8; 4]) {
    texel[order[0]] = rgba[0];
    texel[order[1]] = rgba[1];
    texel[order[2]] = rgba[2];
    texel[order[3]] = rgba[3];
}

#[cfg(test)]
mod tests {
    use super::*;
    use purr_graphics::{
        AlphaMode, ColorSpace, FrameToken, MAX_TEXTURE_EXTENT, SceneGeneration, SceneId,
        SceneIdentity,
    };
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    };

    /// Window stand-in that reports no handle.
    ///
    /// The software backend never reads the handle, so an `Unavailable` result
    /// keeps the test free of any real platform handle and free of `unsafe`.
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

    fn presentation_descriptor(
        width: u32,
        height: u32,
        format: TextureFormatClass,
    ) -> PresentationTargetDescriptor {
        PresentationTargetDescriptor {
            extent: Extent2d::new(width, height),
            format,
            alpha_mode: AlphaMode::Opaque,
        }
    }

    fn texture_descriptor(
        width: u32,
        height: u32,
        format: TextureFormatClass,
    ) -> TextureDescriptor {
        TextureDescriptor {
            extent: Extent2d::new(width, height),
            format,
            color_space: ColorSpace::Srgb,
            alpha_mode: AlphaMode::Premultiplied,
            label: None,
        }
    }

    fn make_target(
        backend: &mut SoftwareBackend,
        width: u32,
        height: u32,
        format: TextureFormatClass,
    ) -> SurfaceIdentity {
        backend
            .create_presentation_target(
                WindowSurface::new(&HeadlessWindow, Extent2d::new(width, height)),
                presentation_descriptor(width, height, format),
            )
            .expect("target creation succeeds")
    }

    fn submission(
        surface: SurfaceIdentity,
        format: TextureFormatClass,
        uploads: Vec<ResourceUpload>,
        commands: Vec<DrawCommand>,
    ) -> FrameSubmission {
        FrameSubmission {
            frame_token: FrameToken::new(1),
            scene: SceneIdentity::new(
                SceneId::new(1),
                SceneGeneration::new(1),
                surface.surface_id(),
                surface.surface_generation(),
            ),
            target: presentation_descriptor(1, 1, format),
            uploads,
            commands,
        }
    }

    #[test]
    fn create_rejects_hardware_selection() {
        assert_eq!(
            SoftwareBackend::create(BackendKind::Hardware).unwrap_err(),
            GraphicsError::Unsupported
        );
    }

    #[test]
    fn round_edge_rounds_half_away_from_zero() {
        assert_eq!(round_edge(0.5), 1);
        assert_eq!(round_edge(1.5), 2);
        assert_eq!(round_edge(2.4), 2);
        assert_eq!(round_edge(-0.5), -1);
        assert_eq!(round_edge(-1.5), -2);
        assert_eq!(round_edge(-0.4), 0);
    }

    #[test]
    fn quantize_channel_rounds_half_up() {
        assert_eq!(quantize_channel(0.0), 0);
        assert_eq!(quantize_channel(1.0), 255);
        assert_eq!(quantize_channel(0.5), 128);
        assert_eq!(quantize_channel(2.0), 255);
        assert_eq!(quantize_channel(-1.0), 0);
    }

    #[test]
    fn clear_fills_uniform_color() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);

        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            Vec::new(),
            vec![DrawCommand::Clear {
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            }],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        let expected: Vec<u8> = [255, 0, 0, 255].repeat(4);
        assert_eq!(backend.read_framebuffer(surface), Some(expected.as_slice()));
    }

    #[test]
    fn fill_rect_fills_clipped_rectangle_and_leaves_rest() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 3, 3, TextureFormatClass::Rgba8Unorm);

        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            Vec::new(),
            vec![
                DrawCommand::Clear {
                    color: Color::new(0.0, 0.0, 0.0, 1.0),
                },
                DrawCommand::FillRect {
                    rect: Rect::new(1.0, 1.0, 1.0, 1.0),
                    color: Color::new(1.0, 0.0, 0.0, 1.0),
                },
            ],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        let offset = |x: usize, y: usize, width: usize| (y * width + x) * 4;
        let mut expected: Vec<u8> = [0, 0, 0, 255].repeat(9);
        let center = offset(1, 1, 3);
        expected[center..center + 4].copy_from_slice(&[255, 0, 0, 255]);

        assert_eq!(backend.read_framebuffer(surface), Some(expected.as_slice()));
    }

    #[test]
    fn fill_rect_clips_to_target_bounds() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);

        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            Vec::new(),
            vec![
                DrawCommand::Clear {
                    color: Color::new(0.0, 0.0, 0.0, 1.0),
                },
                DrawCommand::FillRect {
                    rect: Rect::new(1.0, 1.0, 100.0, 100.0),
                    color: Color::new(0.0, 1.0, 0.0, 1.0),
                },
            ],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        let offset = |x: usize, y: usize, width: usize| (y * width + x) * 4;
        let mut expected: Vec<u8> = [0, 0, 0, 255].repeat(4);
        let corner = offset(1, 1, 2);
        expected[corner..corner + 4].copy_from_slice(&[0, 255, 0, 255]);

        assert_eq!(backend.read_framebuffer(surface), Some(expected.as_slice()));
    }

    #[test]
    fn textured_quad_blits_source_bytes() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);
        let texture = backend
            .allocate_texture(&texture_descriptor(2, 2, TextureFormatClass::Rgba8Unorm))
            .expect("valid descriptor allocates");

        let source_pixels: Vec<u8> = vec![
            255, 0, 0, 255, // (0,0)
            0, 255, 0, 255, // (1,0)
            0, 0, 255, 255, // (0,1)
            255, 255, 0, 255, // (1,1)
        ];
        let upload = ResourceUpload {
            resource: texture,
            descriptor: texture_descriptor(2, 2, TextureFormatClass::Rgba8Unorm),
            pixels: source_pixels.clone(),
        };

        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            vec![upload],
            vec![DrawCommand::TexturedQuad {
                rect: Rect::new(0.0, 0.0, 2.0, 2.0),
                texture,
                source: Rect::new(0.0, 0.0, 2.0, 2.0),
            }],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        assert_eq!(
            backend.read_framebuffer(surface),
            Some(source_pixels.as_slice())
        );
    }

    #[test]
    fn textured_quad_upscales_with_nearest_neighbor() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);
        let texture = backend
            .allocate_texture(&texture_descriptor(1, 1, TextureFormatClass::Rgba8Unorm))
            .expect("valid descriptor allocates");

        let upload = ResourceUpload {
            resource: texture,
            descriptor: texture_descriptor(1, 1, TextureFormatClass::Rgba8Unorm),
            pixels: vec![10, 20, 30, 255],
        };

        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            vec![upload],
            vec![DrawCommand::TexturedQuad {
                rect: Rect::new(0.0, 0.0, 2.0, 2.0),
                texture,
                source: Rect::new(0.0, 0.0, 1.0, 1.0),
            }],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        let expected: Vec<u8> = [10, 20, 30, 255].repeat(4);
        assert_eq!(backend.read_framebuffer(surface), Some(expected.as_slice()));
    }

    #[test]
    fn textured_quad_reorders_channels_for_target_format() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 1, 1, TextureFormatClass::Bgra8Unorm);
        let texture = backend
            .allocate_texture(&texture_descriptor(1, 1, TextureFormatClass::Rgba8Unorm))
            .expect("valid descriptor allocates");

        let upload = ResourceUpload {
            resource: texture,
            descriptor: texture_descriptor(1, 1, TextureFormatClass::Rgba8Unorm),
            pixels: vec![10, 20, 30, 255],
        };

        let frame = submission(
            surface,
            TextureFormatClass::Bgra8Unorm,
            vec![upload],
            vec![DrawCommand::TexturedQuad {
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                texture,
                source: Rect::new(0.0, 0.0, 1.0, 1.0),
            }],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        assert_eq!(
            backend.read_framebuffer(surface),
            Some([30, 20, 10, 255].as_slice())
        );
    }

    #[test]
    fn fill_rect_blends_translucent_color() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 1, 1, TextureFormatClass::Rgba8Unorm);

        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            Vec::new(),
            vec![
                DrawCommand::Clear {
                    color: Color::new(0.0, 0.0, 0.0, 1.0),
                },
                DrawCommand::FillRect {
                    rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                    color: Color::new(1.0, 1.0, 1.0, 0.5),
                },
            ],
        );
        backend.submit(surface, &frame).expect("submit succeeds");

        let alpha = quantize_channel(0.5);
        let expected_channel = over_channel(255, 0, alpha);
        assert_eq!(
            backend.read_framebuffer(surface),
            Some([expected_channel, expected_channel, expected_channel, 255].as_slice())
        );
    }

    #[test]
    fn submit_rejects_over_bound_command_list() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);

        let commands = vec![
            DrawCommand::Clear {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            };
            purr_graphics::MAX_DRAW_COMMANDS + 1
        ];
        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            Vec::new(),
            commands,
        );

        assert_eq!(
            backend.submit(surface, &frame),
            Err(GraphicsError::SubmissionRejected)
        );
    }

    #[test]
    fn submit_rejects_stale_device_generation() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);

        let stale_device = backend
            .device_generation()
            .next()
            .expect("generation advances");
        let stale_texture = GpuResourceIdentity::new(
            ProducerNamespace::new(BACKEND_NAMESPACE),
            ResourceId::new(1),
            ResourceGeneration::new(1),
            ResourceKind::Texture,
            stale_device,
        );

        let upload = ResourceUpload {
            resource: stale_texture,
            descriptor: texture_descriptor(1, 1, TextureFormatClass::Rgba8Unorm),
            pixels: vec![0u8; 4],
        };
        let frame = submission(
            surface,
            TextureFormatClass::Rgba8Unorm,
            vec![upload],
            Vec::new(),
        );

        assert_eq!(
            backend.submit(surface, &frame),
            Err(GraphicsError::DeviceLost)
        );
    }

    #[test]
    fn create_presentation_target_rejects_zero_extent() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");

        let result = backend.create_presentation_target(
            WindowSurface::new(&HeadlessWindow, Extent2d::new(0, 480)),
            presentation_descriptor(0, 480, TextureFormatClass::Rgba8Unorm),
        );

        assert_eq!(result, Err(GraphicsError::InvalidDescriptor));
    }

    #[test]
    fn allocate_texture_rejects_over_bound_extent() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");

        let result = backend.allocate_texture(&texture_descriptor(
            MAX_TEXTURE_EXTENT + 1,
            16,
            TextureFormatClass::Rgba8Unorm,
        ));

        assert_eq!(result.unwrap_err(), GraphicsError::InvalidDescriptor);
    }

    #[test]
    fn present_marks_the_framebuffer_presented() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);

        assert_eq!(backend.is_presented(surface), Some(false));
        backend.present(surface).expect("present succeeds");
        assert_eq!(backend.is_presented(surface), Some(true));
    }

    #[test]
    fn present_unknown_target_is_rejected() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let unknown = SurfaceIdentity::new(
            SurfaceId::new(99),
            SurfaceGeneration::new(1),
            ProducerNamespace::new(BACKEND_NAMESPACE),
        );

        assert_eq!(
            backend.present(unknown),
            Err(GraphicsError::ResourceNotFound)
        );
    }

    #[test]
    fn resize_reallocates_the_framebuffer() {
        let mut backend =
            SoftwareBackend::create(BackendKind::Software).expect("software backend creates");
        let surface = make_target(&mut backend, 2, 2, TextureFormatClass::Rgba8Unorm);

        backend
            .resize_presentation_target(surface, Extent2d::new(4, 3))
            .expect("resize succeeds");

        assert_eq!(
            backend.read_framebuffer(surface).map(<[u8]>::len),
            Some(4 * 3 * 4)
        );
    }
}
