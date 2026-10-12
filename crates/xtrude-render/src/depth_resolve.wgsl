// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
// The frame's depth packed into the RGB bytes of a plain colour target
// (24 bits, low byte first) for a read-back
// (`ViewportRenderer::read_depth`). The depth is bound as a plain float
// texture: GLSL fetches no depth texels.

@group(0) @binding(0) var depth: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // one triangle covering the target
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(pos.xy), 0).x;
    let v = u32(round(clamp(d, 0.0, 1.0) * 16777215.0));
    let b = vec3<u32>(v & 255u, (v >> 8u) & 255u, (v >> 16u) & 255u);
    return vec4<f32>(vec3<f32>(b) / 255.0, 1.0);
}
