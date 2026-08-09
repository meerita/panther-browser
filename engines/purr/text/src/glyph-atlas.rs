// @file engines/purr/text/src/glyph-atlas.rs
// @description Packs rasterized glyph masks into one bounded glyph atlas and its ResourceUpload.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Glyph atlas.
//!
//! The atlas packs the grayscale masks of the glyphs one generation needs into a
//! single texture and produces a `purr-graphics` [`ResourceUpload`] with
//! [`ResourceKind::GlyphAtlas`] in the caller-supplied producer namespace. A later
//! paint stage samples this atlas with `TexturedQuad` commands.
//!
//! The atlas maps each glyph key (a glyph index at a pixel size) to a physical
//! source rectangle in the texture. These source rectangles are physical data that
//! stay out of the logical [`crate::text_shaping::GlyphRun`]: the run carries glyph
//! indices and advances, and the atlas alone knows where a glyph's pixels live.
//! Losing the atlas invalidates only these raster resources, not the logical run.
//!
//! Every size is bounded and checked. The atlas dimensions are validated against
//! [`MAX_TEXTURE_EXTENT`], the pixel-buffer length is computed with checked
//! arithmetic and matches the descriptor exactly, and the glyph count is capped, so
//! a request that would exceed a bound fails closed with a typed error and never
//! panics. The atlas is rebuilt per generation; retention and eviction are a later
//! concern.

// Paint (a later phase) is the first non-test consumer of the atlas and its
// accessors. This phase builds the atlas and exercises it through the unit tests
// below, so several accessors are otherwise unused in a non-test build.
#![allow(dead_code)]

use std::collections::HashSet;

use crate::bundled_font::{BundledFont, GlyphIndex};
use crate::glyph_raster::{GlyphMask, RasterError, rasterize_glyph};
use crate::pixel_unit::TextUnit;
use purr_graphics::{
    AlphaMode, ColorSpace, DeviceGeneration, Extent2d, GpuResourceIdentity, MAX_TEXTURE_EXTENT,
    ProducerNamespace, ResourceGeneration, ResourceId, ResourceKind, ResourceUpload,
    TextureDescriptor, TextureFormatClass,
};

/// Transparent border, in texels, kept around every packed glyph.
///
/// The border stops a sampler from bleeding one glyph's coverage into its
/// neighbor at a rectangle edge.
const PADDING: u32 = 1;

/// Starting atlas width, in texels, before it grows to fit the widest glyph.
const DEFAULT_ATLAS_WIDTH: u32 = 256;

/// Upper bound for the number of distinct glyphs one atlas packs.
///
/// The bound caps the rasterization and packing work one atlas build can request
/// before any allocation.
const MAX_ATLAS_GLYPHS: usize = 8_192;

/// Bytes one atlas texel occupies. The atlas is a single-channel coverage mask,
/// so one texel is one coverage byte.
const BYTES_PER_TEXEL: usize = 1;

/// A glyph rendered at a pixel size: the key the atlas packs and looks up by.
///
/// The size is part of the key because the same glyph index at two sizes is two
/// different masks. The key holds no texture coordinate; the atlas maps it to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    glyph: GlyphIndex,
    size: TextUnit,
}

impl GlyphKey {
    pub fn new(glyph: GlyphIndex, size: TextUnit) -> Self {
        Self { glyph, size }
    }

    pub fn glyph(self) -> GlyphIndex {
        self.glyph
    }

    pub fn size(self) -> TextUnit {
        self.size
    }
}

/// A rectangle in atlas texel coordinates.
///
/// The origin is the top-left texel and the size is in texels. A zero-size
/// rectangle marks a blank glyph (such as a space) that occupies no atlas area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Where one glyph lives in the atlas, kept apart from the logical run.
///
/// The placement pairs a glyph key with its physical source rectangle and the
/// device-pixel offset (`left`, `top`) of the mask from the pen origin on the
/// baseline. A later paint stage reads the placement to position and sample the
/// glyph; the logical run never carries any of this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphPlacement {
    key: GlyphKey,
    source: TexelRect,
    left: i32,
    top: i32,
}

impl GlyphPlacement {
    pub fn key(&self) -> GlyphKey {
        self.key
    }

    pub fn source(&self) -> TexelRect {
        self.source
    }

    pub fn left(&self) -> i32 {
        self.left
    }

    pub fn top(&self) -> i32 {
        self.top
    }
}

/// Failure the atlas builder reports.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum GlyphAtlasError {
    #[error("the atlas holds too many glyphs")]
    TooManyGlyphs,
    #[error("the atlas exceeds the maximum texture extent")]
    AtlasTooLarge,
    #[error("a glyph could not be rasterized")]
    Raster(#[from] RasterError),
}

/// One packed glyph atlas: a resource upload and the glyph placements in it.
///
/// The upload carries the caller-supplied [`ResourceKind::GlyphAtlas`] identity, a
/// bounded [`TextureDescriptor`], and tightly packed pixels whose length matches
/// the descriptor. The placements map each glyph key to its physical source
/// rectangle in the texture.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphAtlas {
    upload: ResourceUpload,
    extent: Extent2d,
    placements: Vec<GlyphPlacement>,
}

impl GlyphAtlas {
    /// The resource upload the frame carries for this atlas.
    pub fn upload(&self) -> &ResourceUpload {
        &self.upload
    }

    /// The full identity of the atlas texture.
    pub fn resource(&self) -> GpuResourceIdentity {
        self.upload.resource
    }

    /// The atlas dimensions in texels.
    pub fn extent(&self) -> Extent2d {
        self.extent
    }

    /// The placements of every packed glyph.
    pub fn placements(&self) -> &[GlyphPlacement] {
        &self.placements
    }

    /// The placement of one glyph key, or `None` when the atlas does not hold it.
    pub fn placement(&self, key: GlyphKey) -> Option<&GlyphPlacement> {
        self.placements
            .iter()
            .find(|placement| placement.key == key)
    }
}

/// Builds one glyph atlas from the glyph keys a generation needs.
///
/// Duplicate keys are packed once. The build rasterizes each distinct glyph,
/// shelf-packs the masks into one texture, and produces the upload in the
/// caller-supplied producer namespace with the given resource id and the resource
/// and device generations. It fails closed with a typed error, and never panics,
/// when the glyph count, the atlas extent, or the pixel-buffer length would exceed
/// its bound.
pub fn build_glyph_atlas(
    font: &BundledFont,
    keys: &[GlyphKey],
    producer_namespace: ProducerNamespace,
    resource_id: ResourceId,
    resource_generation: ResourceGeneration,
    device_generation: DeviceGeneration,
) -> Result<GlyphAtlas, GlyphAtlasError> {
    let unique = deduplicate(keys);
    if unique.len() > MAX_ATLAS_GLYPHS {
        return Err(GlyphAtlasError::TooManyGlyphs);
    }

    let mut masks = Vec::with_capacity(unique.len());
    for key in &unique {
        masks.push(rasterize_glyph(font, key.glyph, key.size)?);
    }

    let dimensions: Vec<(u32, u32)> = masks
        .iter()
        .map(|mask| (mask.width(), mask.height()))
        .collect();
    let (extent, rects) = pack_shelves(&dimensions)?;

    let pixels = paint_atlas(extent, &rects, &masks)?;
    let descriptor = TextureDescriptor {
        extent,
        format: TextureFormatClass::R8Unorm,
        color_space: ColorSpace::LinearSrgb,
        alpha_mode: AlphaMode::Premultiplied,
        label: Some("glyph-atlas".to_owned()),
    };
    descriptor
        .validate()
        .map_err(|_| GlyphAtlasError::AtlasTooLarge)?;

    let resource = GpuResourceIdentity::new(
        producer_namespace,
        resource_id,
        resource_generation,
        ResourceKind::GlyphAtlas,
        device_generation,
    );

    let placements = unique
        .iter()
        .zip(rects.iter())
        .zip(masks.iter())
        .map(|((&key, &source), mask)| GlyphPlacement {
            key,
            source,
            left: mask.left(),
            top: mask.top(),
        })
        .collect();

    Ok(GlyphAtlas {
        upload: ResourceUpload {
            resource,
            descriptor,
            pixels,
        },
        extent,
        placements,
    })
}

/// Returns the distinct keys in first-seen order.
fn deduplicate(keys: &[GlyphKey]) -> Vec<GlyphKey> {
    let mut seen = HashSet::with_capacity(keys.len());
    let mut unique = Vec::with_capacity(keys.len());
    for &key in keys {
        if seen.insert(key) {
            unique.push(key);
        }
    }
    unique
}

/// Shelf-packs glyph masks of the given dimensions into one atlas.
///
/// Glyphs are placed left to right on a shelf; when the next glyph does not fit the
/// shelf, a new shelf starts below the tallest glyph so far. A one-texel border
/// separates glyphs. The atlas width grows to fit the widest glyph, and both
/// dimensions are checked against [`MAX_TEXTURE_EXTENT`], so an oversize request
/// fails closed. A blank glyph gets a zero-size rectangle and occupies no area.
fn pack_shelves(dimensions: &[(u32, u32)]) -> Result<(Extent2d, Vec<TexelRect>), GlyphAtlasError> {
    let border = PADDING
        .checked_mul(2)
        .ok_or(GlyphAtlasError::AtlasTooLarge)?;
    let widest = dimensions
        .iter()
        .map(|&(width, _)| width)
        .max()
        .unwrap_or(0);
    let minimum_width = widest
        .checked_add(border)
        .ok_or(GlyphAtlasError::AtlasTooLarge)?;
    let atlas_width = DEFAULT_ATLAS_WIDTH.max(minimum_width);
    if atlas_width > MAX_TEXTURE_EXTENT {
        return Err(GlyphAtlasError::AtlasTooLarge);
    }

    let mut rects = Vec::with_capacity(dimensions.len());
    let mut pen_x = PADDING;
    let mut pen_y = PADDING;
    let mut shelf_height = 0u32;

    for &(width, height) in dimensions {
        if width == 0 || height == 0 {
            rects.push(TexelRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
            continue;
        }

        let shelf_end = pen_x
            .checked_add(width)
            .and_then(|value| value.checked_add(PADDING))
            .ok_or(GlyphAtlasError::AtlasTooLarge)?;
        if shelf_end > atlas_width {
            pen_x = PADDING;
            pen_y = advance_shelf(pen_y, shelf_height)?;
            shelf_height = 0;
        }

        rects.push(TexelRect {
            x: pen_x,
            y: pen_y,
            width,
            height,
        });
        pen_x = pen_x
            .checked_add(width)
            .and_then(|value| value.checked_add(PADDING))
            .ok_or(GlyphAtlasError::AtlasTooLarge)?;
        shelf_height = shelf_height.max(height);
    }

    let atlas_height = advance_shelf(pen_y, shelf_height)?;
    if atlas_height > MAX_TEXTURE_EXTENT {
        return Err(GlyphAtlasError::AtlasTooLarge);
    }

    Ok((Extent2d::new(atlas_width, atlas_height), rects))
}

/// The next shelf origin below the current shelf, with a border, or a fail-closed
/// error at the `u32` boundary.
fn advance_shelf(pen_y: u32, shelf_height: u32) -> Result<u32, GlyphAtlasError> {
    pen_y
        .checked_add(shelf_height)
        .and_then(|value| value.checked_add(PADDING))
        .ok_or(GlyphAtlasError::AtlasTooLarge)
}

/// Allocates the atlas pixel buffer and blits every glyph mask into its rectangle.
///
/// The buffer length is the checked product of the extent and the texel size, so a
/// buffer that would overflow fails closed. Each mask's coverage is written as one
/// byte per texel; a paint stage samples the atlas as a coverage mask and applies
/// the run color itself.
fn paint_atlas(
    extent: Extent2d,
    rects: &[TexelRect],
    masks: &[GlyphMask],
) -> Result<Vec<u8>, GlyphAtlasError> {
    let length = (extent.width as usize)
        .checked_mul(extent.height as usize)
        .and_then(|texels| texels.checked_mul(BYTES_PER_TEXEL))
        .ok_or(GlyphAtlasError::AtlasTooLarge)?;
    let mut pixels = vec![0u8; length];

    for (rect, mask) in rects.iter().zip(masks.iter()) {
        blit_mask(&mut pixels, extent.width, rect, mask);
    }
    Ok(pixels)
}

/// Blits one glyph mask into its atlas rectangle as one coverage byte per texel.
fn blit_mask(pixels: &mut [u8], atlas_width: u32, rect: &TexelRect, mask: &GlyphMask) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    let coverage = mask.coverage();
    for row in 0..rect.height {
        let source_start = (row as usize) * (rect.width as usize);
        let source_row = &coverage[source_start..source_start + rect.width as usize];
        let destination_y = (rect.y + row) as usize;
        let destination_start =
            (destination_y * atlas_width as usize + rect.x as usize) * BYTES_PER_TEXEL;
        for (column, &value) in source_row.iter().enumerate() {
            pixels[destination_start + column * BYTES_PER_TEXEL] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purr_graphics::{
        FrameSubmission, FrameToken, PresentationTargetDescriptor, SceneGeneration, SceneId,
        SceneIdentity, SurfaceGeneration, SurfaceId,
    };

    fn test_namespace() -> ProducerNamespace {
        ProducerNamespace::new(7)
    }

    fn test_resource_id() -> ResourceId {
        ResourceId::new(42)
    }

    fn font() -> BundledFont {
        BundledFont::load().expect("the bundled font parses")
    }

    fn size() -> TextUnit {
        TextUnit::from_px(16).expect("in range")
    }

    fn key(font: &BundledFont, character: char) -> GlyphKey {
        GlyphKey::new(font.glyph_for(character), size())
    }

    fn build(font: &BundledFont, keys: &[GlyphKey]) -> GlyphAtlas {
        build_glyph_atlas(
            font,
            keys,
            test_namespace(),
            test_resource_id(),
            ResourceGeneration::new(1),
            DeviceGeneration::new(1),
        )
        .expect("the atlas builds")
    }

    #[test]
    fn the_atlas_identity_uses_exactly_the_caller_supplied_namespace_and_resource_id() {
        let font = font();
        let atlas = build(&font, &[key(&font, 'A'), key(&font, 'b')]);
        let resource = atlas.resource();

        assert_eq!(resource.resource_kind(), ResourceKind::GlyphAtlas);
        assert_eq!(resource.producer_namespace(), test_namespace());
        assert_eq!(resource.resource_id(), test_resource_id());
    }

    #[test]
    fn the_upload_pixel_length_passes_the_submission_check() {
        let font = font();
        let atlas = build(&font, &[key(&font, 'A'), key(&font, 'g'), key(&font, '0')]);

        let expected =
            (atlas.extent().width as usize) * (atlas.extent().height as usize) * BYTES_PER_TEXEL;
        assert_eq!(atlas.upload().pixels.len(), expected);

        let submission = FrameSubmission {
            frame_token: FrameToken::new(1),
            scene: SceneIdentity::new(
                SceneId::new(1),
                SceneGeneration::new(1),
                SurfaceId::new(1),
                SurfaceGeneration::new(1),
            ),
            target: PresentationTargetDescriptor {
                extent: Extent2d::new(64, 64),
                format: TextureFormatClass::Bgra8Unorm,
                alpha_mode: AlphaMode::Opaque,
            },
            uploads: vec![atlas.upload().clone()],
            commands: Vec::new(),
        };
        assert_eq!(submission.validate(), Ok(()));
    }

    #[test]
    fn packed_glyphs_do_not_overlap_and_stay_within_the_extent() {
        let font = font();
        let atlas = build(
            &font,
            &[
                key(&font, 'A'),
                key(&font, 'b'),
                key(&font, 'c'),
                key(&font, 'd'),
                key(&font, 'e'),
            ],
        );
        let extent = atlas.extent();

        let rects: Vec<TexelRect> = atlas
            .placements()
            .iter()
            .map(|placement| placement.source())
            .filter(|rect| rect.width > 0 && rect.height > 0)
            .collect();

        for rect in &rects {
            assert!(rect.x + rect.width <= extent.width);
            assert!(rect.y + rect.height <= extent.height);
        }
        for first in 0..rects.len() {
            for second in first + 1..rects.len() {
                assert!(!overlaps(&rects[first], &rects[second]));
            }
        }
    }

    fn overlaps(first: &TexelRect, second: &TexelRect) -> bool {
        let separate = first.x + first.width <= second.x
            || second.x + second.width <= first.x
            || first.y + first.height <= second.y
            || second.y + second.height <= first.y;
        !separate
    }

    #[test]
    fn duplicate_keys_are_packed_once() {
        let font = font();
        let atlas = build(&font, &[key(&font, 'A'), key(&font, 'A'), key(&font, 'A')]);

        assert_eq!(atlas.placements().len(), 1);
    }

    #[test]
    fn a_size_that_exceeds_the_max_extent_fails_closed() {
        let result = pack_shelves(&[(MAX_TEXTURE_EXTENT + 10, 10)]);

        assert_eq!(result, Err(GlyphAtlasError::AtlasTooLarge));
    }

    #[test]
    fn many_shelves_that_exceed_the_max_extent_fail_closed() {
        // Each glyph is wide enough to take a shelf of its own, so a modest count
        // of tall glyphs pushes the atlas height past the bound.
        let dimensions = vec![(200u32, 100u32); 100];

        let result = pack_shelves(&dimensions);

        assert_eq!(result, Err(GlyphAtlasError::AtlasTooLarge));
    }

    #[test]
    fn a_glyph_source_rect_is_physical_and_absent_from_the_logical_run() {
        use crate::text_shaping::{
            CmapOneToOneAdapter, GlyphRunGeneration, GlyphRunId, ShapingRequest, TextShapingAdapter,
        };

        let font = font();
        let run = CmapOneToOneAdapter
            .shape(ShapingRequest {
                font: &font,
                text: "A",
                size: size(),
                run_id: GlyphRunId::new(1),
                generation: GlyphRunGeneration::new(1),
            })
            .expect("the run shapes");
        let positioned = run.glyph(0).expect("one glyph");

        let atlas = build(&font, &[GlyphKey::new(positioned.glyph(), size())]);
        let placement = atlas
            .placement(GlyphKey::new(positioned.glyph(), size()))
            .expect("the glyph is packed");

        // The atlas owns the physical source rectangle. The run exposes the glyph
        // index and advance only; there is no accessor on the run for the source
        // rectangle, so the two representations stay separate by construction.
        assert!(placement.source().width > 0);
        assert_eq!(placement.key().glyph(), positioned.glyph());
    }
}
