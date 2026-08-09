// @file engines/purr/graphics/src/descriptor.rs
// @description Defines backend-neutral resource and pipeline descriptor types.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Backend-neutral resource and pipeline descriptors.
//!
//! Descriptors are the vocabulary the submission seam and both backends share.
//! They express web and compositor semantics (format class, color space,
//! dimensions), never a backend enum. No `wgpu` or native format type appears
//! here.
//!
//! Dimensions can originate from untrusted content in later phases, so the
//! texture descriptor supports an explicit bounds check before any backend
//! allocates from it.

use crate::graphics_error::GraphicsError;

/// M0 upper bound for a texture dimension, in texels.
///
/// The bound applies to width and height independently. It matches the texture
/// dimension both backends guarantee at M0. A descriptor above this bound is
/// rejected before any allocation.
pub const MAX_TEXTURE_EXTENT: u32 = 8192;

/// Two-dimensional size in texels.
///
/// A plain size pair with no invariant of its own. The texture descriptor that
/// carries it enforces the M0 bound through `validate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Extent2d {
    pub width: u32,
    pub height: u32,
}

impl Extent2d {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// Format class of a texture or render target.
///
/// A closed, minimal M0 set. It names a web-visible format class, not a backend
/// texture format. `R8Unorm` is a single-channel format: the glyph atlas uses it
/// to store one coverage byte per texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormatClass {
    Rgba8Unorm,
    Bgra8Unorm,
    Rgba8UnormSrgb,
    R8Unorm,
}

impl TextureFormatClass {
    /// Bytes one texel of this format occupies.
    ///
    /// The four-channel classes are four bytes; the single-channel `R8Unorm` is
    /// one byte. The exhaustive match forces a review when a new format class is
    /// added.
    pub const fn bytes_per_texel(self) -> u32 {
        match self {
            TextureFormatClass::Rgba8Unorm
            | TextureFormatClass::Bgra8Unorm
            | TextureFormatClass::Rgba8UnormSrgb => 4,
            TextureFormatClass::R8Unorm => 1,
        }
    }
}

/// Working color space of a resource.
///
/// Reserved for later color work. At M0 the default working space is `Srgb`.
/// No HDR or wide-gamut space exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorSpace {
    #[default]
    Srgb,
    LinearSrgb,
}

/// Alpha interpretation of a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlphaMode {
    Opaque,
    Premultiplied,
}

/// Intended use of a buffer.
///
/// A closed M0 set. The backend maps each use to its own buffer usage flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BufferUsage {
    Vertex,
    Index,
    Uniform,
}

/// Fixed M0 pipeline set.
///
/// The interface exposes exactly these pipelines at M0. `SolidColor` fills a
/// primitive with one color. `TexturedQuad` samples a texture over a quad. The
/// set is closed; a new pipeline requires an interface change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineKind {
    SolidColor,
    TexturedQuad,
}

/// Color in the working color space.
///
/// Each channel is a component in the range `[0.0, 1.0]`. Components are not
/// clamped here; a backend defines the behavior for an out-of-range component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Creates a color from four channel components.
    ///
    /// Each component is in the range `[0.0, 1.0]` in the working color space.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

/// Describes a texture resource.
///
/// The `label` is an optional, short, human-readable name for diagnostics only.
/// It is never security-sensitive and never affects behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureDescriptor {
    pub extent: Extent2d,
    pub format: TextureFormatClass,
    pub color_space: ColorSpace,
    pub alpha_mode: AlphaMode,
    pub label: Option<String>,
}

impl TextureDescriptor {
    /// Rejects a zero or over-bound extent.
    ///
    /// A zero width or height is invalid. A width or height above
    /// `MAX_TEXTURE_EXTENT` is rejected before any backend allocates from the
    /// descriptor. The check compares against the fixed bound and allocates
    /// nothing.
    pub fn validate(&self) -> Result<(), GraphicsError> {
        if self.extent.width == 0 || self.extent.height == 0 {
            return Err(GraphicsError::InvalidDescriptor);
        }

        if self.extent.width > MAX_TEXTURE_EXTENT || self.extent.height > MAX_TEXTURE_EXTENT {
            return Err(GraphicsError::InvalidDescriptor);
        }

        Ok(())
    }
}

/// Describes a buffer resource.
///
/// The `label` is an optional, short, human-readable name for diagnostics only.
/// It is never security-sensitive and never affects behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferDescriptor {
    pub size_bytes: u64,
    pub usage: BufferUsage,
    pub label: Option<String>,
}

/// Describes an offscreen render target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderTargetDescriptor {
    pub extent: Extent2d,
    pub format: TextureFormatClass,
    pub color_space: ColorSpace,
}

/// Describes the presentation target the compositor presents to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentationTargetDescriptor {
    pub extent: Extent2d,
    pub format: TextureFormatClass,
    pub alpha_mode: AlphaMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor_with_extent(width: u32, height: u32) -> TextureDescriptor {
        TextureDescriptor {
            extent: Extent2d::new(width, height),
            format: TextureFormatClass::Rgba8Unorm,
            color_space: ColorSpace::Srgb,
            alpha_mode: AlphaMode::Premultiplied,
            label: None,
        }
    }

    #[test]
    fn normal_extent_validates() {
        let descriptor = descriptor_with_extent(1280, 720);

        assert_eq!(descriptor.validate(), Ok(()));
    }

    #[test]
    fn zero_width_is_rejected() {
        let descriptor = descriptor_with_extent(0, 720);

        assert_eq!(descriptor.validate(), Err(GraphicsError::InvalidDescriptor));
    }

    #[test]
    fn zero_height_is_rejected() {
        let descriptor = descriptor_with_extent(1280, 0);

        assert_eq!(descriptor.validate(), Err(GraphicsError::InvalidDescriptor));
    }

    #[test]
    fn max_extent_validates() {
        let descriptor = descriptor_with_extent(MAX_TEXTURE_EXTENT, MAX_TEXTURE_EXTENT);

        assert_eq!(descriptor.validate(), Ok(()));
    }

    #[test]
    fn over_bound_extent_is_rejected() {
        let descriptor = descriptor_with_extent(MAX_TEXTURE_EXTENT + 1, 720);

        assert_eq!(descriptor.validate(), Err(GraphicsError::InvalidDescriptor));
    }

    #[test]
    fn color_space_default_is_srgb() {
        assert_eq!(ColorSpace::default(), ColorSpace::Srgb);
    }
}
