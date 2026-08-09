// @file engines/purr/graphics-wgpu/src/render-pipelines.rs
// @description Compiles the WGSL shaders and builds the M0 render pipelines.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! WGSL shader compilation and pipeline construction.
//!
//! The two M0 pipelines are authored in WGSL and translated by `naga` (bundled
//! with `wgpu`). The shader modules, bind group layouts, pipeline layouts, and
//! the shared sampler are format independent and built once. The render
//! pipelines bind a color-target format, so they are built per surface format
//! and cached by the backend. Any shader or pipeline compile failure is
//! translated to `GraphicsError::Unsupported` at this boundary; no `naga` or
//! `wgpu` error text is exposed.

use purr_graphics::GraphicsError;

/// Byte size of one draw uniform (three `vec4<f32>`: rect, source, color).
///
/// Both pipelines share this layout. The solid pipeline leaves the source region
/// unused; the textured pipeline uses all three.
pub(crate) const UNIFORM_BYTES: u64 = 48;

/// Vertex shader entry point shared by both pipelines.
const VERTEX_ENTRY: &str = "vs_main";

/// Fragment shader entry point shared by both pipelines.
const FRAGMENT_ENTRY: &str = "fs_main";

/// Format independent shader and layout resources.
///
/// These are built once at device creation. The dynamic uniform layout is
/// shared by both pipelines; the texture layout is used only by the textured
/// pipeline.
#[derive(Debug)]
pub(crate) struct ShaderResources {
    solid_module: wgpu::ShaderModule,
    textured_module: wgpu::ShaderModule,
    uniform_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    solid_pipeline_layout: wgpu::PipelineLayout,
    textured_pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
}

/// Render pipelines for one color-target format.
///
/// Both pipelines are cheap `Arc` handles, so a cache hit clones them without a
/// GPU allocation.
#[derive(Debug, Clone)]
pub(crate) struct FormatPipelines {
    pub(crate) solid: wgpu::RenderPipeline,
    pub(crate) textured: wgpu::RenderPipeline,
}

impl ShaderResources {
    /// Compiles the WGSL modules and builds the shared layouts.
    ///
    /// A shader compile failure is captured by a validation error scope and
    /// reported as `Unsupported`.
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, GraphicsError> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let solid_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("purr-graphics-wgpu-solid-color"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/solid-color.wgsl").into()),
        });
        let textured_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("purr-graphics-wgpu-textured-quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/textured-quad.wgsl").into()),
        });

        if pollster::block_on(scope.pop()).is_some() {
            return Err(GraphicsError::Unsupported);
        }

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("purr-graphics-wgpu-draw-uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
                count: None,
            }],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("purr-graphics-wgpu-texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let solid_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("purr-graphics-wgpu-solid-color"),
                bind_group_layouts: &[Some(&uniform_layout)],
                immediate_size: 0,
            });
        let textured_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("purr-graphics-wgpu-textured-quad"),
                bind_group_layouts: &[Some(&uniform_layout), Some(&texture_layout)],
                immediate_size: 0,
            });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("purr-graphics-wgpu-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(Self {
            solid_module,
            textured_module,
            uniform_layout,
            texture_layout,
            solid_pipeline_layout,
            textured_pipeline_layout,
            sampler,
        })
    }

    /// Returns the shared dynamic uniform layout.
    pub(crate) fn uniform_layout(&self) -> &wgpu::BindGroupLayout {
        &self.uniform_layout
    }

    /// Returns the texture and sampler layout.
    pub(crate) fn texture_layout(&self) -> &wgpu::BindGroupLayout {
        &self.texture_layout
    }

    /// Returns the shared sampler.
    pub(crate) fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// Builds the two render pipelines for one color-target format.
    ///
    /// A pipeline compile failure is captured by a validation error scope and
    /// reported as `Unsupported`.
    pub(crate) fn pipelines_for(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Result<FormatPipelines, GraphicsError> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let targets = [Some(color_target(format))];

        let solid = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("purr-graphics-wgpu-solid-color"),
            layout: Some(&self.solid_pipeline_layout),
            vertex: vertex_state(&self.solid_module),
            primitive: primitive_state(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.solid_module,
                entry_point: Some(FRAGMENT_ENTRY),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let textured = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("purr-graphics-wgpu-textured-quad"),
            layout: Some(&self.textured_pipeline_layout),
            vertex: vertex_state(&self.textured_module),
            primitive: primitive_state(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.textured_module,
                entry_point: Some(FRAGMENT_ENTRY),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        if pollster::block_on(scope.pop()).is_some() {
            return Err(GraphicsError::Unsupported);
        }

        Ok(FormatPipelines { solid, textured })
    }
}

/// Builds the vertex stage for a module. Both pipelines draw without a vertex
/// buffer; the vertex shader generates the quad from the vertex index.
fn vertex_state(module: &wgpu::ShaderModule) -> wgpu::VertexState<'_> {
    wgpu::VertexState {
        module,
        entry_point: Some(VERTEX_ENTRY),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        buffers: &[],
    }
}

/// The M0 primitive state: a triangle list with culling off, so quad winding
/// does not matter.
fn primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        cull_mode: None,
        ..Default::default()
    }
}

/// The color target with premultiplied alpha blending for the given format.
///
/// Both pipelines output premultiplied color, so the source factor is `One`. An
/// opaque draw (alpha one) is identical to straight-alpha blending, so solid
/// fills and clears are unchanged; a partially covered glyph edge composites
/// correctly instead of darkening toward the cleared target.
fn color_target(format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    }
}
