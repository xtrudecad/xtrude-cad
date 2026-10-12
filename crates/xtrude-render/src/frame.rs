// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What one frame of the viewport draws, as plain data: vertex arrays and
//! an ordered list of draws with their state.
//! The app fills it from the document; [`crate::gpu`] executes it.
//!
//! Positions are `f32` RELATIVE to [`FrameData::origin`] (the model
//! geometry is `f64`): the matrices are composed in `f64` with the origin
//! folded in, so a part far from the world origin keeps its precision.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{DMat4, DVec3};

/// A face-pass vertex: position, the colour of the front side and of the
/// back side (alpha carries the run's opacity).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FaceVertex {
    pub pos: [f32; 3],
    pub front: [f32; 4],
    pub back: [f32; 4],
}

/// A line-pass vertex.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

/// A vertex array that keeps its GPU copy while `generation` stays the
/// same — the scene's faces and edges are uploaded once per edit, not per
/// frame. Two are equal when they are the same array: the same generation
/// of the same allocation, never compared vertex by vertex.
#[derive(Clone, Debug, Default)]
pub struct Cached<T> {
    pub generation: u64,
    pub data: Arc<Vec<T>>,
}

impl<T> PartialEq for Cached<T> {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && Arc::ptr_eq(&self.data, &other.data)
    }
}

/// The depth state of a draw (GL `glDepthFunc` / `glDepthMask`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Depth {
    /// LEQUAL, writing depth: model geometry.
    #[default]
    TestWrite,
    /// LEQUAL, depth read-only: axes, guides, X-ray faces.
    TestOnly,
    /// No depth at all: the sky, the rubber band.
    Off,
    /// GREATER, read-only: what lies BEHIND the faces (back edges, X-ray).
    Behind,
}

/// The screen pattern a draw is masked with (`u_stipple`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stipple {
    #[default]
    None = 0,
    /// Hidden objects shown: a 1 px grid every 4 px.
    Grid = 1,
    /// Hidden geometry shown: dotted.
    Dots = 2,
    /// Selected faces: a 2×2 px dot every 6 px.
    Selection = 3,
    /// Back Edges: a dash measured along the line.
    Dash = 4,
}

/// Which vertex array a draw reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The scene's faces ([`FrameData::scene_faces`]).
    SceneFaces,
    /// The scene's edges ([`FrameData::scene_lines`]).
    SceneLines,
    /// This frame's faces ([`FrameData::faces`]).
    Faces,
    /// This frame's lines ([`FrameData::lines`]).
    Lines,
    /// The groups moving in a live preview ([`FrameData::preview_faces`]).
    PreviewFaces,
    /// Their edges ([`FrameData::preview_lines`]).
    PreviewLines,
}

impl Source {
    pub fn is_lines(self) -> bool {
        matches!(
            self,
            Source::SceneLines | Source::Lines | Source::PreviewLines
        )
    }
}

/// One draw call and the state it runs with.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub source: Source,
    pub first: u32,
    pub count: u32,
    pub depth: Depth,
    /// Polygon offset (faces sit behind their own edges).
    pub bias: bool,
    pub stipple: Stipple,
    /// Model space (`mvp`) or normalised device coordinates (identity).
    pub ndc: bool,
    /// The active section cut applies.
    pub clip: bool,
    /// `u_fade`: washed toward [`FrameData::fade_color`].
    pub fade: f32,
    /// Multiplies every alpha (X-ray, translucent runs).
    pub opacity: f32,
    /// A colour that replaces the vertices' (`u_color`), front and back.
    pub color: Option<[f32; 4]>,
    /// The Back Edges dash, px.
    pub dash_px: f32,
    /// Depth only, no colour (X-ray lays down its faces' depth this way).
    pub depth_only: bool,
    /// A world-space transform applied before `view_proj` (the groups
    /// preview: the moving groups' buffers, uploaded once, ride it).
    pub model: Option<DMat4>,
}

impl Draw {
    pub fn new(source: Source, first: u32, count: u32) -> Draw {
        Draw {
            source,
            first,
            count,
            depth: Depth::TestWrite,
            bias: false,
            stipple: Stipple::None,
            ndc: false,
            clip: false,
            fade: 0.0,
            opacity: 1.0,
            color: None,
            dash_px: 4.0,
            depth_only: false,
            model: None,
        }
    }

    pub fn model(mut self, m: DMat4) -> Draw {
        self.model = Some(m);
        self
    }

    pub fn depth_only(mut self) -> Draw {
        self.depth_only = true;
        self
    }

    pub fn depth(mut self, d: Depth) -> Draw {
        self.depth = d;
        self
    }

    pub fn bias(mut self) -> Draw {
        self.bias = true;
        self
    }

    pub fn stipple(mut self, s: Stipple) -> Draw {
        self.stipple = s;
        self
    }

    pub fn ndc(mut self) -> Draw {
        self.ndc = true;
        self.depth = Depth::Off;
        self
    }

    pub fn clipped(mut self, on: bool) -> Draw {
        self.clip = on;
        self
    }

    pub fn color(mut self, c: [f32; 4]) -> Draw {
        self.color = Some(c);
        self
    }

    pub fn fade(mut self, f: f32) -> Draw {
        self.fade = f;
        self
    }

    pub fn opacity(mut self, o: f32) -> Draw {
        self.opacity = o;
        self
    }
}

/// Everything one frame draws. Two equal frames draw the same image.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameData {
    /// Framebuffer size, px.
    pub size: [u32; 2],
    pub background: [f32; 3],
    /// The world point positions are relative to.
    pub origin: DVec3,
    /// Projection × view (GL conventions, z in −1..1), world space.
    pub view_proj: DMat4,
    /// The section cut, world space: kept where `n·p + d ≥ 0`.
    pub clip_plane: Option<[f64; 4]>,
    pub fade_color: [f32; 3],
    pub scene_faces: Cached<FaceVertex>,
    pub scene_lines: Cached<LineVertex>,
    pub preview_faces: Cached<FaceVertex>,
    pub preview_lines: Cached<LineVertex>,
    pub faces: Vec<FaceVertex>,
    pub lines: Vec<LineVertex>,
    pub draws: Vec<Draw>,
}

impl FrameData {
    /// A world point as a vertex position.
    pub fn local(&self, p: DVec3) -> [f32; 3] {
        let d = p - self.origin;
        [d.x as f32, d.y as f32, d.z as f32]
    }

    /// Append a line segment to this frame's lines; returns its first vertex.
    pub fn push_line(&mut self, a: DVec3, b: DVec3, color: [f32; 4]) -> u32 {
        let first = self.lines.len() as u32;
        self.lines.push(LineVertex {
            pos: self.local(a),
            color,
        });
        self.lines.push(LineVertex {
            pos: self.local(b),
            color,
        });
        first
    }

    /// The matrix the shader gets: `view_proj` with the origin folded in,
    /// z remapped from GL's −1..1 to wgpu's 0..1.
    pub fn shader_mvp(&self) -> [[f32; 4]; 4] {
        self.shader_mvp_for(None)
    }

    /// [`shader_mvp`](Self::shader_mvp) with a world-space `model`
    /// transform applied first.
    pub fn shader_mvp_for(&self, model: Option<DMat4>) -> [[f32; 4]; 4] {
        let gl_to_wgpu = DMat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 0.5, 0.0, //
            0.0, 0.0, 0.5, 1.0,
        ]);
        let m = gl_to_wgpu
            * self.view_proj
            * model.unwrap_or(DMat4::IDENTITY)
            * DMat4::from_translation(self.origin);
        m.to_cols_array_2d().map(|c| c.map(|x| x as f32))
    }

    /// The clip plane in the local frame of vertices drawn through a
    /// world-space `model` transform: for `w' = M·(origin + p)`, the plane
    /// `n·w' + d` becomes `(Aᵀn)·p + (n·M(origin) + d)`.
    pub fn local_clip_for(&self, model: Option<DMat4>) -> Option<[f32; 4]> {
        let [a, b, c, d] = self.clip_plane?;
        let n = DVec3::new(a, b, c);
        let m = model.unwrap_or(DMat4::IDENTITY);
        let nl = glam::DMat3::from_mat4(m).transpose() * n;
        let d2 = d + n.dot(m.transform_point3(self.origin));
        Some([nl.x as f32, nl.y as f32, nl.z as f32, d2 as f32])
    }
}
