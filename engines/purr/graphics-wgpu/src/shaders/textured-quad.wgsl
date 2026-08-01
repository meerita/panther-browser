// @file engines/purr/graphics-wgpu/src/shaders/textured-quad.wgsl
// @description Samples a texture region over a destination quad for the TexturedQuad pipeline.
// @created Diego Martín Lafuente <meerita@icloud.com>

// The destination rectangle is in normalized device coordinates and the source
// rectangle is in texture coordinates. Both are supplied by the backend.
struct TexturedQuadUniform {
    dest: vec4<f32>,
    source: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> quad: TexturedQuadUniform;

@group(1) @binding(0)
var quad_texture: texture_2d<f32>;
@group(1) @binding(1)
var quad_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(quad.dest.x, quad.dest.y),
        vec2<f32>(quad.dest.z, quad.dest.y),
        vec2<f32>(quad.dest.x, quad.dest.w),
        vec2<f32>(quad.dest.z, quad.dest.w),
    );
    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(quad.source.x, quad.source.y),
        vec2<f32>(quad.source.z, quad.source.y),
        vec2<f32>(quad.source.x, quad.source.w),
        vec2<f32>(quad.source.z, quad.source.w),
    );
    var order = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let index = order[vertex_index];
    var out: VertexOutput;
    out.position = vec4<f32>(corners[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(quad_texture, quad_sampler, in.uv);
}
