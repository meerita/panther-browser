// @file engines/purr/graphics-wgpu/src/wgpu-backend.rs
// @description Implements the hardware graphics backend over wgpu.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Hardware graphics backend over `wgpu`.
//!
//! `WgpuBackend` implements the Panther `GraphicsBackend` contract with `wgpu`.
//! Every `wgpu` type stays inside this file. The public surface speaks only in
//! Panther identities, descriptors, and `GraphicsError`, so no backend or
//! native-API type escapes the interface. Each `wgpu` failure is translated to a
//! `GraphicsError` at the method boundary.
//!
//! At M0 the backend delivers the device, the presentation target, and the
//! present path. `submit` handles the `Clear` command only; resource uploads and
//! the pipeline draw commands arrive in a later phase and are reported as
//! `Unsupported` until then. `allocate_texture` is `Unsupported` for the same
//! reason.

use std::collections::HashMap;

use purr_graphics::{
    AlphaMode, BackendKind, Color, DeviceGeneration, DrawCommand, Extent2d, FrameSubmission,
    GpuResourceIdentity, GraphicsBackend, GraphicsError, MAX_TEXTURE_EXTENT,
    PresentationTargetDescriptor, ProducerNamespace, SurfaceGeneration, SurfaceId, SurfaceIdentity,
    TextureDescriptor, TextureFormatClass, WindowSurface,
};

/// Producer namespace this backend stamps on the surfaces it creates.
///
/// A single-process backend has one producer, so the namespace is fixed. A
/// multi-process split assigns namespaces per producer later.
const BACKEND_NAMESPACE: u32 = 1;

/// Number of monitor refreshes the presentation engine may buffer.
///
/// Two frames balance latency and throughput and match the `wgpu` default.
const FRAME_LATENCY: u32 = 2;

/// One live presentation target owned by the backend.
///
/// The backend keys these by `SurfaceIdentity`. The caller holds the identity
/// only and never a `wgpu` handle. The clear color is the last color a `Clear`
/// submission stored, so `present` paints a meaningful frame before the full
/// submission path exists.
#[derive(Debug)]
struct PresentationTarget {
    surface: wgpu::Surface<'static>,
    configuration: wgpu::SurfaceConfiguration,
    clear_color: wgpu::Color,
}

/// Hardware backend that presents through `wgpu`.
#[derive(Debug)]
pub struct WgpuBackend {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    device_generation: DeviceGeneration,
    targets: HashMap<SurfaceIdentity, PresentationTarget>,
    next_surface_id: u64,
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

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            device_generation: DeviceGeneration::new(1),
            targets: HashMap::new(),
            next_surface_id: 1,
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
                clear_color: wgpu::Color::BLACK,
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
        _descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, GraphicsError> {
        Err(GraphicsError::Unsupported)
    }

    fn submit(
        &mut self,
        surface: SurfaceIdentity,
        submission: &FrameSubmission,
    ) -> Result<(), GraphicsError> {
        let target = self
            .targets
            .get_mut(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;

        submission.validate()?;

        if !submission.uploads.is_empty() {
            return Err(GraphicsError::Unsupported);
        }

        let mut clear_color = None;
        for command in &submission.commands {
            match command {
                DrawCommand::Clear { color } => clear_color = Some(color_to_wgpu(*color)),
                DrawCommand::FillRect { .. } | DrawCommand::TexturedQuad { .. } => {
                    return Err(GraphicsError::Unsupported);
                }
            }
        }

        if let Some(color) = clear_color {
            target.clear_color = color;
        }

        Ok(())
    }

    fn present(&mut self, surface: SurfaceIdentity) -> Result<(), GraphicsError> {
        let target = self
            .targets
            .get(&surface)
            .ok_or(GraphicsError::ResourceNotFound)?;

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

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(target.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
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

    if extent.width > MAX_TEXTURE_EXTENT || extent.height > MAX_TEXTURE_EXTENT {
        return Err(GraphicsError::InvalidDescriptor);
    }

    Ok(())
}

/// Builds a `wgpu` surface configuration from a presentation descriptor.
///
/// The extent is validated before the configuration is built, so a zero or
/// over-bound extent never reaches `wgpu`.
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

/// Maps a Panther format class to a `wgpu` texture format.
///
/// The exhaustive match forces a review when a new format class is added.
fn texture_format(format: TextureFormatClass) -> wgpu::TextureFormat {
    match format {
        TextureFormatClass::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        TextureFormatClass::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        TextureFormatClass::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
    }
}

/// Maps a Panther alpha mode to a `wgpu` composite alpha mode.
fn composite_alpha(mode: AlphaMode) -> wgpu::CompositeAlphaMode {
    match mode {
        AlphaMode::Opaque => wgpu::CompositeAlphaMode::Opaque,
        AlphaMode::Premultiplied => wgpu::CompositeAlphaMode::PreMultiplied,
    }
}

/// Converts a Panther color to a `wgpu` color.
///
/// Panther colors are `f32` per channel; `wgpu` colors are `f64`. The widening
/// conversion is exact.
fn color_to_wgpu(color: Color) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(color.r),
        g: f64::from(color.g),
        b: f64::from(color.b),
        a: f64::from(color.a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_selection_is_unsupported() {
        assert_eq!(
            WgpuBackend::create(BackendKind::Software).unwrap_err(),
            GraphicsError::Unsupported
        );
    }

    #[test]
    fn hardware_backend_reports_device_generation() {
        match WgpuBackend::create(BackendKind::Hardware) {
            Ok(backend) => {
                assert_eq!(backend.device_generation(), DeviceGeneration::new(1));
            }
            Err(GraphicsError::Unsupported) => {
                eprintln!("skipping hardware backend smoke test: no wgpu adapter available");
            }
            Err(other) => panic!("unexpected backend creation error: {other}"),
        }
    }
}
