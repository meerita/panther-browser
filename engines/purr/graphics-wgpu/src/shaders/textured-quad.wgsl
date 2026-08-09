// @file engines/purr/graphics-wgpu/src/shaders/textured-quad.wgsl
// @description Samples a texture region over a destination quad for the TexturedQuad pipeline.
// @created Diego Martín Lafuente <meerita@icloud.com>

// The draw uniform is shared with the solid pipeline. `rect` is the destination
// in normalized device coordinates, `source` is the sample region in texture
// coordinates, and `color` is the text color in straight alpha.
struct DrawUniform {
    rect: vec4<f32>,
    source: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> quad: DrawUniform;

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
        vec2<f32>(quad.rect.x, quad.rect.y),
        vec2<f32>(quad.rect.z, quad.rect.y),
        vec2<f32>(quad.rect.x, quad.rect.w),
        vec2<f32>(quad.rect.z, quad.rect.w),
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

// The texture is a single-channel coverage mask: its red channel is the glyph
// coverage. The fragment colorizes that coverage with the straight-alpha text
// color and returns a premultiplied result for the premultiplied color target,
// so anti-aliased edges blend without the dark rim a straight-alpha mask leaves.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(quad_texture, quad_sampler, in.uv).r;
    let alpha = quad.color.a * coverage;
    return vec4<f32>(quad.color.rgb * alpha, alpha);
}
