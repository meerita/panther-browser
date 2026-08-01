// @file engines/purr/graphics/src/lib.rs
// @description Library root for the Panther-owned engine graphics interface.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Panther-owned engine graphics interface.
//!
//! This crate defines the narrow graphics interface that the engine and the
//! product use to render. It exposes only Panther-owned types.
//!
//! Isolation rule: this crate must never depend on a graphics backend library
//! or a native graphics API. No `wgpu`, `winit`, `raw-window-handle`, or
//! platform GPU type appears here. Backend and native types live only inside
//! the adapter and windowing crates, behind this interface.

#[path = "descriptor.rs"]
mod descriptor;
#[path = "graphics-error.rs"]
mod graphics_error;
#[path = "identity.rs"]
mod identity;
#[path = "submission.rs"]
mod submission;

pub use descriptor::{
    AlphaMode, BufferDescriptor, BufferUsage, Color, ColorSpace, Extent2d, MAX_TEXTURE_EXTENT,
    PipelineKind, PresentationTargetDescriptor, RenderTargetDescriptor, TextureDescriptor,
    TextureFormatClass,
};
pub use graphics_error::GraphicsError;
pub use identity::{
    DeviceGeneration, FrameToken, GpuResourceIdentity, ProducerNamespace, ResourceGeneration,
    ResourceId, ResourceKind, SceneGeneration, SceneId, SceneIdentity, SurfaceGeneration,
    SurfaceId, SurfaceIdentity,
};
pub use submission::{
    DrawCommand, FrameSubmission, MAX_DRAW_COMMANDS, MAX_RESOURCE_UPLOADS, Rect, ResourceUpload,
};
