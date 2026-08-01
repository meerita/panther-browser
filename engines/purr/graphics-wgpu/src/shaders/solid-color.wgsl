// @file engines/purr/graphics-wgpu/src/shaders/solid-color.wgsl
// @description Fills a rectangle with one solid color for the SolidColor pipeline.
// @created Diego Martín Lafuente <meerita@icloud.com>

// The rectangle is supplied in normalized device coordinates. Clear paints the
// full target as a rectangle that covers the whole clip space.
struct SolidColorUniform {
    rect: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> solid: SolidColorUniform;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(solid.rect.x, solid.rect.y),
        vec2<f32>(solid.rect.z, solid.rect.y),
        vec2<f32>(solid.rect.x, solid.rect.w),
        vec2<f32>(solid.rect.z, solid.rect.w),
    );
    var order = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let corner = corners[order[vertex_index]];
    return vec4<f32>(corner, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return solid.color;
}
