// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//
// The viewport's one program (resources/shaders/basic.vert + basic.frag):
// faces show their front or back colour by the side seen, lines their own;
// a section plane discards the cut side; stipples mask hidden things, the
// selection dots and the Back Edges dash; the context of an open group is
// washed toward the background.

struct Uniforms {
    mvp: mat4x4<f32>,
    // Section cut in the vertices' frame: kept where dot(n, p) + d >= 0.
    clip: vec4<f32>,
    // rgb = the colour the context is washed toward, a = u_fade.
    fade: vec4<f32>,
    // A colour replacing the vertices' (flags.z == 1).
    color: vec4<f32>,
    // x = opacity, y = dash px, zw = viewport px.
    params: vec4<f32>,
    // x = stipple, y = clip on, z = use `color`, w = NDC (no mvp).
    flags: vec4<u32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct FaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) front: vec4<f32>,
    @location(1) back: vec4<f32>,
    @location(2) clip_dist: f32,
};

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) clip_dist: f32,
    // The provoking vertex's clip position: the Back Edges dash measures
    // along the line from it.
    @location(2) @interpolate(flat) line_clip: vec4<f32>,
};

fn project(p: vec3<f32>) -> vec4<f32> {
    if u.flags.w == 1u {
        return vec4<f32>(p, 1.0);
    }
    return u.mvp * vec4<f32>(p, 1.0);
}

fn clip_distance(p: vec3<f32>) -> f32 {
    if u.flags.y == 1u {
        return dot(vec4<f32>(p, 1.0), u.clip);
    }
    return 1.0;
}

@vertex
fn vs_face(@location(0) pos: vec3<f32>, @location(1) front: vec4<f32>, @location(2) back: vec4<f32>) -> FaceOut {
    var out: FaceOut;
    out.pos = project(pos);
    out.front = front;
    out.back = back;
    out.clip_dist = clip_distance(pos);
    return out;
}

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var out: LineOut;
    out.pos = project(pos);
    out.color = color;
    out.clip_dist = clip_distance(pos);
    out.line_clip = out.pos;
    return out;
}

// GLSL mod(): x - y * floor(x / y), never negative for y > 0.
fn gmod(x: f32, y: f32) -> f32 {
    return x - y * floor(x / y);
}

fn masked(frag: vec4<f32>) -> bool {
    let s = u.flags.x;
    if s == 1u {
        return gmod(frag.x, 4.0) >= 1.0 && gmod(frag.y, 4.0) >= 1.0;
    } else if s == 2u {
        return gmod(frag.x + frag.y, 6.0) >= 3.0;
    } else if s == 3u {
        return gmod(frag.x, 6.0) >= 2.0 || gmod(frag.y, 6.0) >= 2.0;
    }
    return false;
}

fn finish(c: vec4<f32>) -> vec4<f32> {
    let rgb = mix(c.rgb, u.fade.rgb, u.fade.a);
    return vec4<f32>(rgb, c.a * u.params.x);
}

@fragment
fn fs_face(in: FaceOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if in.clip_dist < 0.0 || masked(in.pos) {
        discard;
    }
    var c = in.back;
    if front_facing {
        c = in.front;
    }
    if u.flags.z == 1u {
        c = u.color;
    }
    return finish(c);
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    if in.clip_dist < 0.0 || masked(in.pos) {
        discard;
    }
    if u.flags.x == 4u {
        // Window y runs down in wgpu; NDC y up.
        let ndc = in.line_clip.xy / in.line_clip.w;
        let o = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * u.params.zw;
        if gmod(length(in.pos.xy - o), 2.0 * u.params.y) >= u.params.y {
            discard;
        }
    }
    var c = in.color;
    if u.flags.z == 1u {
        c = u.color;
    }
    return finish(c);
}
