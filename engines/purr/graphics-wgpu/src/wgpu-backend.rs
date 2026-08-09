// @file engines/purr/graphics-wgpu/src/wgpu-backend.rs
// @description Implements the hardware graphics backend over wgpu.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Hardware graphics backend over `wgpu`.
//!
//! `WgpuBackend` implements the Panther `GraphicsBackend` contract with `wgpu`.
//! Every `wgpu` type stays inside this crate. The public surface speaks only in
//! Panther identities, descriptors, and `GraphicsError`, so no backend or
//! native-API type escapes the interface. Each `wgpu` failure is translated to a
//! `GraphicsError` at the method boundary.
//!
//! The backend allocates textures, builds the two WGSL pipelines through `naga`,
//! and renders the fixed M0 command set (`Clear`, `FillRect`, `TexturedQuad`)
//! into a surface target. `submit` validates a frame, applies resource uploads
//! with checked layout math, and stores the command list; `present` draws the
//! stored frame. A submission that references a resource from a different device
//! generation is rejected before any work is applied.

use std::collections::HashMap;

use purr_graphics::{
    AlphaMode, BackendKind, Color, DeviceGeneration, DrawCommand, Extent2d, FrameSubmission,
    GpuResourceIdentity, GraphicsBackend, GraphicsError, PresentationTargetDescriptor,
    ProducerNamespace, Rect, ResourceGeneration, ResourceId, ResourceKind, ResourceUpload,
    SurfaceGeneration, SurfaceId, SurfaceIdentity, TextureDescriptor, TextureFormatClass,
    WindowSurface,
};

use crate::render_pipelines::{FormatPipelines, ShaderResources, UNIFORM_BYTES};

/// Producer namespace this backend stamps on the identities it creates.
///
/// A single-process backend has one producer, so the namespace is fixed. A
/// multi-process split assigns namespaces per producer later.
const BACKEND_NAMESPACE: u32 = 1;

/// Number of monitor refreshes the presentation engine may buffer.
///
/// Two frames balance latency and throughput and match the `wgpu` default.
const FRAME_LATENCY: u32 = 2;

/// Vertices in one quad drawn as two triangles.
const QUAD_VERTICES: u32 = 6;

/// One live presentation target owned by the backend.
///
/// The backend keys these by `SurfaceIdentity`. The caller holds the identity
/// only and never a `wgpu` handle. The pending command list is the last valid
/// submission; `present` draws it.
#[derive(Debug)]
struct PresentationTarget {
    surface: wgpu::Surface<'static>,
    configuration: wgpu::SurfaceConfiguration,
    pending_commands: Vec<DrawCommand>,
}

/// One texture resource owned by the backend.
///
/// The extent is kept so a `TexturedQuad` can convert a source rectangle in
/// texels into normalized texture coordinates.
#[derive(Debug)]
struct TextureResource {
    texture: wgpu::Texture,
    extent: Extent2d,
}

/// One planned draw ready to record into a render pass.
enum PlannedDraw {
    Solid {
        offset: u32,
    },
    Textured {
        offset: u32,
        bind_group: wgpu::BindGroup,
    },
}

/// Hardware backend that renders and presents through `wgpu`.
#[derive(Debug)]
pub struct WgpuBackend {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shaders: ShaderResources,
    pipelines: HashMap<wgpu::TextureFormat, FormatPipelines>,
    uniform_slot: u64,
    device_generation: DeviceGeneration,
    targets: HashMap<SurfaceIdentity, PresentationTarget>,
    resources: HashMap<GpuResourceIdentity, TextureResource>,
    next_surface_id: u64,
    next_resource_id: u64,
}

impl WgpuBackend {
    /// Creates a `wgpu` surface from the neutral window seam.
    ///
    /// The safe `create_surface` path returns a surface bound to the borrowed
    /// window handle, so it cannot be stored past the call. Storing a surface
    /// keyed by identity needs a `'static` surface, which `wgpu` only provides
    /// through the unsafe raw-handle path. The unsafe use is confined to this one
    /// function.
    #[allow(unsafe_code)]
    fn create_surface(
        &self,
        window: &WindowSurface<'_>,
    ) -> Result<wgpu::Surface<'static>, GraphicsError> {
        let display_handle = window
            .display_handle()
            .display_handle()
            .map_err(|_| GraphicsError::Unsupported)?
            .as_raw();
        let window_handle = window
            .window_handle()
            .window_handle()
            .map_err(|_| GraphicsError::Unsupported)?
            .as_raw();

        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display_handle),
            raw_window_handle: window_handle,
        };

        // SAFETY: The raw display and window handles come from the caller's
        // `WindowSurface`, which borrows a live window through the neutral
        // `raw-window-handle` traits for this call. The caller keeps the window
        // valid while it uses the returned target. The window-lifetime contract
        // that keeps the handle valid across later presents is owned by the
        // windowing integration.
        let surface = unsafe { self.instance.create_surface_unsafe(target) };
        surface.map_err(|_| GraphicsError::Unsupported)
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

    /// Returns the render pipelines for a format, building and caching them on
    /// first use.
    fn ensure_pipelines(
        &mut self,
        format: wgpu::TextureFormat,
    ) -> Result<FormatPipelines, GraphicsError> {
        if let Some(pipelines) = self.pipelines.get(&format) {
            return Ok(pipelines.clone());
        }

        let pipelines = self.shaders.pipelines_for(&self.device, format)?;
        self.pipelines.insert(format, pipelines.clone());
        Ok(pipelines)
    }

    /// Rejects a submission that references a resource from a different device
    /// generation, or a resource that does not exist.
    ///
    /// A generation mismatch means the resource belongs to a device the backend
    /// no longer owns, so it is reported as a device loss. Both checks run before
    /// any upload is applied.
    fn validate_resource_identities(
        &self,
        submission: &FrameSubmission,
    ) -> Result<(), GraphicsError> {
        for upload in &submission.uploads {
            self.check_generation(upload.resource)?;
            if !self.resources.contains_key(&upload.resource) {
                return Err(GraphicsError::ResourceNotFound);
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

    /// Writes each upload to its target texture with checked layout math.
    ///
    /// The upload extent must match the target texture extent, so the write
    /// covers the whole texture and never addresses outside it. The row and
    /// buffer lengths are recomputed with checked arithmetic before the write.
    fn apply_uploads(&self, uploads: &[ResourceUpload]) -> Result<(), GraphicsError> {
        for upload in uploads {
            let resource = self
                .resources
                .get(&upload.resource)
                .ok_or(GraphicsError::ResourceNotFound)?;

            if upload.descriptor.extent != resource.extent {
                return Err(GraphicsError::InvalidDescriptor);
            }

            let bytes_per_row = upload
                .descriptor
                .extent
                .width
                .checked_mul(upload.descriptor.format.bytes_per_texel())
                .ok_or(GraphicsError::InvalidDescriptor)?;
            let expected = u64::from(bytes_per_row)
                .checked_mul(u64::from(upload.descriptor.extent.height))
                .ok_or(GraphicsError::InvalidDescriptor)?;
            if upload.pixels.len() as u64 != expected {
                return Err(GraphicsError::InvalidDescriptor);
            }

            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &resource.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(upload.descriptor.extent.height),
                },
                wgpu::Extent3d {
                    width: upload.descriptor.extent.width,
                    height: upload.descriptor.extent.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        Ok(())
    }

    /// Records the command list into a render pass targeting `view` and submits
    /// it to the queue.
    ///
    /// The uniform data for every draw is written into one dynamic uniform buffer
    /// per pipeline before the pass, so the pass only sets offsets and draws. The
    /// pass starts from a defined cleared target, then draws each command in
    /// order.
    fn encode_frame(
        &mut self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        extent: Extent2d,
        commands: &[DrawCommand],
    ) -> Result<(), GraphicsError> {
        let pipelines = self.ensure_pipelines(format)?;
        let slot = usize::try_from(self.uniform_slot).map_err(|_| GraphicsError::Unsupported)?;

        let mut solid_bytes: Vec<u8> = Vec::new();
        let mut textured_bytes: Vec<u8> = Vec::new();
        let mut plan: Vec<PlannedDraw> = Vec::with_capacity(commands.len());

        for command in commands {
            match command {
                DrawCommand::Clear { color } => {
                    let index = push_uniform(
                        &mut solid_bytes,
                        [-1.0, -1.0, 1.0, 1.0],
                        [0.0; 4],
                        color_components(*color),
                        slot,
                    );
                    plan.push(PlannedDraw::Solid {
                        offset: slot_offset(index, slot)?,
                    });
                }
                DrawCommand::FillRect { rect, color } => {
                    let index = push_uniform(
                        &mut solid_bytes,
                        rect_to_ndc(*rect, extent),
                        [0.0; 4],
                        color_components(*color),
                        slot,
                    );
                    plan.push(PlannedDraw::Solid {
                        offset: slot_offset(index, slot)?,
                    });
                }
                DrawCommand::TexturedQuad {
                    rect,
                    texture,
                    source,
                    color,
                } => {
                    let resource = self
                        .resources
                        .get(texture)
                        .ok_or(GraphicsError::ResourceNotFound)?;
                    let index = push_uniform(
                        &mut textured_bytes,
                        rect_to_ndc(*rect, extent),
                        source_to_uv(*source, resource.extent),
                        color_components(*color),
                        slot,
                    );
                    let texture_view = resource
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: self.shaders.texture_layout(),
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(self.shaders.sampler()),
                            },
                        ],
                    });
                    plan.push(PlannedDraw::Textured {
                        offset: slot_offset(index, slot)?,
                        bind_group,
                    });
                }
            }
        }

        let solid_buffer = self.make_uniform_buffer(&solid_bytes);
        let textured_buffer = self.make_uniform_buffer(&textured_bytes);
        let solid_bind_group = solid_buffer
            .as_ref()
            .map(|buffer| self.uniform_bind_group(buffer));
        let textured_bind_group = textured_buffer
            .as_ref()
            .map(|buffer| self.uniform_bind_group(buffer));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            for draw in &plan {
                match draw {
                    PlannedDraw::Solid { offset } => {
                        let bind_group = solid_bind_group
                            .as_ref()
                            .ok_or(GraphicsError::SubmissionRejected)?;
                        pass.set_pipeline(&pipelines.solid);
                        pass.set_bind_group(0, bind_group, &[*offset]);
                        pass.draw(0..QUAD_VERTICES, 0..1);
                    }
                    PlannedDraw::Textured { offset, bind_group } => {
                        let uniform_bind_group = textured_bind_group
                            .as_ref()
                            .ok_or(GraphicsError::SubmissionRejected)?;
                        pass.set_pipeline(&pipelines.textured);
                        pass.set_bind_group(0, uniform_bind_group, &[*offset]);
                        pass.set_bind_group(1, bind_group, &[]);
                        pass.draw(0..QUAD_VERTICES, 0..1);
                    }
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// Creates a uniform buffer holding the packed draw data, or `None` when no
    /// draw of that kind exists.
    fn make_uniform_buffer(&self, bytes: &[u8]) -> Option<wgpu::Buffer> {
        if bytes.is_empty() {
            return None;
        }

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("purr-graphics-wgpu-draw-uniform"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, bytes);
        Some(buffer)
    }

    /// Creates a bind group over a dynamic uniform buffer for one draw slot.
    fn uniform_bind_group(&self, buffer: &wgpu::Buffer) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: self.shaders.uniform_layout(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(UNIFORM_BYTES),
                }),
            }],
        })
    }
}

impl GraphicsBackend for WgpuBackend {
    fn create(selection: BackendKind) -> Result<Self, GraphicsError> {
        if selection != BackendKind::Hardware {
            return Err(GraphicsError::Unsupported);
        }

        let instance = wgpu::Instance::default();

        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        };
        let adapter = pollster::block_on(instance.request_adapter(&options))
            .map_err(|_| GraphicsError::Unsupported)?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|_| GraphicsError::Unsupported)?;

        let shaders = ShaderResources::new(&device)?;
        let uniform_slot = uniform_slot(&device);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            shaders,
            pipelines: HashMap::new(),
            uniform_slot,
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
        surface: WindowSurface<'_>,
        descriptor: PresentationTargetDescriptor,
    ) -> Result<SurfaceIdentity, GraphicsError> {
        let configuration = surface_configuration(&descriptor)?;

        let wgpu_surface = self.create_surface(&surface)?;
        if !wgpu_surface
            .get_capabilities(&self.adapter)
            .formats
            .contains(&configuration.format)
        {
            return Err(GraphicsError::Unsupported);
        }
        wgpu_surface.configure(&self.device, &configuration);

        let identity = self.next_surface_identity();
        self.targets.insert(
            identity,
            PresentationTarget {
                surface: wgpu_surface,
                configuration,
                pending_commands: Vec::new(),
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

        let device = &self.device;
        let target = self
            .targets
            .get_mut(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;

        target.configuration.width = extent.width;
        target.configuration.height = extent.height;
        target.surface.configure(device, &target.configuration);

        Ok(())
    }

    fn allocate_texture(
        &mut self,
        descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, GraphicsError> {
        descriptor.validate()?;

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: descriptor.extent.width,
                height: descriptor.extent.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format(descriptor.format),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let identity = self.next_resource_identity();
        self.resources.insert(
            identity,
            TextureResource {
                texture,
                extent: descriptor.extent,
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
        self.validate_resource_identities(submission)?;

        if !self.targets.contains_key(&surface) {
            return Err(GraphicsError::ResourceNotFound);
        }

        self.apply_uploads(&submission.uploads)?;

        let target = self
            .targets
            .get_mut(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;
        target.pending_commands = submission.commands.clone();

        Ok(())
    }

    fn present(&mut self, surface: SurfaceIdentity) -> Result<(), GraphicsError> {
        let (format, extent, commands, frame) = {
            let target = self
                .targets
                .get(&surface)
                .ok_or(GraphicsError::ResourceNotFound)?;
            let format = target.configuration.format;
            let extent = Extent2d::new(target.configuration.width, target.configuration.height);
            let commands = target.pending_commands.clone();

            let frame = match target.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                wgpu::CurrentSurfaceTexture::Lost => return Err(GraphicsError::DeviceLost),
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Outdated
                | wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(GraphicsError::SubmissionRejected);
                }
            };

            (format, extent, commands, frame)
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.encode_frame(&view, format, extent, &commands)?;
        self.queue.present(frame);

        Ok(())
    }
}

/// Rejects a zero or over-bound presentation extent.
///
/// A zero dimension cannot configure a surface, and a dimension above the M0
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

/// Builds a `wgpu` surface configuration from a presentation descriptor.
fn surface_configuration(
    descriptor: &PresentationTargetDescriptor,
) -> Result<wgpu::SurfaceConfiguration, GraphicsError> {
    validate_extent(descriptor.extent)?;

    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: texture_format(descriptor.format),
        color_space: wgpu::SurfaceColorSpace::Auto,
        width: descriptor.extent.width,
        height: descriptor.extent.height,
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: FRAME_LATENCY,
        alpha_mode: composite_alpha(descriptor.alpha_mode),
        view_formats: Vec::new(),
    })
}

/// Returns the dynamic uniform buffer stride for the device, rounded up to the
/// device alignment.
fn uniform_slot(device: &wgpu::Device) -> u64 {
    let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(1);
    UNIFORM_BYTES.div_ceil(alignment) * alignment
}

/// Returns the byte offset of a uniform slot, rejecting an offset that does not
/// fit a dynamic offset.
fn slot_offset(index: usize, slot: usize) -> Result<u32, GraphicsError> {
    let offset = index
        .checked_mul(slot)
        .ok_or(GraphicsError::SubmissionRejected)?;
    u32::try_from(offset).map_err(|_| GraphicsError::SubmissionRejected)
}

/// Appends one draw uniform (three `vec4<f32>`: rect, source, color) and pads it
/// to the slot stride. Returns the slot index of the appended uniform. The solid
/// path passes a zero source region, which its shader ignores.
fn push_uniform(
    buffer: &mut Vec<u8>,
    rect: [f32; 4],
    source: [f32; 4],
    color: [f32; 4],
    slot: usize,
) -> usize {
    let index = buffer.len() / slot;
    for value in rect.iter().chain(source.iter()).chain(color.iter()) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    buffer.resize((index + 1) * slot, 0);
    index
}

/// Converts a logical-pixel rectangle to a normalized device rectangle.
///
/// The target extent is validated to be nonzero before this runs, so the
/// division is well defined. The returned corners are `(x0, y0, x1, y1)`.
fn rect_to_ndc(rect: Rect, extent: Extent2d) -> [f32; 4] {
    let width = extent.width as f32;
    let height = extent.height as f32;

    let x0 = rect.x / width * 2.0 - 1.0;
    let x1 = (rect.x + rect.width) / width * 2.0 - 1.0;
    let y0 = 1.0 - rect.y / height * 2.0;
    let y1 = 1.0 - (rect.y + rect.height) / height * 2.0;

    [x0, y0, x1, y1]
}

/// Converts a source rectangle in texels to normalized texture coordinates.
///
/// The texture extent is nonzero because it passed descriptor validation at
/// allocation. The returned corners are `(u0, v0, u1, v1)`.
fn source_to_uv(source: Rect, texture: Extent2d) -> [f32; 4] {
    let width = texture.width as f32;
    let height = texture.height as f32;

    let u0 = source.x / width;
    let u1 = (source.x + source.width) / width;
    let v0 = source.y / height;
    let v1 = (source.y + source.height) / height;

    [u0, v0, u1, v1]
}

/// Returns the four color components in channel order.
fn color_components(color: Color) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

/// Maps a Panther format class to a `wgpu` texture format.
///
/// The exhaustive match forces a review when a new format class is added.
fn texture_format(format: TextureFormatClass) -> wgpu::TextureFormat {
    match format {
        TextureFormatClass::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        TextureFormatClass::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        TextureFormatClass::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        TextureFormatClass::R8Unorm => wgpu::TextureFormat::R8Unorm,
    }
}

/// Maps a Panther alpha mode to a `wgpu` composite alpha mode.
fn composite_alpha(mode: AlphaMode) -> wgpu::CompositeAlphaMode {
    match mode {
        AlphaMode::Opaque => wgpu::CompositeAlphaMode::Opaque,
        AlphaMode::Premultiplied => wgpu::CompositeAlphaMode::PreMultiplied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purr_graphics::{Color, ColorSpace, FrameToken, SceneGeneration, SceneId, SceneIdentity};

    /// Builds a backend when a GPU adapter is present, or returns `None` so the
    /// caller skips the test on a machine without a usable adapter.
    fn backend_or_skip() -> Option<WgpuBackend> {
        match WgpuBackend::create(BackendKind::Hardware) {
            Ok(backend) => Some(backend),
            Err(GraphicsError::Unsupported) => {
                eprintln!("skipping wgpu test: no adapter available");
                None
            }
            Err(other) => panic!("unexpected backend creation error: {other}"),
        }
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

    #[test]
    fn software_selection_is_unsupported() {
        assert_eq!(
            WgpuBackend::create(BackendKind::Software).unwrap_err(),
            GraphicsError::Unsupported
        );
    }

    #[test]
    fn allocate_texture_stamps_current_generation() {
        let Some(mut backend) = backend_or_skip() else {
            return;
        };

        let identity = backend
            .allocate_texture(&texture_descriptor(16, 16))
            .expect("valid descriptor allocates");

        assert_eq!(identity.resource_kind(), ResourceKind::Texture);
        assert_eq!(identity.device_generation(), backend.device_generation());
    }

    #[test]
    fn allocate_texture_rejects_zero_extent() {
        let Some(mut backend) = backend_or_skip() else {
            return;
        };

        assert_eq!(
            backend
                .allocate_texture(&texture_descriptor(0, 16))
                .unwrap_err(),
            GraphicsError::InvalidDescriptor
        );
    }

    #[test]
    fn submit_rejects_stale_device_generation() {
        let Some(mut backend) = backend_or_skip() else {
            return;
        };

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

        let surface = SurfaceIdentity::new(
            SurfaceId::new(1),
            SurfaceGeneration::new(1),
            ProducerNamespace::new(BACKEND_NAMESPACE),
        );
        let submission = FrameSubmission {
            frame_token: FrameToken::new(1),
            scene: SceneIdentity::new(
                SceneId::new(1),
                SceneGeneration::new(1),
                surface.surface_id(),
                surface.surface_generation(),
            ),
            target: PresentationTargetDescriptor {
                extent: Extent2d::new(64, 64),
                format: TextureFormatClass::Rgba8Unorm,
                alpha_mode: AlphaMode::Opaque,
            },
            uploads: vec![ResourceUpload {
                resource: stale_texture,
                descriptor: texture_descriptor(1, 1),
                pixels: vec![0u8; 4],
            }],
            commands: Vec::new(),
        };

        assert_eq!(
            backend.submit(surface, &submission).unwrap_err(),
            GraphicsError::DeviceLost
        );
    }

    fn coverage_descriptor(width: u32, height: u32) -> TextureDescriptor {
        TextureDescriptor {
            extent: Extent2d::new(width, height),
            format: TextureFormatClass::R8Unorm,
            color_space: ColorSpace::LinearSrgb,
            alpha_mode: AlphaMode::Premultiplied,
            label: None,
        }
    }

    #[test]
    fn encode_frame_draws_the_op_set() {
        let Some(mut backend) = backend_or_skip() else {
            return;
        };

        // The textured quad samples a single-channel coverage atlas, matching the
        // glyph atlas the paint stage produces.
        let texture = backend
            .allocate_texture(&coverage_descriptor(4, 4))
            .expect("valid descriptor allocates");

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let extent = Extent2d::new(64, 64);
        let target = backend.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("purr-graphics-wgpu-test-target"),
            size: wgpu::Extent3d {
                width: extent.width,
                height: extent.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let commands = vec![
            DrawCommand::Clear {
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            },
            DrawCommand::FillRect {
                rect: Rect::new(8.0, 8.0, 24.0, 24.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            },
            DrawCommand::TexturedQuad {
                rect: Rect::new(32.0, 32.0, 16.0, 16.0),
                texture,
                source: Rect::new(0.0, 0.0, 4.0, 4.0),
                color: Color::new(0.0, 0.0, 0.0, 1.0),
            },
        ];

        backend
            .encode_frame(&view, format, extent, &commands)
            .expect("the op set encodes without error");
    }
}
