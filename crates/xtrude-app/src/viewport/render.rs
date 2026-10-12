// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport as a frame to draw: the scene's faces and edges, cached
//! until the document changes, and the per-frame passes in order — sky,
//! faces, the default back, translucent runs, selection and hover dots,
//! previews, ghosts, axes, guides, the edit box, edges, profiles, the
//! selected and hovered edges, the depth-tested rubber band. What is
//! painted in 2D on top is in [`super::overlay`].

use std::collections::HashMap;
use std::sync::Arc;
use xtrude_core::Axis;

use glam::{DMat4, DVec3};
use xtrude_core::attrs::{Back, FaceAttrs};
use xtrude_core::math::{newell, MatExt, VecExt};
use xtrude_core::triangulate::triangulate;
use xtrude_core::{EdgeId, FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::materials::effective_attrs;
use xtrude_model::style::{effective_back_color, Style};
use xtrude_render::frame::{
    Cached, Depth, Draw, FaceVertex, FrameData, LineVertex, Source, Stipple,
};
use xtrude_render::lines::guide_vertices;

use super::pick::layer_state;
use super::RestMode;
use super::Viewport;
use crate::tools::paint::DEFAULT_FACE_COLOR;

/// The fixed world light of the matte shading.
fn light() -> DVec3 {
    DVec3::new(0.35, 0.25, 1.0).normalize()
}

/// `EDIT_REST_FADE`: how far the context of an open group is washed out.
pub const EDIT_REST_FADE: f32 = 0.75;
/// `XRAY_FACE_OPACITY`.
pub const XRAY_FACE_OPACITY: f32 = 0.6;
/// `XRAY_HIDDEN_EDGE_FADE`.
pub const XRAY_HIDDEN_EDGE_FADE: f64 = 0.6;
const SELECTION: [f32; 4] = [0.95, 0.45, 0.16, 1.0];
const HOVER_EDGE: [f32; 4] = [0.30, 0.55, 0.95, 1.0];
const GUIDE: [f32; 4] = [0.27, 0.35, 0.47, 1.0];
const EDIT_BOX: [f32; 4] = [90.0 / 255.0, 110.0 / 255.0, 140.0 / 255.0, 1.0];
const AXES: [(Axis, [f32; 4]); 3] = [
    (Axis::X, [0.86, 0.22, 0.27, 1.0]),
    (Axis::Y, [0.16, 0.62, 0.36, 1.0]),
    (Axis::Z, [0.20, 0.40, 0.78, 1.0]),
];
const SKY_GRAD_SPAN: f64 = 1.4;
const GROUND_GRAD_SPAN: f64 = 0.9;

/// The backdrop's corner colours ([`sky_gradient`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyGradient {
    pub sky_bottom: [f64; 3],
    pub sky_top: [f64; 3],
    pub ground_top: [f64; 3],
    pub ground_bottom: [f64; 3],
    pub horizon_line: [f64; 3],
}

/// The backdrop's corner colours — the style's sky tone
/// at the zenith and ground tone underfoot, both hazed toward white at the
/// horizon, against the UNCLAMPED horizon `hy`.
pub fn sky_gradient(sky: [f64; 3], ground: [f64; 3], hy: f64) -> SkyGradient {
    let white = [1.0; 3];
    let sky_h = mix3(sky, white, 0.55);
    let gnd_h = mix3(ground, white, 0.30);
    let sky_at = |y: f64| mix3(sky_h, sky, ((y - hy) / SKY_GRAD_SPAN).clamp(0.0, 1.0));
    let gnd_at = |y: f64| mix3(gnd_h, ground, ((hy - y) / GROUND_GRAD_SPAN).clamp(0.0, 1.0));
    let y0 = hy.clamp(-1.0, 1.0);
    let line = mix3(mix3(sky_h, gnd_h, 0.5), [0.0; 3], 0.12);
    SkyGradient {
        sky_bottom: sky_at(y0),
        sky_top: sky_at(1.0),
        ground_top: gnd_at(y0),
        ground_bottom: gnd_at(-1.0),
        horizon_line: line,
    }
}

/// 0.62..1.0 against the light, by the face's plane,
/// quantised to 1/32.
pub fn shade_factor(normal: DVec3) -> f64 {
    if normal.length() < 1e-9 {
        return 0.84375;
    }
    let d = normal.normalize().dot(light()).abs();
    ((0.62 + 0.38 * d) * 32.0).round() / 32.0
}

/// `base` under the light, quantised to 1/64.
pub fn shaded_color(base: [f64; 3], normal: DVec3) -> [f64; 3] {
    let s = shade_factor(normal);
    base.map(|c| ((c * s).min(1.0) * 64.0).round() / 64.0)
}

fn rgba(c: [f64; 3], a: f64) -> [f32; 4] {
    [c[0] as f32, c[1] as f32, c[2] as f32, a as f32]
}

fn mix3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The soft edges, for the profile test. Each face's plane is kept once
/// (on a smooth surface a face bounds two soft edges), so the test checks
/// each face against the eye once; it runs again only when the eye moves.
#[derive(Default)]
struct SoftEdges {
    /// A face's plane: its Newell normal and its first point.
    planes: Vec<(DVec3, DVec3)>,
    /// Per edge, its faces' planes; no second one = an open boundary,
    /// always drawn.
    faces: Vec<(u32, Option<u32>)>,
    /// Per edge, its endpoints.
    ends: Vec<(DVec3, DVec3)>,
    /// The edges drawn as profiles from `profile_eye`.
    profiles: Vec<u32>,
    profile_eye: Option<DVec3>,
}

impl SoftEdges {
    /// Add soft edge `e` of `m` (endpoints `a`, `b`, placed) unless it bounds
    /// no face or more than two. `seen`: the planes of `m`'s faces already
    /// kept for this placement.
    fn push(
        &mut self,
        m: &Mesh,
        e: EdgeId,
        (a, b): (DVec3, DVec3),
        map: &dyn Fn(DVec3) -> DVec3,
        seen: &mut HashMap<FaceId, u32>,
    ) {
        let faces = &m.edge(e).faces;
        if faces.is_empty() || faces.len() > 2 {
            return;
        }
        let mut plane = |f: FaceId| {
            *seen.entry(f).or_insert_with(|| {
                let ps: Vec<DVec3> = m.face_positions(f).into_iter().map(map).collect();
                self.planes.push((newell(&ps), ps[0]));
                self.planes.len() as u32 - 1
            })
        };
        let f0 = plane(faces[0]);
        let f1 = (faces.len() == 2).then(|| plane(faces[1]));
        self.faces.push((f0, f1));
        self.ends.push((a, b));
    }

    /// Find the profiles seen from `eye`: the edges between a face seen from
    /// the front and one seen from the back, and the open boundaries.
    fn sync_profiles(&mut self, eye: DVec3) {
        if self.profile_eye == Some(eye) {
            return;
        }
        let behind: Vec<bool> = self
            .planes
            .iter()
            .map(|&(n, c)| n.dot(c - eye) < 0.0)
            .collect();
        self.profiles.clear();
        for (k, &(f0, f1)) in self.faces.iter().enumerate() {
            let profile = match f1 {
                None => true,
                Some(f1) => behind[f0 as usize] != behind[f1 as usize],
            };
            if profile {
                self.profiles.push(k as u32);
            }
        }
        self.profile_eye = Some(eye);
    }
}

/// The document as vertex arrays, rebuilt when its key changes.
#[derive(Default)]
pub struct RenderCache {
    key: Option<String>,
    generation: u64,
    origin: DVec3,
    faces: Arc<Vec<FaceVertex>>,
    lines: Arc<Vec<LineVertex>>,
    /// Faces: [0, ctx) the faded context, [ctx, opaque) the subject,
    /// [opaque, ghosts) translucent, [ghosts, len) hidden objects shown.
    face_ctx: u32,
    face_opaque: u32,
    face_ghosts: u32,
    /// Lines: [0, ctx) context, [ctx, ghosts) subject, [ghosts, len) ghosts.
    line_ctx: u32,
    line_ghosts: u32,
    soft: SoftEdges,
    /// The scene's faces in world coordinates, for the selection dots.
    selected_faces_world: Vec<(MeshId, FaceId, Option<DMat4>)>,
    /// The groups moving in a live preview: their buffers, built once per
    /// drag and drawn through the preview matrix.
    preview_key: Option<String>,
    preview_generation: u64,
    preview_faces: Arc<Vec<FaceVertex>>,
    preview_lines: Arc<Vec<LineVertex>>,
    /// The last frame handed out by [`Viewport::changed_frame`]: what the
    /// screen shows.
    on_screen: Option<FrameData>,
}

/// One mesh the scene draws, and how.
struct Piece<'a> {
    mesh_id: MeshId,
    mesh: &'a Mesh,
    xform: Option<DMat4>,
    material: Option<xtrude_core::attrs::Paint>,
    context: bool,
    ghost: bool,
}

impl Viewport {
    /// The scene groups a live preview draws apart (none for Paste's
    /// clipboard templates, which are not in the scene).
    fn scene_preview_groups(&self) -> &[xtrude_model::ids::GroupId] {
        if self.preview.external {
            &[]
        } else {
            &self.preview.groups
        }
    }

    fn render_key(&self, style: &Style) -> String {
        let s = &self.scene;
        format!(
            "{}|{}|{:?}|{:?}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}",
            s.version,
            s.mesh().mut_serial,
            s.edit_stack.iter().map(|l| l.group).collect::<Vec<_>>(),
            self.prefs.edit_rest_mode,
            s.show_hidden_objects,
            s.show_hidden_geometry,
            style.face_mode,
            style.front_color,
            effective_back_color(style, s.back_face_color),
            style.edge_color,
            s.doc
                .layers
                .iter()
                .map(|l| (l.name.clone(), l.visible))
                .collect::<Vec<_>>(),
            self.scene_preview_groups(),
            self.preview.suppressed_serial,
        )
    }

    /// The meshes the frame draws: the loose one, then every placement.
    fn pieces(&self) -> Vec<Piece<'_>> {
        let s = &self.scene;
        let in_edit = s.edit_group().is_some();
        let rest = self.prefs.edit_rest_mode;
        let mut out = vec![Piece {
            mesh_id: MeshId::LOOSE,
            mesh: s.loose_mesh(),
            xform: None,
            material: None,
            context: in_edit && rest == RestMode::Fade,
            ghost: false,
        }];
        if in_edit && rest == RestMode::Hide {
            out.pop();
        }
        let moving = self.scene_preview_groups();
        for p in self.placements() {
            if moving.contains(&p.owner) || moving.contains(&p.group) {
                continue; // drawn apart, through the preview matrix
            }
            let (vis, _locked) = layer_state(s, p.layer.as_deref());
            if !vis {
                continue;
            }
            let ghost = p.hidden;
            if ghost && !s.show_hidden_objects {
                continue;
            }
            let ctx = self.draws_in_edit_context(&p);
            if ctx && rest == RestMode::Hide {
                continue;
            }
            let Some(mesh) = s.doc.meshes.get(&p.mesh) else {
                continue;
            };
            out.push(Piece {
                mesh_id: p.mesh,
                mesh,
                xform: p.xform,
                material: p.material.clone(),
                context: ctx && rest == RestMode::Fade,
                ghost,
            });
        }
        // While a group is open its own mesh is the active one (identity).
        if let Some(g) = s.edit_group() {
            let mid = s.active_mesh_id();
            if mid != MeshId::LOOSE && !out.iter().any(|p| p.mesh_id == mid && p.xform.is_none()) {
                if let Some(mesh) = s.doc.meshes.get(&mid) {
                    let material = s.doc.find_group(g).and_then(|g| g.material.clone());
                    out.push(Piece {
                        mesh_id: mid,
                        mesh,
                        xform: None,
                        material,
                        context: false,
                        ghost: false,
                    });
                }
            }
        }
        out
    }

    /// The meshes a live groups preview draws: the moving scene groups
    /// (with their nested placements), or the clipboard's templates.
    fn preview_pieces(&self) -> Vec<Piece<'_>> {
        let mut out = Vec::new();
        if self.preview.groups.is_empty() {
            return out;
        }
        if self.preview.external {
            let Some(clip) = &self.clipboard else {
                return out;
            };
            fn walk<'a>(
                out: &mut Vec<Piece<'a>>,
                meshes: &'a xtrude_model::group::MeshTable,
                g: &'a xtrude_model::group::Group,
                world: Option<DMat4>,
            ) {
                let m = match (world, g.xform) {
                    (None, x) => x,
                    (Some(w), Some(x)) => Some(w * x),
                    (Some(w), None) => Some(w),
                };
                if let Some(mesh) = meshes.get(&g.mesh) {
                    out.push(Piece {
                        mesh_id: g.mesh,
                        mesh,
                        xform: m,
                        material: g.material.clone(),
                        context: false,
                        ghost: false,
                    });
                }
                for c in &g.children {
                    walk(out, meshes, c, m);
                }
            }
            for g in clip
                .groups
                .iter()
                .filter(|g| self.preview.groups.contains(&g.id))
            {
                walk(&mut out, &clip.meshes, g, None);
            }
            return out;
        }
        let s = &self.scene;
        for p in self.placements() {
            if !(self.preview.groups.contains(&p.owner) || self.preview.groups.contains(&p.group)) {
                continue;
            }
            let (vis, _) = layer_state(s, p.layer.as_deref());
            if !vis || p.hidden {
                continue;
            }
            if let Some(mesh) = s.doc.meshes.get(&p.mesh) {
                out.push(Piece {
                    mesh_id: p.mesh,
                    mesh,
                    xform: p.xform,
                    material: p.material.clone(),
                    context: false,
                    ghost: false,
                });
            }
        }
        out
    }

    /// Build the preview buffers once per preview (and per edit).
    fn sync_preview_cache(&mut self, style: &Style) {
        if self.preview.groups.is_empty() {
            return;
        }
        let key = format!(
            "{}|{:?}|{}|{}",
            self.render_key(style),
            self.preview.groups,
            self.preview.external,
            self.render_cache
                .origin
                .to_array()
                .map(f64::to_bits)
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        if self.render_cache.preview_key.as_deref() == Some(key.as_str()) {
            return;
        }
        let origin = self.render_cache.origin;
        let local = |p: DVec3| {
            let d = p - origin;
            [d.x as f32, d.y as f32, d.z as f32]
        };
        let back_default = effective_back_color(style, self.scene.back_face_color);
        let mut faces = Vec::new();
        let mut lines = Vec::new();
        let white = [1.0f32; 4];
        for piece in self.preview_pieces() {
            let m = piece.mesh;
            let map = |p: DVec3| piece.xform.map_or(p, |x| x.project_point(p));
            let mirrored = piece.xform.is_some_and(|x| x.determinant() < 0.0);
            for f in m.face_ids() {
                let face = m.face(f);
                if face.attrs.hidden {
                    continue;
                }
                let (outer, holes) = mapped_loops(m, f, &map, mirrored);
                if outer.len() < 3 {
                    continue;
                }
                let n = newell(&outer).unit_or_zero();
                let attrs = effective_attrs(&face.attrs, piece.material.as_ref());
                let (front, back, op) = Self::face_colors(style, back_default, &attrs, n);
                let (fc, bc) = (rgba(front, op), rgba(back, op));
                for [a, b, c] in triangulate(&outer, &holes, Some(n)) {
                    for p in [a, b, c] {
                        faces.push(FaceVertex {
                            pos: local(p),
                            front: fc,
                            back: bc,
                        });
                    }
                }
            }
            for e in m.edge_ids() {
                let edge = m.edge(e);
                if edge.hidden || edge.soft {
                    continue;
                }
                let (a, b) = m.edge_ab(e);
                lines.push(LineVertex {
                    pos: local(map(a)),
                    color: white,
                });
                lines.push(LineVertex {
                    pos: local(map(b)),
                    color: white,
                });
            }
        }
        let c = &mut self.render_cache;
        c.preview_faces = Arc::new(faces);
        c.preview_lines = Arc::new(lines);
        c.preview_generation += 1;
        c.preview_key = Some(key);
    }

    /// The face's two side colours in the current face mode.
    fn face_colors(
        style: &Style,
        back_default: [f64; 3],
        attrs: &FaceAttrs,
        n: DVec3,
    ) -> ([f64; 3], [f64; 3], f64) {
        let op = attrs.paint.opacity.unwrap_or(1.0);
        match style.face_mode.as_str() {
            "hidden_line" => (style.front_color, style.front_color, 1.0),
            "monochrome" => (style.front_color, back_default, 1.0),
            _ => {
                let front = shaded_color(attrs.paint.color.unwrap_or(DEFAULT_FACE_COLOR), n);
                let back = match &attrs.back {
                    Some(Back::Mirror) => front,
                    Some(Back::Own(p)) => match p.color {
                        Some(c) => shaded_color(c, n),
                        None => back_default,
                    },
                    // Translucent paint reads the same from both sides.
                    None if op < 0.999 => front,
                    None => back_default,
                };
                (front, back, op)
            }
        }
    }

    fn sync_render_cache(&mut self, style: &Style) {
        let key = self.render_key(style);
        if self.render_cache.key.as_deref() == Some(key.as_str()) {
            return;
        }
        let s = &self.scene;
        let back_default = effective_back_color(style, s.back_face_color);
        let origin = s
            .compute_bounds()
            .map_or(DVec3::ZERO, |(lo, hi)| ((lo + hi) * 0.5).round());
        let local = |p: DVec3| {
            let d = p - origin;
            [d.x as f32, d.y as f32, d.z as f32]
        };
        let pieces = self.pieces();
        let show_geo = s.show_hidden_geometry;
        // faces: context, subject, translucent, ghosts
        let mut buckets: [Vec<FaceVertex>; 4] = Default::default();
        let mut lbuckets: [Vec<LineVertex>; 3] = Default::default();
        let mut soft = SoftEdges::default();
        let ec = rgba(style.edge_color, 1.0);
        for piece in &pieces {
            let m = piece.mesh;
            let map = |p: DVec3| piece.xform.map_or(p, |x| x.project_point(p));
            let mirrored = piece.xform.is_some_and(|x| x.determinant() < 0.0);
            let mut soft_planes = HashMap::new();
            for f in m.face_ids() {
                if self.preview.suppressed_faces.contains(&(piece.mesh_id, f)) {
                    continue;
                }
                let face = m.face(f);
                let hidden = face.attrs.hidden;
                if hidden && !show_geo {
                    continue;
                }
                if piece.mesh_id == MeshId::LOOSE
                    && !s.entity_visible(&EntityRef::Face(MeshId::LOOSE, f))
                    && !hidden
                {
                    continue;
                }
                let (outer, holes) = mapped_loops(m, f, &map, mirrored);
                if outer.len() < 3 {
                    continue;
                }
                let n = newell(&outer).unit_or_zero();
                let attrs = effective_attrs(&face.attrs, piece.material.as_ref());
                let (front, back, op) = Self::face_colors(style, back_default, &attrs, n);
                let bucket = if piece.ghost || hidden {
                    3
                } else if op < 0.999 {
                    2
                } else if piece.context {
                    0
                } else {
                    1
                };
                let (fc, bc) = (rgba(front, op), rgba(back, op));
                for [a, b, c] in triangulate(&outer, &holes, Some(n)) {
                    for p in [a, b, c] {
                        buckets[bucket].push(FaceVertex {
                            pos: local(p),
                            front: fc,
                            back: bc,
                        });
                    }
                }
            }
            for e in m.edge_ids() {
                if self.preview.suppressed_edges.contains(&(piece.mesh_id, e)) {
                    continue;
                }
                let edge = m.edge(e);
                let (a, b) = m.edge_ab(e);
                let (a, b) = (map(a), map(b));
                let ghost = piece.ghost || edge.hidden;
                if edge.hidden && !show_geo {
                    continue;
                }
                if piece.mesh_id == MeshId::LOOSE
                    && !s.entity_visible(&EntityRef::Edge(MeshId::LOOSE, e))
                    && !edge.hidden
                {
                    continue;
                }
                if edge.soft && !ghost {
                    if show_geo {
                        lbuckets[2].push(LineVertex {
                            pos: local(a),
                            color: ec,
                        });
                        lbuckets[2].push(LineVertex {
                            pos: local(b),
                            color: ec,
                        });
                    }
                    if !(piece.context && self.prefs.edit_rest_mode == RestMode::Fade) {
                        soft.push(m, e, (a, b), &map, &mut soft_planes);
                    }
                    continue;
                }
                let bucket = if ghost {
                    2
                } else if piece.context {
                    0
                } else {
                    1
                };
                lbuckets[bucket].push(LineVertex {
                    pos: local(a),
                    color: ec,
                });
                lbuckets[bucket].push(LineVertex {
                    pos: local(b),
                    color: ec,
                });
            }
        }
        let c = &mut self.render_cache;
        c.face_ctx = buckets[0].len() as u32;
        c.face_opaque = c.face_ctx + buckets[1].len() as u32;
        c.face_ghosts = c.face_opaque + buckets[2].len() as u32;
        let faces: Vec<FaceVertex> = buckets.into_iter().flatten().collect();
        c.line_ctx = lbuckets[0].len() as u32;
        c.line_ghosts = c.line_ctx + lbuckets[1].len() as u32;
        let lines: Vec<LineVertex> = lbuckets.into_iter().flatten().collect();
        c.faces = Arc::new(faces);
        c.lines = Arc::new(lines);
        c.soft = soft;
        c.origin = origin;
        c.generation += 1;
        c.key = Some(key);
        c.selected_faces_world.clear();
    }

    /// The frame to draw at `size` px. `export`: an image render (no
    /// selection cues).
    pub fn frame_data(&mut self, size: [u32; 2], export: bool) -> FrameData {
        if !export {
            self.note_painted_view(size);
        }
        self.build_frame(size, export)
    }

    /// The frame to draw on screen at `size` px, or `None` when it is the
    /// frame the screen already shows: a window where nothing changed
    /// (the camera, the scene, its style, the selection, the hover, a
    /// tool's preview, the size…) keeps its last image and does not render
    /// the scene again. The depth read from the frame on screen stays
    /// valid while it is kept.
    pub fn changed_frame(&mut self, size: [u32; 2]) -> Option<FrameData> {
        let f = self.build_frame(size, false);
        if self.render_cache.on_screen.as_ref() == Some(&f) {
            return None;
        }
        self.note_painted_view(size);
        self.render_cache.on_screen = Some(f.clone());
        Some(f)
    }

    /// The image on screen is gone (a new renderer): the next
    /// [`Self::changed_frame`] draws.
    pub fn forget_frame_on_screen(&mut self) {
        self.render_cache.on_screen = None;
    }

    fn build_frame(&mut self, size: [u32; 2], export: bool) -> FrameData {
        self.sync_axes();
        let style = self.effective_style().clone();
        self.sync_render_cache(&style);
        self.sync_preview_cache(&style);
        let mode = style.face_mode.clone();
        if style.profiles && mode != "wireframe" {
            let eye = self.camera.eye();
            self.render_cache.soft.sync_profiles(eye);
        }
        let cache = &self.render_cache;
        let view_proj = self.camera.projection_matrix() * self.camera.view_matrix();
        let mut f = FrameData {
            size,
            background: style.background.map(|c| c as f32),
            origin: cache.origin,
            view_proj,
            fade_color: style.background.map(|c| c as f32),
            scene_faces: Cached {
                generation: cache.generation,
                data: cache.faces.clone(),
            },
            scene_lines: Cached {
                generation: cache.generation,
                data: cache.lines.clone(),
            },
            preview_faces: Cached {
                generation: cache.preview_generation,
                data: cache.preview_faces.clone(),
            },
            preview_lines: Cached {
                generation: cache.preview_generation,
                data: cache.preview_lines.clone(),
            },
            ..Default::default()
        };
        let section = if self.scene.show_section_cuts {
            self.scene.active_section().map(|sp| {
                let n = sp.normal;
                [-n.x, -n.y, -n.z, n.dot(sp.point)]
            })
        } else {
            None
        };
        f.clip_plane = section;
        let clip = section.is_some();
        let fading =
            self.scene.edit_group().is_some() && self.prefs.edit_rest_mode == RestMode::Fade;
        let (fctx, fop, fgh, nf) = (
            cache.face_ctx,
            cache.face_opaque,
            cache.face_ghosts,
            cache.faces.len() as u32,
        );
        let (lctx, lgh, nl) = (cache.line_ctx, cache.line_ghosts, cache.lines.len() as u32);

        // ---- sky -------------------------------------------------------------
        if style.sky {
            self.push_sky(&mut f, &style, view_proj);
        }

        // ---- faces -----------------------------------------------------------
        if mode != "wireframe" {
            let xray = mode == "xray";
            let depth = if xray {
                Depth::TestOnly
            } else {
                Depth::TestWrite
            };
            let opacity = if xray { XRAY_FACE_OPACITY } else { 1.0 };
            let base = |first, count| {
                Draw::new(Source::SceneFaces, first, count)
                    .bias()
                    .depth(depth)
                    .opacity(opacity)
                    .clipped(clip)
            };
            f.draws
                .push(base(0, fctx).fade(if fading { EDIT_REST_FADE } else { 0.0 }));
            f.draws.push(base(fctx, fop - fctx));
            // translucent runs: blended, depth-tested, not written
            f.draws.push(
                Draw::new(Source::SceneFaces, fop, fgh - fop)
                    .bias()
                    .depth(Depth::TestOnly)
                    .opacity(opacity)
                    .clipped(clip),
            );
            // hidden objects / geometry shown: the see-through grid
            f.draws.push(
                Draw::new(Source::SceneFaces, fgh, nf - fgh)
                    .bias()
                    .depth(Depth::TestOnly)
                    .stipple(Stipple::Grid)
                    .clipped(clip),
            );
            if !export {
                self.push_selection_dots(&mut f, clip);
            }
        }

        // ---- the groups moving in a live preview: their frozen buffers at
        // the preview transform (selection orange edges: moving) -----------
        if !self.preview.groups.is_empty() {
            let m = self.preview.matrix.unwrap_or(DMat4::IDENTITY);
            let (npf, npl) = (
                self.render_cache.preview_faces.len() as u32,
                self.render_cache.preview_lines.len() as u32,
            );
            if mode != "wireframe" {
                f.draws.push(
                    Draw::new(Source::PreviewFaces, 0, npf)
                        .bias()
                        .model(m)
                        .clipped(clip),
                );
            }
            f.draws.push(
                Draw::new(Source::PreviewLines, 0, npl)
                    .color([1.0, 0.45, 0.0, 1.0])
                    .model(m)
                    .clipped(clip),
            );
        }

        // ---- the tool's solid preview ----------------------------------------
        self.push_preview_faces(&mut f);

        // ---- axes --------------------------------------------------------------
        let dist = self.camera.distance.max(1e-3);
        if self.preview.style_override.is_none() {
            let spacing = (self.camera.distance * 0.008).max(1e-4);
            for (axis, color) in AXES {
                let first = f.lines.len() as u32;
                push_axis(&mut f, axis, spacing, color);
                let count = f.lines.len() as u32 - first;
                f.draws
                    .push(Draw::new(Source::Lines, first, count).depth(Depth::TestOnly));
            }
            // guides, depth-tested, never culling
            let sel: Vec<_> = self
                .scene
                .selection
                .iter()
                .filter_map(|e| match e {
                    EntityRef::Guide(g) => Some(*g),
                    _ => None,
                })
                .collect();
            let (pts, spans) = guide_vertices(
                &self.scene.doc.guides,
                dist * 0.03,
                &sel,
                Some(self.camera.target),
                dist * 15.0,
            );
            for (start, count, selected) in spans {
                let color = if selected { SELECTION } else { GUIDE };
                let first = f.lines.len() as u32;
                for p in &pts[start..start + count] {
                    let pos = f.local(*p);
                    f.lines.push(LineVertex { pos, color });
                }
                f.draws
                    .push(Draw::new(Source::Lines, first, count as u32).depth(Depth::TestOnly));
            }
            self.push_edit_box(&mut f, dist);
        }

        // ---- edges ---------------------------------------------------------------
        let show_edges = style.edges || mode == "wireframe";
        if show_edges {
            if mode == "xray" {
                // the faces' depth, then the edges behind it washed out
                f.draws.push(
                    Draw::new(Source::SceneFaces, 0, fgh)
                        .depth(Depth::TestWrite)
                        .bias()
                        .depth_only()
                        .clipped(clip),
                );
                let bg = style.background;
                let ec = style.edge_color;
                let washed: [f64; 3] =
                    std::array::from_fn(|i| ec[i] + (bg[i] - ec[i]) * XRAY_HIDDEN_EDGE_FADE);
                f.draws.push(
                    Draw::new(Source::SceneLines, 0, lgh)
                        .depth(Depth::Behind)
                        .color(rgba(washed, 1.0))
                        .clipped(clip),
                );
            }
            f.draws.push(
                Draw::new(Source::SceneLines, 0, lctx)
                    .fade(if fading { EDIT_REST_FADE } else { 0.0 })
                    .clipped(clip),
            );
            f.draws
                .push(Draw::new(Source::SceneLines, lctx, lgh - lctx).clipped(clip));
            f.draws.push(
                Draw::new(Source::SceneLines, lgh, nl - lgh)
                    .stipple(Stipple::Dots)
                    .clipped(clip),
            );
            if mode != "xray" && mode != "wireframe" && style.back_edges {
                let mut d = Draw::new(Source::SceneLines, 0, lgh)
                    .depth(Depth::Behind)
                    .stipple(Stipple::Dash)
                    .clipped(clip);
                d.dash_px = 4.0;
                f.draws.push(d);
            }
            if style.profiles {
                let ec = rgba(style.edge_color, 1.0);
                let first = f.lines.len() as u32;
                for &k in &cache.soft.profiles {
                    let (a, b) = cache.soft.ends[k as usize];
                    f.push_line(a, b, ec);
                }
                let count = f.lines.len() as u32 - first;
                f.draws
                    .push(Draw::new(Source::Lines, first, count).clipped(clip));
            }
        }
        if !export {
            self.push_selected_edges(&mut f);
            self.push_hover_edge(&mut f);
        }

        // ---- rubber band (depth-tested ones; the rest are overlay) ---------
        if let Some(t) = self.active_tool() {
            if t.info().wireframe_depth_tested {
                let segs = t.rubber_band_lines(self);
                let color = t
                    .wireframe_color()
                    .map(|c| c.map(|x| x as f32))
                    .unwrap_or_else(|| self.rubber_band_color());
                let first = f.lines.len() as u32;
                for (a, b) in &segs {
                    f.push_line(*a, *b, color);
                }
                let count = f.lines.len() as u32 - first;
                f.draws.push(Draw::new(Source::Lines, first, count));
            }
        }
        f
    }

    /// The rubber band's colour: the snap's axis / reference cue, else
    /// the selection orange.
    fn rubber_band_color(&self) -> [f32; 4] {
        use xtrude_core::snap::SnapKind;
        let Some(s) = &self.inference.last_snap else {
            return [0.95, 0.45, 0.16, 0.85];
        };
        let [r, g, b] = s.color;
        match s.kind {
            SnapKind::Axis | SnapKind::Reference | SnapKind::ThroughPoint | SnapKind::PerpFace => {
                rgba([r, g, b], 1.0)
            }
            SnapKind::AxisInference => rgba([r, g, b], 0.5),
            SnapKind::Close => [0.20, 0.40, 0.78, 0.95],
            _ => [0.95, 0.45, 0.16, 0.85],
        }
    }

    fn push_sky(&self, f: &mut FrameData, style: &Style, mvp: DMat4) {
        let hy_raw = self.horizon_ndc_y(mvp);
        let hy = hy_raw.clamp(-1.0, 1.0);
        let SkyGradient {
            sky_bottom: c_sky0,
            sky_top: c_sky1,
            ground_top: c_gnd0,
            ground_bottom: c_gnd1,
            horizon_line: line,
        } = sky_gradient(style.sky_color, style.ground_color, hy_raw);
        let first = f.lines.len() as u32;
        let quad = |y0: f64, y1: f64, c0: [f64; 3], c1: [f64; 3], faces: &mut Vec<FaceVertex>| {
            let v = |x: f64, y: f64, c: [f64; 3]| FaceVertex {
                pos: [x as f32, y as f32, 0.0],
                front: rgba(c, 1.0),
                back: rgba(c, 1.0),
            };
            faces.extend([
                v(-1.0, y0, c0),
                v(1.0, y0, c0),
                v(1.0, y1, c1),
                v(-1.0, y0, c0),
                v(1.0, y1, c1),
                v(-1.0, y1, c1),
            ]);
        };
        let ff = f.faces.len() as u32;
        if hy < 1.0 {
            quad(hy, 1.0, c_sky0, c_sky1, &mut f.faces);
        }
        if hy > -1.0 {
            quad(-1.0, hy, c_gnd1, c_gnd0, &mut f.faces);
        }
        let fc = f.faces.len() as u32 - ff;
        f.draws.push(Draw::new(Source::Faces, ff, fc).ndc());
        if -1.0 < hy && hy < 1.0 {
            let c = rgba(line, 1.0);
            f.lines.push(LineVertex {
                pos: [-1.0, hy as f32, 0.0],
                color: c,
            });
            f.lines.push(LineVertex {
                pos: [1.0, hy as f32, 0.0],
                color: c,
            });
            f.draws.push(Draw::new(Source::Lines, first, 2).ndc());
        }
    }

    /// Where the ground plane at infinity crosses the
    /// screen (beyond ±1 when it is off screen).
    pub fn horizon_ndc_y(&self, mvp: DMat4) -> f64 {
        let eye = self.camera.eye();
        let fwd = self.camera.target - eye;
        let dh = DVec3::new(fwd.x, fwd.y, 0.0);
        if dh.length() < 1e-5 {
            return if fwd.z < 0.0 { 2.0 } else { -2.0 };
        }
        let far = eye + dh.normalize() * 1.0e6;
        let clip = mvp * far.extend(1.0);
        if clip.w.abs() < 1e-9 {
            return if fwd.z < 0.0 { 2.0 } else { -2.0 };
        }
        clip.y / clip.w
    }

    /// The selected faces (and the hovered one) as dots, each side's dot
    /// coloured against that side's paint.
    fn push_selection_dots(&self, f: &mut FrameData, clip: bool) {
        let mut faces: Vec<(MeshId, FaceId)> = self
            .scene
            .selection
            .iter()
            .filter_map(|e| match e {
                EntityRef::Face(m, x) => Some((*m, *x)),
                _ => None,
            })
            .collect();
        if let Some(EntityRef::Face(m, x)) = self.hover_entity {
            if !faces.contains(&(m, x)) {
                faces.push((m, x));
            }
        }
        let first = f.faces.len() as u32;
        for (mid, fid) in faces {
            let Some(mesh) = self.scene.doc.meshes.get(&mid) else {
                continue;
            };
            if !mesh.has_face(fid) {
                continue;
            }
            let (front, back) = self.selection_dot_colors(mid, fid);
            let outer = mesh.face_positions(fid);
            let holes = mesh.face_hole_positions(fid);
            let n = newell(&outer).unit_or_zero();
            for [a, b, c] in triangulate(&outer, &holes, Some(n)) {
                for p in [a, b, c] {
                    f.faces.push(FaceVertex {
                        pos: f.local(p),
                        front: rgba(front, 1.0),
                        back: rgba(back, 1.0),
                    });
                }
            }
        }
        let count = f.faces.len() as u32 - first;
        f.draws.push(
            Draw::new(Source::Faces, first, count)
                .bias()
                .depth(Depth::TestOnly)
                .stipple(Stipple::Selection)
                .clipped(clip),
        );
    }

    fn push_preview_faces(&self, f: &mut FrameData) {
        let Some(t) = self.active_tool() else { return };
        let faces = t.preview_faces();
        if faces.is_empty() {
            return;
        }
        let first = f.faces.len() as u32;
        for pf in faces {
            if pf.vertices.len() < 3 {
                continue;
            }
            let n = newell(&pf.vertices).unit_or_zero();
            let base = pf
                .attrs
                .as_ref()
                .and_then(|a| a.paint.color)
                .unwrap_or(DEFAULT_FACE_COLOR);
            // A preview has no back: both sides wear the face's colour.
            let c = rgba(shaded_color(base, n), 1.0);
            for [a, b, cc] in triangulate(&pf.vertices, &pf.holes, Some(n)) {
                for p in [a, b, cc] {
                    f.faces.push(FaceVertex {
                        pos: f.local(p),
                        front: c,
                        back: c,
                    });
                }
            }
        }
        let count = f.faces.len() as u32 - first;
        f.draws.push(Draw::new(Source::Faces, first, count).bias());
    }

    fn push_edit_box(&self, f: &mut FrameData, dist: f64) {
        let Some(g) = self.scene.edit_group() else {
            return;
        };
        let Some((frame, lo, hi)) = self.group_obb(g) else {
            return;
        };
        let corners = xtrude_model::group::oriented_box_corners(frame, lo, hi);
        let first = f.lines.len() as u32;
        for ab in xtrude_render::lines::box_dash_vertices(&corners, dist * 0.012).chunks_exact(2) {
            f.push_line(ab[0], ab[1], EDIT_BOX);
        }
        let count = f.lines.len() as u32 - first;
        f.draws
            .push(Draw::new(Source::Lines, first, count).depth(Depth::TestOnly));
    }

    fn edge_world(&self, mid: MeshId, e: EdgeId) -> Option<(DVec3, DVec3)> {
        let m = self.scene.doc.meshes.get(&mid)?;
        m.has_edge(e).then(|| m.edge_ab(e))
    }

    fn push_selected_edges(&self, f: &mut FrameData) {
        let first = f.lines.len() as u32;
        let sel: Vec<(MeshId, EdgeId)> = self
            .scene
            .selection
            .iter()
            .filter_map(|e| match e {
                EntityRef::Edge(m, x) => Some((*m, *x)),
                _ => None,
            })
            .collect();
        for (m, e) in sel {
            if let Some((a, b)) = self.edge_world(m, e) {
                f.push_line(a, b, SELECTION);
            }
        }
        // a selected group: its box
        for e in self.scene.selection.iter() {
            if let EntityRef::Group(g) = e {
                if let Some((frame, lo, hi)) = self.group_obb(*g) {
                    let c = xtrude_model::group::oriented_box_corners(frame, lo, hi);
                    for i in 0..8usize {
                        for bit in [1usize, 2, 4] {
                            let j = i | bit;
                            if j != i {
                                f.push_line(c[i], c[j], SELECTION);
                            }
                        }
                    }
                }
            }
        }
        let count = f.lines.len() as u32 - first;
        f.draws.push(Draw::new(Source::Lines, first, count));
    }

    fn push_hover_edge(&self, f: &mut FrameData) {
        let Some(EntityRef::Edge(m, e)) = self.hover_entity else {
            return;
        };
        let Some(mesh) = self.scene.doc.meshes.get(&m) else {
            return;
        };
        if !mesh.has_edge(e) {
            return;
        }
        let first = f.lines.len() as u32;
        // a curve segment highlights its whole contour
        let edges: Vec<EdgeId> = match mesh.edge(e).curve {
            Some(c) => mesh
                .edge_ids()
                .into_iter()
                .filter(|&x| mesh.edge(x).curve == Some(c))
                .collect(),
            None => vec![e],
        };
        for x in edges {
            let (a, b) = mesh.edge_ab(x);
            f.push_line(a, b, HOVER_EDGE);
        }
        let count = f.lines.len() as u32 - first;
        f.draws.push(Draw::new(Source::Lines, first, count));
    }
}

/// A face's loops in world space. A mirroring placement (negative
/// determinant) reverses the winding of every mapped polygon, which would
/// put the painted front on the far side; reversing the loops keeps the
/// front where the prototype's front is.
fn mapped_loops(
    m: &Mesh,
    f: FaceId,
    map: &dyn Fn(DVec3) -> DVec3,
    mirrored: bool,
) -> (Vec<DVec3>, Vec<Vec<DVec3>>) {
    let mut outer: Vec<DVec3> = m.face_positions(f).into_iter().map(map).collect();
    let mut holes: Vec<Vec<DVec3>> = m
        .face_hole_positions(f)
        .into_iter()
        .map(|h| h.into_iter().map(map).collect())
        .collect();
    if mirrored {
        outer.reverse();
        for h in &mut holes {
            h.reverse();
        }
    }
    (outer, holes)
}

/// A long solid line toward + and evenly spaced short
/// dashes toward −, for one drawing axis.
pub fn push_axis(f: &mut FrameData, axis: Axis, spacing: f64, color: [f32; 4]) {
    let o = xtrude_core::axes::origin();
    let d = xtrude_core::axes::axis(axis);
    f.push_line(o, o + d * 1.0e5, color);
    let dash = spacing * 0.18;
    for k in 0..520 {
        let t0 = k as f64 * spacing;
        f.push_line(o - d * t0, o - d * (t0 + dash), color);
    }
}
