// @file products/panther/window/src/compositor.rs
// @description Offsets and clips the document frame into the viewport and merges it with chrome.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Document-into-viewport compositor.
//!
//! The engine lays out a document in document-local coordinates at origin (0,0)
//! and never learns where the viewport sits in the window. This module owns the
//! document-local to surface transform: it offsets each document draw command by
//! the viewport origin, clips it to the viewport rectangle, and merges the result
//! with the shell chrome into one submission. The engine uploads are carried
//! through unchanged.
//!
//! A frame from a superseded generation is rejected before it reaches the surface,
//! so a stale render never paints over the live chrome.

use purr_embedding::{DocumentFrame, DocumentGeneration};
use purr_graphics::{
    DrawCommand, FrameSubmission, FrameToken, PresentationTargetDescriptor, Rect, ResourceUpload,
    SceneIdentity,
};

/// Document content placed into the viewport, ready to merge with chrome.
///
/// The commands are in surface pixel space (already offset and clipped). The
/// uploads are the engine uploads carried through unchanged.
pub struct CompositedDocument {
    pub commands: Vec<DrawCommand>,
    pub uploads: Vec<ResourceUpload>,
}

/// Offsets and clips a document frame into the viewport rectangle.
///
/// Returns `None` when the frame names a generation other than the live one, so a
/// superseded frame never reaches the surface. Each command translates by the
/// viewport origin and clips to the viewport; a command clipped to nothing is
/// dropped. The uploads carry through unchanged.
pub fn composite_document(
    frame: &DocumentFrame,
    viewport: Rect,
    live_generation: DocumentGeneration,
) -> Option<CompositedDocument> {
    if frame.generation != live_generation {
        return None;
    }

    let mut commands = Vec::with_capacity(frame.commands.len());
    for command in &frame.commands {
        if let Some(placed) = offset_and_clip(*command, viewport) {
            commands.push(placed);
        }
    }

    Some(CompositedDocument {
        commands,
        uploads: frame.uploads.clone(),
    })
}

/// Merges the shell chrome and the composited document into one submission.
///
/// The chrome paints first and the document paints over the viewport region, so
/// the document is visible without removing the chrome viewport fill. The chrome
/// atlas upload leads the submission uploads and the document uploads follow, so
/// both the chrome text quads and the document glyph quads name a live backend
/// resource. The result is validation ready; the backend validates it before it
/// presents.
pub fn merge_submission(
    chrome_commands: Vec<DrawCommand>,
    chrome_uploads: Vec<ResourceUpload>,
    content: Option<CompositedDocument>,
    frame_token: FrameToken,
    scene: SceneIdentity,
    target: PresentationTargetDescriptor,
) -> FrameSubmission {
    let mut commands = chrome_commands;
    let mut uploads = chrome_uploads;

    if let Some(content) = content {
        commands.extend(content.commands);
        uploads.extend(content.uploads);
    }

    FrameSubmission {
        frame_token,
        scene,
        target,
        uploads,
        commands,
    }
}

/// Offsets one document-local command by the viewport origin and clips it.
///
/// A `FillRect` and a `TexturedQuad` translate into the viewport and clip to it.
/// A `TexturedQuad` clips its source region by the same fractions so the texture
/// keeps sampling the correct texels. A `Clear` fills the viewport, since a
/// whole-surface clear has no meaning inside a composited viewport.
fn offset_and_clip(command: DrawCommand, viewport: Rect) -> Option<DrawCommand> {
    match command {
        DrawCommand::Clear { color } => Some(DrawCommand::FillRect {
            rect: viewport,
            color,
        }),
        DrawCommand::FillRect { rect, color } => {
            let placed = intersect(offset(rect, viewport), viewport)?;
            Some(DrawCommand::FillRect {
                rect: placed,
                color,
            })
        }
        DrawCommand::TexturedQuad {
            rect,
            texture,
            source,
        } => {
            let placed = offset(rect, viewport);
            let clipped = intersect(placed, viewport)?;
            let source = clip_source(placed, clipped, source)?;
            Some(DrawCommand::TexturedQuad {
                rect: clipped,
                texture,
                source,
            })
        }
    }
}

/// Translates a document-local rectangle by the viewport origin.
fn offset(rect: Rect, viewport: Rect) -> Rect {
    Rect::new(
        rect.x + viewport.x,
        rect.y + viewport.y,
        rect.width,
        rect.height,
    )
}

/// Intersects two rectangles, returning `None` when they do not overlap.
///
/// A zero-area intersection is treated as no overlap, so a command that touches
/// only the viewport edge is dropped instead of producing a degenerate rectangle.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);

    if right <= left || bottom <= top {
        return None;
    }

    Some(Rect::new(left, top, right - left, bottom - top))
}

/// Clips a texture source region to match a clipped destination rectangle.
///
/// The destination was clipped by some fraction on each side; the source clips by
/// the same fractions so the visible texels stay aligned with the visible
/// destination. A degenerate destination (zero width or height) samples nothing
/// and is dropped.
fn clip_source(dest: Rect, clipped: Rect, source: Rect) -> Option<Rect> {
    if dest.width <= 0.0 || dest.height <= 0.0 {
        return None;
    }

    let left_fraction = (clipped.x - dest.x) / dest.width;
    let top_fraction = (clipped.y - dest.y) / dest.height;
    let width_fraction = clipped.width / dest.width;
    let height_fraction = clipped.height / dest.height;

    Some(Rect::new(
        source.x + left_fraction * source.width,
        source.y + top_fraction * source.height,
        width_fraction * source.width,
        height_fraction * source.height,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use purr_graphics::{
        AlphaMode, Color, ColorSpace, DeviceGeneration, Extent2d, GpuResourceIdentity,
        ProducerNamespace, ResourceGeneration, ResourceId, ResourceKind, SceneGeneration, SceneId,
        SurfaceGeneration, SurfaceId, TextureDescriptor, TextureFormatClass,
    };

    const CONTENT_COLOR: Color = Color::new(0.2, 0.4, 0.8, 1.0);
    const CHROME_COLOR: Color = Color::new(0.1, 0.1, 0.1, 1.0);

    fn viewport() -> Rect {
        Rect::new(100.0, 50.0, 400.0, 300.0)
    }

    fn texture() -> GpuResourceIdentity {
        GpuResourceIdentity::new(
            ProducerNamespace::new(7),
            ResourceId::new(1),
            ResourceGeneration::new(1),
            ResourceKind::GlyphAtlas,
            DeviceGeneration::new(1),
        )
    }

    fn atlas_upload() -> ResourceUpload {
        ResourceUpload {
            resource: texture(),
            descriptor: TextureDescriptor {
                extent: Extent2d::new(2, 2),
                format: TextureFormatClass::Rgba8Unorm,
                color_space: ColorSpace::Srgb,
                alpha_mode: AlphaMode::Opaque,
                label: None,
            },
            pixels: vec![0u8; 2 * 2 * 4],
        }
    }

    fn chrome_atlas() -> GpuResourceIdentity {
        GpuResourceIdentity::new(
            ProducerNamespace::new(3),
            ResourceId::new(1),
            ResourceGeneration::new(1),
            ResourceKind::GlyphAtlas,
            DeviceGeneration::new(1),
        )
    }

    fn chrome_upload() -> ResourceUpload {
        ResourceUpload {
            resource: chrome_atlas(),
            descriptor: TextureDescriptor {
                extent: Extent2d::new(2, 2),
                format: TextureFormatClass::Rgba8Unorm,
                color_space: ColorSpace::Srgb,
                alpha_mode: AlphaMode::Opaque,
                label: None,
            },
            pixels: vec![0u8; 2 * 2 * 4],
        }
    }

    fn frame(generation: DocumentGeneration, commands: Vec<DrawCommand>) -> DocumentFrame {
        DocumentFrame {
            generation,
            producer: ProducerNamespace::new(7),
            uploads: vec![atlas_upload()],
            commands,
        }
    }

    fn scene() -> SceneIdentity {
        SceneIdentity::new(
            SceneId::new(1),
            SceneGeneration::new(1),
            SurfaceId::new(1),
            SurfaceGeneration::new(1),
        )
    }

    fn target() -> PresentationTargetDescriptor {
        PresentationTargetDescriptor {
            extent: Extent2d::new(640, 480),
            format: TextureFormatClass::Bgra8Unorm,
            alpha_mode: AlphaMode::Opaque,
        }
    }

    #[test]
    fn a_document_rectangle_offsets_by_the_viewport_origin() {
        let document_rect = Rect::new(10.0, 20.0, 50.0, 60.0);
        let composited = composite_document(
            &frame(
                DocumentGeneration::FIRST,
                vec![DrawCommand::FillRect {
                    rect: document_rect,
                    color: CONTENT_COLOR,
                }],
            ),
            viewport(),
            DocumentGeneration::FIRST,
        )
        .expect("frame composites");

        assert_eq!(
            composited.commands,
            vec![DrawCommand::FillRect {
                rect: Rect::new(110.0, 70.0, 50.0, 60.0),
                color: CONTENT_COLOR,
            }]
        );
    }

    #[test]
    fn a_document_rectangle_clips_at_the_viewport_bounds() {
        // Offsets to x = 480, extends to x = 580, past the viewport right at 500.
        let document_rect = Rect::new(380.0, 0.0, 100.0, 100.0);
        let composited = composite_document(
            &frame(
                DocumentGeneration::FIRST,
                vec![DrawCommand::FillRect {
                    rect: document_rect,
                    color: CONTENT_COLOR,
                }],
            ),
            viewport(),
            DocumentGeneration::FIRST,
        )
        .expect("frame composites");

        assert_eq!(
            composited.commands,
            vec![DrawCommand::FillRect {
                rect: Rect::new(480.0, 50.0, 20.0, 100.0),
                color: CONTENT_COLOR,
            }]
        );
    }

    #[test]
    fn a_rectangle_outside_the_viewport_is_dropped() {
        let document_rect = Rect::new(500.0, 500.0, 40.0, 40.0);
        let composited = composite_document(
            &frame(
                DocumentGeneration::FIRST,
                vec![DrawCommand::FillRect {
                    rect: document_rect,
                    color: CONTENT_COLOR,
                }],
            ),
            viewport(),
            DocumentGeneration::FIRST,
        )
        .expect("frame composites");

        assert!(composited.commands.is_empty());
    }

    #[test]
    fn a_superseded_generation_is_rejected() {
        let stale = frame(
            DocumentGeneration::FIRST,
            vec![DrawCommand::FillRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: CONTENT_COLOR,
            }],
        );

        let live = DocumentGeneration::new(2);

        assert!(composite_document(&stale, viewport(), live).is_none());
    }

    #[test]
    fn the_merged_submission_carries_chrome_content_and_uploads() {
        let content_rect = Rect::new(10.0, 10.0, 20.0, 20.0);
        let quad_rect = Rect::new(40.0, 40.0, 8.0, 8.0);
        let composited = composite_document(
            &frame(
                DocumentGeneration::FIRST,
                vec![
                    DrawCommand::FillRect {
                        rect: content_rect,
                        color: CONTENT_COLOR,
                    },
                    DrawCommand::TexturedQuad {
                        rect: quad_rect,
                        texture: texture(),
                        source: Rect::new(0.0, 0.0, 2.0, 2.0),
                    },
                ],
            ),
            viewport(),
            DocumentGeneration::FIRST,
        )
        .expect("frame composites");

        let chrome = vec![
            DrawCommand::Clear {
                color: CHROME_COLOR,
            },
            DrawCommand::FillRect {
                rect: Rect::new(0.0, 0.0, 640.0, 40.0),
                color: CHROME_COLOR,
            },
        ];

        let submission = merge_submission(
            chrome,
            vec![chrome_upload()],
            Some(composited),
            FrameToken::new(1),
            scene(),
            target(),
        );

        assert_eq!(submission.validate(), Ok(()));

        // Chrome paints first, then the two offset content commands over it.
        assert_eq!(submission.commands.len(), 4);
        assert!(matches!(submission.commands[0], DrawCommand::Clear { .. }));
        assert!(
            submission
                .commands
                .iter()
                .any(|command| matches!(command, DrawCommand::TexturedQuad { .. }))
        );

        // The chrome atlas upload leads and the document upload follows.
        assert_eq!(submission.uploads.len(), 2);
        assert_eq!(submission.uploads[0].resource, chrome_atlas());
        assert_eq!(submission.uploads[1].resource, texture());
    }

    #[test]
    fn a_quad_clips_its_source_region_with_the_destination() {
        // Offsets to x = 490, extends to x = 510, past the viewport right at 500,
        // so half the quad and half the source width survive.
        let quad_rect = Rect::new(390.0, 0.0, 20.0, 10.0);
        let composited = composite_document(
            &frame(
                DocumentGeneration::FIRST,
                vec![DrawCommand::TexturedQuad {
                    rect: quad_rect,
                    texture: texture(),
                    source: Rect::new(0.0, 0.0, 8.0, 4.0),
                }],
            ),
            viewport(),
            DocumentGeneration::FIRST,
        )
        .expect("frame composites");

        let DrawCommand::TexturedQuad { rect, source, .. } = composited.commands[0] else {
            panic!("expected a textured quad");
        };
        assert_eq!(rect, Rect::new(490.0, 50.0, 10.0, 10.0));
        assert_eq!(source, Rect::new(0.0, 0.0, 4.0, 4.0));
    }

    #[test]
    fn merge_without_content_returns_only_chrome() {
        let chrome = vec![DrawCommand::Clear {
            color: CHROME_COLOR,
        }];

        let submission = merge_submission(
            chrome,
            Vec::new(),
            None,
            FrameToken::new(1),
            scene(),
            target(),
        );

        assert_eq!(submission.commands.len(), 1);
        assert!(submission.uploads.is_empty());
        assert_eq!(submission.validate(), Ok(()));
    }
}
