// @file engines/purr/graphics-wgpu/src/shaders/solid-color.wgsl
// @description Fills a rectangle with one solid color for the SolidColor pipeline.
// @created Diego Martín Lafuente <meerita@icloud.com>

// The draw uniform is shared with the textured pipeline, so both bind one layout
// and one dynamic buffer stride. The rectangle is in normalized device
// coordinates; `source` is the textured-quad sample region, unused here; `color`
// is the fill color in straight alpha. Clear paints the full target as a
// rectangle that covers the whole clip space.
struct DrawUniform {
    rect: vec4<f32>,
    source: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> draw: DrawUniform;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(draw.rect.x, draw.rect.y),
        vec2<f32>(draw.rect.z, draw.rect.y),
        vec2<f32>(draw.rect.x, draw.rect.w),
        vec2<f32>(draw.rect.z, draw.rect.w),
    );
    var order = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let corner = corners[order[vertex_index]];
    return vec4<f32>(corner, 0.0, 1.0);
}

// The color target blends premultiplied alpha, so the fragment premultiplies the
// straight-alpha fill color. An opaque fill is unchanged (rgb times one).
@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(draw.color.rgb * draw.color.a, draw.color.a);
}
