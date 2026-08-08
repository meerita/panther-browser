// @file engines/purr/graphics/src/submission.rs
// @description Defines the data-only logical submission protocol seam for one frame.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Logical submission protocol seam.
//!
//! One `FrameSubmission` describes a complete, self-contained frame of logical
//! rendering work: the resource uploads it needs and the fixed command set that
//! paints it. Both backends consume these types. A future GPU-process split
//! turns this seam into a transport message without an interface redesign.
//!
//! These are plain data types. They carry identities and descriptors from the
//! interface, never a backend type. No `serde` derive exists yet; the types are
//! serialization-ready only.
//!
//! Every externally sized field is bounded. A submission that exceeds the count
//! bounds, or an upload whose pixel buffer does not match its descriptor, is
//! rejected by `FrameSubmission::validate` before any backend consumes it.

use crate::descriptor::{PresentationTargetDescriptor, TextureDescriptor};
use crate::graphics_error::GraphicsError;
use crate::identity::{FrameToken, GpuResourceIdentity, SceneIdentity};

/// M0 upper bound for the number of draw commands in one submission.
///
/// A submission above this bound is rejected before any backend consumes it.
/// The bound caps the work one frame can request.
pub const MAX_DRAW_COMMANDS: usize = 65_536;

/// M0 upper bound for the number of resource uploads in one submission.
///
/// A submission above this bound is rejected before any backend allocates from
/// it. The bound caps the upload work one frame can request.
pub const MAX_RESOURCE_UPLOADS: usize = 4_096;

/// Axis-aligned rectangle in logical pixels.
///
/// A plain geometry pair with no invariant of its own. The command that carries
/// it defines how the coordinates are interpreted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Fixed M0 draw command set.
///
/// The set is closed and small. `Clear` fills the target with one color.
/// `FillRect` paints a solid-color rectangle. `TexturedQuad` samples a coverage
/// mask over a destination rectangle and colorizes it with `color`: the source
/// texture supplies per-texel coverage and the color supplies the visible color,
/// so one atlas serves every text color. A new command requires an interface
/// change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrawCommand {
    Clear {
        color: crate::descriptor::Color,
    },
    FillRect {
        rect: Rect,
        color: crate::descriptor::Color,
    },
    TexturedQuad {
        rect: Rect,
        texture: GpuResourceIdentity,
        source: Rect,
        color: crate::descriptor::Color,
    },
}

/// One texture upload carried by a submission.
///
/// The `pixels` buffer holds tightly packed rows in top-to-bottom order. Each
/// row is `width` texels wide with no padding, and each texel occupies the byte
/// count of the descriptor format. The buffer length must equal width times
/// height times bytes per texel; `FrameSubmission::validate` enforces the match
/// before any backend reads the buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceUpload {
    pub resource: GpuResourceIdentity,
    pub descriptor: TextureDescriptor,
    pub pixels: Vec<u8>,
}

/// One self-contained frame of logical rendering work.
///
/// The submission carries the target it presents to, the uploads it needs, and
/// the ordered command list that paints it. Each submission is complete; the
/// seam carries no incremental or delta state at M0.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSubmission {
    pub frame_token: FrameToken,
    pub scene: SceneIdentity,
    pub target: PresentationTargetDescriptor,
    pub uploads: Vec<ResourceUpload>,
    pub commands: Vec<DrawCommand>,
}

impl FrameSubmission {
    /// Rejects an over-bound or malformed submission.
    ///
    /// A command or upload count above the M0 bound is a `SubmissionRejected`.
    /// An upload with an invalid descriptor, or a pixel buffer whose length does
    /// not match the descriptor, is an `InvalidDescriptor`. The pixel length is
    /// computed with checked multiplication, so an overflowing descriptor is
    /// rejected instead of wrapping. The check allocates nothing.
    pub fn validate(&self) -> Result<(), GraphicsError> {
        if self.commands.len() > MAX_DRAW_COMMANDS {
            return Err(GraphicsError::SubmissionRejected);
        }

        if self.uploads.len() > MAX_RESOURCE_UPLOADS {
            return Err(GraphicsError::SubmissionRejected);
        }

        for upload in &self.uploads {
            upload.descriptor.validate()?;

            let expected = expected_pixel_length(&upload.descriptor)
                .ok_or(GraphicsError::InvalidDescriptor)?;

            if upload.pixels.len() as u64 != expected {
                return Err(GraphicsError::InvalidDescriptor);
            }
        }

        Ok(())
    }
}

/// Expected tightly packed pixel buffer length for a descriptor.
///
/// Returns `None` when the width times height times bytes-per-texel product
/// overflows, so the caller rejects an overflowing descriptor. The descriptor
/// bound keeps this product small in practice; the checked multiplication is a
/// defensive guard.
fn expected_pixel_length(descriptor: &TextureDescriptor) -> Option<u64> {
    let width = u64::from(descriptor.extent.width);
    let height = u64::from(descriptor.extent.height);
    let bytes = u64::from(descriptor.format.bytes_per_texel());

    width.checked_mul(height)?.checked_mul(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{
        AlphaMode, Color, ColorSpace, Extent2d, PresentationTargetDescriptor, TextureDescriptor,
        TextureFormatClass,
    };
    use crate::identity::{
        DeviceGeneration, ProducerNamespace, ResourceGeneration, ResourceId, ResourceKind,
        SceneGeneration, SceneId, SurfaceGeneration, SurfaceId,
    };

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

    fn texture_identity() -> GpuResourceIdentity {
        GpuResourceIdentity::new(
            ProducerNamespace::new(1),
            ResourceId::new(1),
            ResourceGeneration::new(1),
            ResourceKind::Texture,
            DeviceGeneration::new(1),
        )
    }

    fn texture_descriptor(width: u32, height: u32) -> TextureDescriptor {
        TextureDescriptor {
            extent: Extent2d::new(width, height),
            format: TextureFormatClass::Rgba8Unorm,
            color_space: ColorSpace::Srgb,
            alpha_mode: AlphaMode::Premultiplied,
            label: None,
        }
    }

    fn upload(width: u32, height: u32, pixel_bytes: usize) -> ResourceUpload {
        ResourceUpload {
            resource: texture_identity(),
            descriptor: texture_descriptor(width, height),
            pixels: vec![0u8; pixel_bytes],
        }
    }

    fn submission(uploads: Vec<ResourceUpload>, commands: Vec<DrawCommand>) -> FrameSubmission {
        FrameSubmission {
            frame_token: FrameToken::new(1),
            scene: scene(),
            target: target(),
            uploads,
            commands,
        }
    }

    #[test]
    fn valid_submission_passes() {
        let pixels = (2 * 2 * 4) as usize;
        let commands = vec![
            DrawCommand::Clear {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            },
            DrawCommand::FillRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            },
            DrawCommand::TexturedQuad {
                rect: Rect::new(0.0, 0.0, 2.0, 2.0),
                texture: texture_identity(),
                source: Rect::new(0.0, 0.0, 2.0, 2.0),
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            },
        ];

        let frame = submission(vec![upload(2, 2, pixels)], commands);

        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn over_bound_command_list_is_rejected() {
        let commands = vec![
            DrawCommand::Clear {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            };
            MAX_DRAW_COMMANDS + 1
        ];

        let frame = submission(Vec::new(), commands);

        assert_eq!(frame.validate(), Err(GraphicsError::SubmissionRejected));
    }

    #[test]
    fn over_bound_upload_list_is_rejected() {
        let uploads = vec![upload(1, 1, 4); MAX_RESOURCE_UPLOADS + 1];

        let frame = submission(uploads, Vec::new());

        assert_eq!(frame.validate(), Err(GraphicsError::SubmissionRejected));
    }

    #[test]
    fn upload_with_wrong_pixel_length_is_rejected() {
        let frame = submission(vec![upload(2, 2, 8)], Vec::new());

        assert_eq!(frame.validate(), Err(GraphicsError::InvalidDescriptor));
    }

    #[test]
    fn upload_with_overflowing_descriptor_is_rejected() {
        let frame = submission(vec![upload(u32::MAX, u32::MAX, 0)], Vec::new());

        assert_eq!(frame.validate(), Err(GraphicsError::InvalidDescriptor));
    }
}
