// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Picking: the flat pick index of the scene (every loose and placed face
//! as world triangles, every loose edge, every group hard edge) and the
//! queries over it — face / group / edge / vertex picks, occlusion, the
//! point a pan grabs and the zoom focus.
//!
//! The index is plain vectors with per-placement AABB spans (huge models),
//! and each mesh's local triangles are cached by its
//! mutation serial. The screen projections of the indexed edges are
//! cached per view (projection and index build), so the several picks of
//! one hover, and the hovers that follow while the camera stays still,
//! project them once.

use std::collections::HashMap;
use std::rc::Rc;
use xtrude_core::Axis;

use crate::input::ScreenPt;
use glam::{DMat4, DVec3, DVec4};
use xtrude_core::attrs::Paint;
use xtrude_core::math::{MatExt, VecExt};
use xtrude_core::snap::{self, SnapContext, SnapEdge};
use xtrude_core::{EdgeId, FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{effective_material, Group};
use xtrude_model::ids::{GroupId, MeshId, ObjId};
use xtrude_model::layers::layer_name;
use xtrude_model::scene::Scene;
use xtrude_model::section::SectionPlane;

use super::{DepthSeen, InMotion, PickedEdge, Viewport};

/// An edge's two ends on screen.
type ScreenSegment = ((f64, f64), (f64, f64));

/// Per indexed edge, its screen segment (`None` where an end is behind
/// the camera).
type EdgeScreens = Rc<[Option<ScreenSegment>]>;

/// World → widget pixel as one matrix and the widget size: projects many
/// points with the matrix built once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    /// The camera's model-view-projection.
    pub mvp: DMat4,
    pub width: f64,
    pub height: f64,
}

impl Projection {
    /// World point → widget pixel, `None` behind the camera.
    pub fn to_pixel(&self, world: DVec3) -> Option<ScreenPt> {
        let clip = self.mvp * DVec4::new(world.x, world.y, world.z, 1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
        Some(ScreenPt::new(
            (nx * 0.5 + 0.5) * self.width,
            (1.0 - (ny * 0.5 + 0.5)) * self.height,
        ))
    }
}

/// One placement of a group's mesh in the world: a top-level group, or a
/// nested child of a component expanded with its composed matrix.
#[derive(Clone, Debug)]
pub struct Placement {
    /// The group (or nested child) this placement draws.
    pub group: GroupId,
    pub mesh: MeshId,
    /// The composed world matrix (`None` = identity, a plain group).
    pub xform: Option<DMat4>,
    /// What a click on it selects: the top-level group, or inside an open
    /// group its first-level child.
    pub owner: GroupId,
    /// The open group this placement lives inside, if any.
    pub context: Option<GroupId>,
    pub layer: Option<String>,
    pub hidden: bool,
    pub material: Option<Paint>,
    /// A nested placement (proxy), not a top-level group.
    pub nested: bool,
    /// The owner is a component instance (for the ScreenTip).
    pub owner_is_instance: bool,
}

/// A face under the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceHit {
    pub mesh: MeshId,
    pub face: FaceId,
    /// The placement the face was hit through (`None` = loose / the open
    /// group's own geometry).
    pub placement: Option<GroupId>,
    /// The world matrix of that placement.
    pub xform: Option<DMat4>,
    /// What a click selects (the top-level owner), `None` for loose.
    pub owner: Option<GroupId>,
}

impl FaceHit {
    pub fn entity(&self) -> EntityRef {
        EntityRef::Face(self.mesh, self.face)
    }
}

#[derive(Clone, Debug)]
struct FaceEnt {
    mesh: MeshId,
    face: FaceId,
    /// Index into `placements`.
    placement: Option<u32>,
    owner: Option<GroupId>,
}

#[derive(Clone, Debug)]
struct Span {
    bbox: Option<(DVec3, DVec3)>,
    start: usize,
    count: usize,
}

/// A mesh's local pick payload, cached by the mesh's mutation serial.
#[derive(Clone, Debug, Default)]
struct LocalChunk {
    serial: u64,
    /// Visible (not Hide-n) faces, in mesh order.
    faces: Vec<FaceId>,
    /// Outer-loop corners of each face (for world areas).
    outers: Vec<Vec<DVec3>>,
    areas: Vec<f64>,
    tris: Vec<[DVec3; 3]>,
    tri_face: Vec<u32>,
    /// Hard edges (not soft, not hidden).
    edges: Vec<(DVec3, DVec3)>,
}

fn build_local_chunk(mesh: &Mesh) -> LocalChunk {
    let mut c = LocalChunk {
        serial: mesh.mut_serial,
        ..Default::default()
    };
    for f in mesh.face_ids() {
        if mesh.face(f).attrs.hidden {
            continue;
        }
        let i = c.faces.len() as u32;
        c.faces.push(f);
        c.outers.push(mesh.face_positions(f));
        c.areas.push(mesh.face_area(f));
        for t in mesh.face_triangulate(f, None) {
            c.tris.push(t);
            c.tri_face.push(i);
        }
    }
    for e in mesh.edge_ids() {
        let ed = mesh.edge(e);
        if ed.hidden || ed.soft {
            continue;
        }
        let (a, b) = mesh.edge_ab(e);
        c.edges.push((a, b));
    }
    c
}

fn loop_area(pts: &[DVec3]) -> f64 {
    let n = pts.len();
    let mut s = DVec3::ZERO;
    for i in 0..n {
        s += pts[i].cross(pts[(i + 1) % n]);
    }
    0.5 * s.length()
}

/// The scene's pick index.
#[derive(Clone, Debug, Default)]
pub struct PickIndex {
    placements: Vec<Placement>,
    ents: Vec<FaceEnt>,
    area: Vec<f64>,
    sel: Vec<bool>,
    vis: Vec<bool>,
    loose: Vec<bool>,
    v0: Vec<DVec3>,
    e1: Vec<DVec3>,
    e2: Vec<DVec3>,
    tri_ent: Vec<u32>,
    spans: Vec<Span>,
    /// Loose edges of the active mesh.
    edges: Vec<EdgeId>,
    edge_a: Vec<DVec3>,
    edge_b: Vec<DVec3>,
    edge_sel: Vec<bool>,
    edge_soft: Vec<bool>,
    /// Group hard edges (world).
    gedge_a: Vec<DVec3>,
    gedge_b: Vec<DVec3>,
    gedge_gi: Vec<u32>,
    gedge_sel: Vec<bool>,
    /// Per `gedge_gi`: (owner, owner is an instance).
    gedge_groups: Vec<(GroupId, bool)>,
}

impl PickIndex {
    pub fn face_count(&self) -> usize {
        self.ents.len()
    }

    pub fn group_edge_count(&self) -> usize {
        self.gedge_a.len()
    }

    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// The owners of the indexed group edges: `(owner, is an instance)`.
    pub fn group_edge_owners(&self) -> &[(GroupId, bool)] {
        &self.gedge_groups
    }

    /// Per group edge: selectable in the current context (the rest of the
    /// model is snap-only).
    pub fn group_edge_selectable(&self) -> &[bool] {
        &self.gedge_sel
    }
}

/// The index and the caches it is built from.
#[derive(Default)]
pub(crate) struct PickCache {
    key: Option<IndexKey>,
    index: PickIndex,
    chunks: HashMap<MeshId, LocalChunk>,
    hover_hits: Option<(HitKey, Vec<f64>)>,
    builds: u64,
    /// The group edges' screen projections, for one view.
    group_screens: Option<(ViewKey, EdgeScreens)>,
    /// The loose edges' screen projections, for one view.
    loose_screens: Option<(ViewKey, EdgeScreens)>,
    /// How many times edges were projected (tests: caching).
    projection_passes: u64,
}

/// The view a cached edge projection was made for: the projection and the
/// index build it projected.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewKey {
    projection: Projection,
    build: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct IndexKey {
    version: u64,
    loose_serial: u64,
    edit: Vec<GroupId>,
    rest_hidden: bool,
    show_hidden_objects: bool,
    show_hidden_geometry: bool,
    layers: Vec<(String, bool, bool)>,
}

type HitKey = (u64, [u64; 6]);

/// Which entities a ray may hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mask {
    Visible,
    Selectable,
}

/// The section plane actually cutting (cuts shown + one active).
pub fn active_cut(scene: &Scene) -> Option<&SectionPlane> {
    if !scene.show_section_cuts {
        return None;
    }
    scene.active_section()
}

pub(super) fn layer_state(scene: &Scene, label: Option<&str>) -> (bool, bool) {
    match scene.doc.layer(layer_name(label)) {
        None => (true, false),
        Some(l) => (l.visible, l.locked),
    }
}

/// Does the forward ray touch the box?
pub fn ray_aabb(o: DVec3, d: DVec3, lo: DVec3, hi: DVec3) -> bool {
    ray_aabb_span(o, d, lo, hi).is_some()
}

/// `(t_in, t_out)` of the forward ray through the box.
pub fn ray_aabb_span(o: DVec3, d: DVec3, lo: DVec3, hi: DVec3) -> Option<(f64, f64)> {
    let (mut tmin, mut tmax) = (0.0f64, f64::INFINITY);
    for i in 0..3 {
        let di = d[i];
        if -1e-12 < di && di < 1e-12 {
            if o[i] < lo[i] - 1e-9 || o[i] > hi[i] + 1e-9 {
                return None;
            }
            continue;
        }
        let (mut t1, mut t2) = ((lo[i] - o[i]) / di, (hi[i] - o[i]) / di);
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmin > tmax {
            return None;
        }
    }
    Some((tmin, tmax))
}

/// Distance from `p` to the segment `a`–`b` on screen.
pub fn point_to_segment_distance_2d(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if dx == 0.0 && dy == 0.0 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// Screen distance from `(px, py)` to segment `pa`–`pb`.
fn seg_dist(px: f64, py: f64, pa: (f64, f64), pb: (f64, f64)) -> f64 {
    let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
    let l2 = dx * dx + dy * dy;
    let safe = if l2 > 1e-12 { l2 } else { 1.0 };
    let t = (((px - pa.0) * dx + (py - pa.1) * dy) / safe).clamp(0.0, 1.0);
    (pa.0 + t * dx - px).hypot(pa.1 + t * dy - py)
}

impl Viewport {
    // ---- projection ----------------------------------------------------------------
    /// The camera's model-view-projection.
    pub fn mvp(&self) -> DMat4 {
        self.camera.projection_matrix() * self.camera.view_matrix()
    }

    /// World point → widget pixel, `None` behind the camera.
    pub fn world_to_pixel(&self, world: DVec3) -> Option<ScreenPt> {
        self.probe.world_to_pixel(self, world)
    }

    pub(super) fn camera_world_to_pixel(&self, world: DVec3) -> Option<ScreenPt> {
        self.camera_projection().to_pixel(world)
    }

    /// The camera's projection onto the widget.
    pub(super) fn camera_projection(&self) -> Projection {
        Projection {
            mvp: self.mvp(),
            width: self.width(),
            height: self.height(),
        }
    }

    /// Camera ray (origin, unit direction) through a pixel.
    pub fn pixel_to_ray(&self, p: ScreenPt) -> Option<(DVec3, DVec3)> {
        self.probe.pixel_to_ray(self, p)
    }

    pub(super) fn camera_pixel_to_ray(&self, p: ScreenPt) -> Option<(DVec3, DVec3)> {
        let ScreenPt { x, y } = p;
        let w = self.width().max(1.0);
        let h = self.height().max(1.0);
        let ndc_x = 2.0 * x / w - 1.0;
        let ndc_y = 1.0 - 2.0 * y / h;
        let inv = self.mvp().try_inverse()?;
        let p_near = inv.project_point(DVec3::new(ndc_x, ndc_y, -1.0));
        let p_far = inv.project_point(DVec3::new(ndc_x, ndc_y, 1.0));
        let d = p_far - p_near;
        if d.length() < 1e-9 {
            return None;
        }
        Some((p_near, d.unit_or_zero()))
    }

    /// Clip a world segment to the part in FRONT of the camera.
    pub fn clip_segment_front(&self, a: DVec3, b: DVec3) -> Option<(DVec3, DVec3)> {
        let m = self.mvp();
        let wa = (m * a.extend(1.0)).w;
        let wb = (m * b.extend(1.0)).w;
        let eps = 1e-3;
        if wa <= eps && wb <= eps {
            return None;
        }
        if wa <= eps || wb <= eps {
            let t = (eps - wa) / (wb - wa);
            let cut = a + (b - a) * t;
            return Some(if wa <= eps { (cut, b) } else { (a, cut) });
        }
        Some((a, b))
    }

    // ---- placements ------------------------------------------------------------------------
    /// `scene.groups` with every nested placement expanded.
    pub fn placements(&self) -> Vec<Placement> {
        let mut out = Vec::new();
        for g in &self.scene.doc.groups {
            self.expand_placements(g, &mut out);
        }
        out
    }

    /// `group` followed by its nested placements.
    pub fn expand_placements(&self, group: &Group, out: &mut Vec<Placement>) {
        out.push(Placement {
            group: group.id,
            mesh: group.mesh,
            xform: group.xform,
            owner: group.id,
            context: None,
            layer: group.layer.clone(),
            hidden: group.hidden,
            material: group.material.clone(),
            nested: false,
            owner_is_instance: group.is_instance(),
        });
        if group.children.is_empty() {
            return;
        }
        let (vis, locked) = layer_state(&self.scene, group.layer.as_deref());
        let forced = if !vis || locked {
            group.layer.clone()
        } else {
            None
        };
        let ctx = self.scene.edit_group();
        struct Walk<'a> {
            out: &'a mut Vec<Placement>,
            forced: Option<String>,
            ctx: Option<GroupId>,
            top: &'a Group,
        }
        fn walk(
            w: &mut Walk<'_>,
            node: &Group,
            world: Option<DMat4>,
            inside: Option<GroupId>,
            hidden: bool,
        ) {
            for child in &node.children {
                let m = match (world, child.xform) {
                    (None, x) => x,
                    (Some(wm), Some(cx)) => Some(wm * cx),
                    (Some(wm), None) => Some(wm),
                };
                let layer = w
                    .forced
                    .clone()
                    .or_else(|| child.layer.clone())
                    .or_else(|| node.layer.clone());
                let phidden = hidden || child.hidden;
                let (owner, context) =
                    if Some(child.id) == w.ctx || (w.ctx.is_some() && Some(node.id) == w.ctx) {
                        (child.id, w.ctx)
                    } else if let Some(d) = inside {
                        (d, w.ctx)
                    } else {
                        (w.top.id, None)
                    };
                let owner_is_instance = if owner == w.top.id {
                    w.top.is_instance()
                } else {
                    w.top.find(owner).is_some_and(|g| g.is_instance())
                };
                let owner_paint = if owner == w.top.id {
                    w.top.material.as_ref()
                } else {
                    w.top.find(owner).and_then(|g| g.material.as_ref())
                };
                let material = effective_material(child.material.as_ref(), owner_paint).cloned();
                w.out.push(Placement {
                    group: child.id,
                    mesh: child.mesh,
                    xform: m,
                    owner,
                    context,
                    layer,
                    hidden: phidden,
                    material,
                    nested: true,
                    owner_is_instance,
                });
                if !child.children.is_empty() {
                    let next = if w.ctx.is_some() && Some(node.id) == w.ctx {
                        Some(child.id)
                    } else {
                        inside
                    };
                    walk(w, child, m, next, phidden);
                }
            }
        }
        let mut w = Walk {
            out,
            forced,
            ctx,
            top: group,
        };
        walk(&mut w, group, group.xform, None, group.hidden);
    }

    fn placement_visible(&self, p: &Placement) -> bool {
        !p.hidden && layer_state(&self.scene, p.layer.as_deref()).0
    }

    fn placement_selectable(&self, p: &Placement) -> bool {
        if p.hidden && !self.scene.show_hidden_objects {
            return false;
        }
        let (v, l) = layer_state(&self.scene, p.layer.as_deref());
        v && !l
    }

    /// Whether a placement is part of the surroundings of the group being
    /// edited (so it fades or hides), rather than the subject itself.
    pub fn draws_in_edit_context(&self, p: &Placement) -> bool {
        let Some(ctx) = self.scene.edit_group() else {
            return false;
        };
        if p.group == ctx || p.context == Some(ctx) {
            return false; // the container and what lives inside it
        }
        p.owner != ctx
    }

    /// The placements the current context lets you touch.
    pub fn context_placements(&self) -> Vec<Placement> {
        let all = self.placements();
        let Some(ctx) = self.scene.edit_group() else {
            return all;
        };
        all.into_iter()
            .filter(|p| p.context == Some(ctx) && p.owner != ctx)
            .collect()
    }

    // ---- the index ---------------------------------------------------------------------------
    fn index_key(&self) -> IndexKey {
        let s = &self.scene;
        IndexKey {
            version: s.version,
            loose_serial: s.mesh().mut_serial,
            edit: s.edit_stack.iter().map(|l| l.group).collect(),
            rest_hidden: self.rest_is_hidden(),
            show_hidden_objects: s.show_hidden_objects,
            show_hidden_geometry: s.show_hidden_geometry,
            layers: s
                .doc
                .layers
                .iter()
                .map(|l| (l.name.clone(), l.visible, l.locked))
                .collect(),
        }
    }

    /// The pick index, rebuilt when the scene changed.
    pub fn pick_index(&mut self) -> &PickIndex {
        let key = self.index_key();
        if self.pick.key.as_ref() != Some(&key) {
            let idx = self.build_pick_index();
            self.pick.index = idx;
            self.pick.key = Some(key);
            self.pick.hover_hits = None;
            self.pick.builds += 1;
        }
        &self.pick.index
    }

    /// How many times the index was built (tests: caching).
    pub fn pick_index_builds(&self) -> u64 {
        self.pick.builds
    }

    fn chunk(&mut self, mesh: MeshId) -> LocalChunk {
        let m = self.scene.doc.mesh(mesh);
        match self.pick.chunks.get(&mesh) {
            Some(c) if c.serial == m.mut_serial => c.clone(),
            _ => {
                let c = build_local_chunk(m);
                if self.pick.chunks.len() > 4096 {
                    self.pick.chunks.clear();
                }
                self.pick.chunks.insert(mesh, c.clone());
                c
            }
        }
    }

    fn build_pick_index(&mut self) -> PickIndex {
        let mut idx = PickIndex::default();
        let active = self.scene.active_mesh_id();
        // The loose part: the active mesh, every face (masks say what may
        // be hit), in mesh order.
        {
            let mesh = self.scene.mesh();
            let start = 0;
            for f in mesh.face_ids() {
                let e = EntityRef::Face(active, f);
                let i = idx.ents.len() as u32;
                idx.ents.push(FaceEnt {
                    mesh: active,
                    face: f,
                    placement: None,
                    owner: None,
                });
                idx.area.push(mesh.face_area(f));
                idx.vis.push(self.scene.entity_visible(&e));
                idx.sel.push(self.scene.entity_selectable(&e));
                idx.loose.push(true);
                for [a, b, c] in mesh.face_triangulate(f, None) {
                    idx.v0.push(a);
                    idx.e1.push(b - a);
                    idx.e2.push(c - a);
                    idx.tri_ent.push(i);
                }
            }
            if idx.v0.len() > start {
                idx.spans.push(Span {
                    bbox: None,
                    start,
                    count: idx.v0.len() - start,
                });
            }
        }
        // Groups: what the context lets you touch is selectable; the rest
        // of the model is in the index for snapping and occlusion only.
        let hidden_rest = self.rest_is_hidden();
        let touchable: Vec<Placement> = self.context_placements();
        let touch_ids: std::collections::HashSet<GroupId> =
            touchable.iter().map(|p| p.group).collect();
        let ctx = self.scene.edit_group();
        let candidates: Vec<Placement> = match ctx {
            None => touchable.clone(),
            Some(_) if hidden_rest => touchable.clone(),
            Some(c) => self
                .placements()
                .into_iter()
                .filter(|p| p.group != c && p.owner != c)
                .collect(),
        };
        for p in candidates {
            let gvis = self.placement_visible(&p);
            let gsnap = self.placement_selectable(&p);
            let gsel = gsnap && touch_ids.contains(&p.group);
            if !(gvis || gsnap) {
                continue;
            }
            let chunk = self.chunk(p.mesh);
            if chunk.faces.is_empty() && chunk.edges.is_empty() {
                continue;
            }
            let pi = idx.placements.len() as u32;
            let off = idx.ents.len() as u32;
            let xf = p.xform;
            let map = |q: DVec3| xf.map_or(q, |m| m.project_point(q));
            for (k, &f) in chunk.faces.iter().enumerate() {
                idx.ents.push(FaceEnt {
                    mesh: p.mesh,
                    face: f,
                    placement: Some(pi),
                    owner: Some(p.owner),
                });
                let area = match xf {
                    None => chunk.areas[k],
                    Some(_) => {
                        loop_area(&chunk.outers[k].iter().map(|q| map(*q)).collect::<Vec<_>>())
                    }
                };
                idx.area.push(area);
                idx.vis.push(gvis);
                idx.sel.push(gsel);
                idx.loose.push(false);
            }
            let start = idx.v0.len();
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            for (t, &fi) in chunk.tris.iter().zip(&chunk.tri_face) {
                let (a, b, c) = (map(t[0]), map(t[1]), map(t[2]));
                for q in [a, b, c] {
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
                idx.v0.push(a);
                idx.e1.push(b - a);
                idx.e2.push(c - a);
                idx.tri_ent.push(off + fi);
            }
            if idx.v0.len() > start {
                idx.spans.push(Span {
                    bbox: Some((lo, hi)),
                    start,
                    count: idx.v0.len() - start,
                });
            }
            if gsnap && !chunk.edges.is_empty() {
                let gi = idx.gedge_groups.len() as u32;
                idx.gedge_groups.push((p.owner, p.owner_is_instance));
                for &(a, b) in &chunk.edges {
                    idx.gedge_a.push(map(a));
                    idx.gedge_b.push(map(b));
                    idx.gedge_gi.push(gi);
                    idx.gedge_sel.push(gsel);
                }
            }
            idx.placements.push(p);
        }
        // Loose edges of the active mesh.
        let mesh = self.scene.mesh();
        for e in mesh.edge_ids() {
            let ed = mesh.edge(e);
            let (a, b) = mesh.edge_ab(e);
            idx.edges.push(e);
            idx.edge_a.push(a);
            idx.edge_b.push(b);
            idx.edge_soft.push(ed.soft);
            idx.edge_sel.push(
                self.scene.entity_selectable(&EntityRef::Edge(active, e))
                    && (!ed.hidden || self.scene.show_hidden_geometry),
            );
        }
        idx
    }

    // ---- ray queries ---------------------------------------------------------------------------
    /// Per-entity nearest ray parameter (`inf` = no hit) over the
    /// triangles whose entity passes `mask`.
    fn ray_hits(idx: &PickIndex, o: DVec3, d: DVec3, mask: Mask) -> Vec<f64> {
        let mut face_t = vec![f64::INFINITY; idx.ents.len()];
        Self::ray_walk(idx, o, d, mask, |e, t| {
            if t < face_t[e] {
                face_t[e] = t;
            }
        });
        face_t
    }

    /// The single nearest hit over `mask`, or `None`.
    fn ray_nearest(idx: &PickIndex, o: DVec3, d: DVec3, mask: Mask) -> Option<f64> {
        let mut best = f64::INFINITY;
        Self::ray_walk(idx, o, d, mask, |_, t| best = best.min(t));
        best.is_finite().then_some(best)
    }

    fn ray_walk(idx: &PickIndex, o: DVec3, d: DVec3, mask: Mask, mut hit: impl FnMut(usize, f64)) {
        let ok_ent = |e: usize| match mask {
            Mask::Visible => idx.vis[e],
            Mask::Selectable => idx.sel[e],
        };
        for span in &idx.spans {
            if let Some((lo, hi)) = span.bbox {
                if !ray_aabb(o, d, lo, hi) {
                    continue;
                }
            }
            for i in span.start..span.start + span.count {
                let e = idx.tri_ent[i] as usize;
                if !ok_ent(e) {
                    continue;
                }
                let (v0, e1, e2) = (idx.v0[i], idx.e1[i], idx.e2[i]);
                let p = d.cross(e2);
                let det = e1.dot(p);
                if det.abs() <= 1e-6 {
                    continue;
                }
                let inv = 1.0 / det;
                let s = o - v0;
                let u = s.dot(p) * inv;
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let q = s.cross(e1);
                let v = q.dot(d) * inv;
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let t = e2.dot(q) * inv;
                if t > 1e-6 {
                    hit(e, t);
                }
            }
        }
    }

    /// Per-entity nearest-hit `t` against every selectable entity for the
    /// cursor ray, with hits beyond the active cut dropped (memoised).
    fn hover_face_t(&mut self, o: DVec3, d: DVec3) -> Vec<f64> {
        self.pick_index();
        let key: HitKey = (
            self.scene.version,
            [
                o.x.to_bits(),
                o.y.to_bits(),
                o.z.to_bits(),
                d.x.to_bits(),
                d.y.to_bits(),
                d.z.to_bits(),
            ],
        );
        if let Some((k, v)) = &self.pick.hover_hits {
            if *k == key {
                return v.clone();
            }
        }
        let mut face_t = Self::ray_hits(&self.pick.index, o, d, Mask::Selectable);
        if let Some(sp) = active_cut(&self.scene) {
            let (n, c) = (sp.normal, sp.normal.dot(sp.point));
            for t in face_t.iter_mut().filter(|t| t.is_finite()) {
                if (o + d * *t).dot(n) - c > 1e-6 {
                    *t = f64::INFINITY;
                }
            }
        }
        self.pick.hover_hits = Some((key, face_t.clone()));
        face_t
    }

    /// The front-most entity; among hits at the same depth (coplanar
    /// overlapping faces) the smallest. "The same depth" is a
    /// ten-thousandth of the distance from the EYE, never under 0.1 mm
    /// (the parallel camera's ray starts far behind the eye).
    fn coplanar_choice(
        &self,
        idx: &PickIndex,
        face_t: &[f64],
        o: DVec3,
        d: DVec3,
    ) -> Option<usize> {
        let best_t = face_t.iter().cloned().fold(f64::INFINITY, f64::min);
        if !best_t.is_finite() {
            return None;
        }
        let t_eye = (self.camera.eye() - o).dot(d);
        let eps = (1e-4f64).max((best_t - t_eye).abs() * 1e-4);
        let cand: Vec<usize> = (0..face_t.len())
            .filter(|&i| face_t[i] <= best_t + eps)
            .collect();
        if cand.len() == 1 {
            return Some(cand[0]);
        }
        let mut best: Option<usize> = None;
        for &i in &cand {
            if best.is_none_or(|b| idx.area[i] < idx.area[b]) {
                best = Some(i);
            }
        }
        best
    }

    /// The loose face the cursor ray hits (front-most; among coplanar
    /// overlapping faces the smallest).
    pub fn pick_face(&mut self, x: f64, y: f64) -> Option<(MeshId, FaceId)> {
        self.probe().pick_face(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_face(&mut self, at: ScreenPt) -> Option<(MeshId, FaceId)> {
        let (o, d) = self.pixel_to_ray(at)?;
        self.pick_index();
        if self.pick.index.ents.is_empty() {
            return None;
        }
        let mut face_t = self.hover_face_t(o, d);
        let idx = &self.pick.index;
        for (i, t) in face_t.iter_mut().enumerate() {
            if !idx.loose[i] {
                *t = f64::INFINITY;
            }
        }
        let i = self.coplanar_choice(&self.pick.index, &face_t, o, d)?;
        let e = &self.pick.index.ents[i];
        Some((e.mesh, e.face))
    }

    fn face_hit(&self, i: usize, through_placement: bool) -> FaceHit {
        let idx = &self.pick.index;
        let e = &idx.ents[i];
        let pl = e.placement.map(|p| &idx.placements[p as usize]);
        FaceHit {
            mesh: e.mesh,
            face: e.face,
            placement: if through_placement {
                pl.map(|p| p.group)
            } else {
                e.owner
            },
            xform: pl.and_then(|p| p.xform),
            owner: e.owner,
        }
    }

    /// Front-most face across the loose mesh AND every group; `owner` is
    /// the group a click selects.
    pub fn pick_face_any(&mut self, x: f64, y: f64) -> Option<FaceHit> {
        self.probe().pick_face_any(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_face_any(&mut self, at: ScreenPt) -> Option<FaceHit> {
        let i = self.pick_face_any_index(at.x, at.y)?;
        Some(self.face_hit(i, false))
    }

    fn pick_face_any_index(&mut self, x: f64, y: f64) -> Option<usize> {
        let (o, d) = self.pixel_to_ray(ScreenPt::new(x, y))?;
        self.pick_index();
        if self.pick.index.ents.is_empty() {
            return None;
        }
        let face_t = self.hover_face_t(o, d);
        self.coplanar_choice(&self.pick.index, &face_t, o, d)
    }

    /// Like [`Self::pick_face_any`], but `placement` / `xform` are the
    /// placement the face was hit through — what every face-PLANE reading
    /// needs.
    pub fn pick_face_placement(&mut self, x: f64, y: f64) -> Option<FaceHit> {
        self.probe().pick_face_placement(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_face_placement(&mut self, at: ScreenPt) -> Option<FaceHit> {
        let i = self.pick_face_any_index(at.x, at.y)?;
        Some(self.face_hit(i, true))
    }

    /// The world plane of a hit face.
    pub fn face_plane_world(&self, hit: &FaceHit) -> (DVec3, DVec3) {
        snap::face_plane_world(self.scene.doc.mesh(hit.mesh), hit.face, hit.xform.as_ref())
    }

    /// The group whose geometry the cursor hits (front-most face, or the
    /// nearest edge for a lines-only group), or `None`.
    pub fn pick_group(&mut self, x: f64, y: f64) -> Option<GroupId> {
        self.probe().pick_group(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_group(&mut self, at: ScreenPt) -> Option<GroupId> {
        let ScreenPt { x, y } = at;
        if let Some(eg) = self.scene.edit_group() {
            if self
                .scene
                .doc
                .find_group(eg)
                .is_some_and(|g| g.children.is_empty())
            {
                return None;
            }
        }
        if let Some((o, d)) = self.pixel_to_ray(ScreenPt::new(x, y)) {
            self.pick_index();
            if !self.pick.index.ents.is_empty() {
                let face_t = self.hover_face_t(o, d);
                let idx = &self.pick.index;
                let mut best: Option<(f64, usize)> = None;
                for (i, &t) in face_t.iter().enumerate() {
                    if idx.loose[i] || !t.is_finite() {
                        continue;
                    }
                    if best.is_none_or(|(bt, _)| t < bt) {
                        best = Some((t, i));
                    }
                }
                if let Some((_, i)) = best {
                    return idx.ents[i].owner;
                }
            }
        }
        // Edge fallback.
        let mut best_d = self.prefs.pick_threshold_px;
        let mut best_g = None;
        let gp = self.gedge_screen();
        let idx = &self.pick.index;
        for (i, seg) in gp.iter().enumerate() {
            let Some((pa, pb)) = seg else { continue };
            if !idx.gedge_sel[i] {
                continue;
            }
            let dd = seg_dist(x, y, *pa, *pb);
            if dd < best_d {
                best_d = dd;
                best_g = Some(idx.gedge_groups[idx.gedge_gi[i] as usize].0);
            }
        }
        best_g
    }

    /// Screen projections of every group hard edge (`None` where an end is
    /// behind the camera), cached per view.
    fn gedge_screen(&mut self) -> EdgeScreens {
        self.pick_index();
        let key = self.view_key();
        if let (Some(k), Some((ck, screens))) = (key, &self.pick.group_screens) {
            if k == *ck {
                return Rc::clone(screens);
            }
        }
        let idx = &self.pick.index;
        let screens = self.project_edges(&idx.gedge_a, &idx.gedge_b);
        self.pick.projection_passes += 1;
        self.pick.group_screens = key.map(|k| (k, Rc::clone(&screens)));
        screens
    }

    /// Screen projections of every loose edge, cached per view.
    fn ledge_screen(&mut self) -> EdgeScreens {
        self.pick_index();
        let key = self.view_key();
        if let (Some(k), Some((ck, screens))) = (key, &self.pick.loose_screens) {
            if k == *ck {
                return Rc::clone(screens);
            }
        }
        let idx = &self.pick.index;
        let screens = self.project_edges(&idx.edge_a, &idx.edge_b);
        self.pick.projection_passes += 1;
        self.pick.loose_screens = key.map(|k| (k, Rc::clone(&screens)));
        screens
    }

    /// The view the edge projections are cached for; `None` when the probe
    /// projects point by point (nothing is cached then).
    fn view_key(&self) -> Option<ViewKey> {
        Some(ViewKey {
            projection: self.probe.projection(self)?,
            build: self.pick.builds,
        })
    }

    /// The screen segments of the edges `a[i]`–`b[i]`: through the probe's
    /// projection in one pass, else point by point.
    fn project_edges(&self, a: &[DVec3], b: &[DVec3]) -> EdgeScreens {
        match self.probe.projection(self) {
            Some(pr) => a
                .iter()
                .zip(b)
                .map(|(a, b)| Some((pr.to_pixel(*a)?.tuple(), pr.to_pixel(*b)?.tuple())))
                .collect(),
            None => a
                .iter()
                .zip(b)
                .map(|(a, b)| {
                    Some((
                        self.world_to_pixel(*a)?.tuple(),
                        self.world_to_pixel(*b)?.tuple(),
                    ))
                })
                .collect(),
        }
    }

    /// How many times the edges were projected on screen (tests: the
    /// per-view cache).
    pub fn edge_projection_passes(&self) -> u64 {
        self.pick.projection_passes
    }

    fn loose_picked_edge(&self, i: usize) -> PickedEdge {
        let idx = &self.pick.index;
        let mesh = self.scene.active_mesh_id();
        let e = idx.edges[i];
        let ed = self.scene.mesh().edge(e);
        PickedEdge {
            a: idx.edge_a[i],
            b: idx.edge_b[i],
            edge: Some((mesh, e)),
            in_group: false,
            context: None,
            group: None,
            soft: ed.soft,
            curve: ed.curve,
        }
    }

    fn group_picked_edge(&self, i: usize) -> PickedEdge {
        let idx = &self.pick.index;
        let (owner, inst) = idx.gedge_groups[idx.gedge_gi[i] as usize];
        PickedEdge {
            a: idx.gedge_a[i],
            b: idx.gedge_b[i],
            edge: None,
            in_group: true,
            context: Some(if inst {
                SnapContext::Component
            } else {
                SnapContext::Group
            }),
            group: Some(owner),
            soft: false,
            curve: None,
        }
    }

    /// The loose edge closest to the cursor within the pick threshold.
    pub fn pick_edge(&mut self, x: f64, y: f64) -> Option<PickedEdge> {
        self.probe().pick_edge(self, ScreenPt::new(x, y), false)
    }

    /// The edge a CLICK means: never a soft edge (unless hidden geometry
    /// shows) nor one hidden behind the model.
    pub fn pick_visible_edge(&mut self, x: f64, y: f64) -> Option<PickedEdge> {
        self.probe().pick_edge(self, ScreenPt::new(x, y), true)
    }

    pub(super) fn camera_pick_edge(
        &mut self,
        at: ScreenPt,
        visible_only: bool,
    ) -> Option<PickedEdge> {
        let ScreenPt { x, y } = at;
        let proj = self.ledge_screen();
        let idx = &self.pick.index;
        let cut = active_cut(&self.scene).map(|sp| (sp.normal, sp.normal.dot(sp.point)));
        let show_hidden = self.scene.show_hidden_geometry;
        let mut d: Vec<(f64, usize)> = Vec::new();
        for (i, seg) in proj.iter().enumerate() {
            let Some((pa, pb)) = seg else { continue };
            if !idx.edge_sel[i] {
                continue;
            }
            if visible_only && !show_hidden && idx.edge_soft[i] {
                continue;
            }
            if let Some((n, c)) = cut {
                if idx.edge_a[i].dot(n) - c > 1e-6 && idx.edge_b[i].dot(n) - c > 1e-6 {
                    continue;
                }
            }
            d.push((seg_dist(x, y, *pa, *pb), i));
        }
        if d.is_empty() {
            return None;
        }
        if !visible_only {
            let (dd, i) = d
                .iter()
                .cloned()
                .fold((f64::INFINITY, 0), |b, c| if c.0 < b.0 { c } else { b });
            return (dd < self.prefs.pick_threshold_px).then(|| self.loose_picked_edge(i));
        }
        d.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(dd, i) in d.iter().take(8) {
            if dd >= self.prefs.pick_threshold_px {
                return None;
            }
            let e = self.loose_picked_edge(i);
            match self.edge_point_under_cursor(e.a, e.b, x, y) {
                Some(p) if self.is_occluded(p) => continue,
                _ => return Some(e),
            }
        }
        None
    }

    /// The world point of edge `a`–`b` nearest the cursor.
    pub fn edge_point_under_cursor(&self, a: DVec3, b: DVec3, x: f64, y: f64) -> Option<DVec3> {
        let pa = self.world_to_pixel(a)?;
        let pb = self.world_to_pixel(b)?;
        let (dx, dy) = (pb.x - pa.x, pb.y - pa.y);
        let l2 = dx * dx + dy * dy;
        if l2 < 1e-12 {
            return Some(a);
        }
        let t = (((x - pa.x) * dx + (y - pa.y) * dy) / l2).clamp(0.0, 1.0);
        Some(a + (b - a) * t)
    }

    /// The edge under the cursor, loose OR a group's (as a world
    /// pseudo-edge), nearest first.
    pub fn pick_edge_any(&mut self, x: f64, y: f64) -> Option<PickedEdge> {
        self.probe().pick_edge_any(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_edge_any(&mut self, at: ScreenPt) -> Option<PickedEdge> {
        let ScreenPt { x, y } = at;
        let loose = self.pick_edge(x, y);
        let gp = self.gedge_screen();
        let mut best: Option<(f64, usize)> = None;
        for (i, seg) in gp.iter().enumerate() {
            let Some((pa, pb)) = seg else { continue };
            let dd = seg_dist(x, y, *pa, *pb);
            if best.is_none_or(|(b, _)| dd < b) {
                best = Some((dd, i));
            }
        }
        let Some((dg, i)) = best else { return loose };
        if dg >= self.prefs.pick_threshold_px {
            return loose;
        }
        if let Some(l) = &loose {
            if let (Some(pa), Some(pb)) = (self.world_to_pixel(l.a), self.world_to_pixel(l.b)) {
                let (dl, _t) = snap::closest_on_segment_2d((x, y), pa.tuple(), pb.tuple());
                if dl <= dg {
                    return loose;
                }
            }
        }
        Some(self.group_picked_edge(i))
    }

    /// Group hard edges within `radius_px` of the cursor, as snap
    /// pseudo-edges, nearest first, at most `cap`.
    pub fn nearby_group_edges(
        &mut self,
        x: f64,
        y: f64,
        radius_px: f64,
        cap: usize,
    ) -> Vec<SnapEdge> {
        let gp = self.gedge_screen();
        let mut cand: Vec<(f64, usize)> = gp
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|(pa, pb)| (seg_dist(x, y, pa, pb), i)))
            .filter(|(d, _)| *d < radius_px)
            .collect();
        if cand.len() > cap {
            cand.sort_by(|a, b| a.0.total_cmp(&b.0));
            cand.truncate(cap);
        }
        cand.into_iter()
            .map(|(_, i)| {
                let e = self.group_picked_edge(i);
                SnapEdge {
                    a: e.a,
                    b: e.b,
                    in_group: true,
                    context: e.context,
                    group: e.group,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Loose edges whose screen segment passes near the cursor, nearest
    /// first, at most `cap` (the snap prefilter of a big loose mesh).
    pub fn nearby_loose_edges(
        &mut self,
        x: f64,
        y: f64,
        radius_px: f64,
        cap: usize,
    ) -> Vec<EdgeId> {
        let proj = self.ledge_screen();
        let mut cand: Vec<(f64, usize)> = proj
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|(pa, pb)| (seg_dist(x, y, pa, pb), i)))
            .filter(|(d, _)| *d < radius_px)
            .collect();
        if cand.len() > cap {
            cand.sort_by(|a, b| a.0.total_cmp(&b.0));
            cand.truncate(cap);
        }
        let idx = &self.pick.index;
        cand.into_iter().map(|(_, i)| idx.edges[i]).collect()
    }

    /// The corner (loose or a group's) closest to the cursor within the
    /// pick threshold, skipping occluded ones.
    pub fn pick_vertex(&mut self, x: f64, y: f64) -> Option<DVec3> {
        self.pick_vertex_skipping(x, y, None)
    }

    /// Like [`Self::pick_vertex`], never a corner of what is in motion.
    pub fn pick_still_vertex(&mut self, x: f64, y: f64, moving: &InMotion) -> Option<DVec3> {
        self.pick_vertex_skipping(x, y, Some(moving))
    }

    fn pick_vertex_skipping(&mut self, x: f64, y: f64, moving: Option<&InMotion>) -> Option<DVec3> {
        let lp = self.ledge_screen();
        let gp = self.gedge_screen();
        let idx = &self.pick.index;
        // Per loose and per group edge: does it move?
        let loose_moves: Vec<bool> = idx
            .edges
            .iter()
            .map(|e| moving.is_some_and(|m| m.edges.contains(e)))
            .collect();
        let group_moves: Vec<bool> = idx
            .gedge_gi
            .iter()
            .map(|&gi| {
                let owner = idx.gedge_groups[gi as usize].0;
                moving.is_some_and(|m| m.placements.contains(&owner))
            })
            .collect();
        let mut cand: Vec<(f64, DVec3)> = Vec::new();
        let mut push = |pt: Option<(f64, f64)>, w: DVec3, moves: bool| {
            if moves {
                return;
            }
            if let Some(p) = pt {
                let d = (p.0 - x).hypot(p.1 - y);
                if d < self.prefs.pick_threshold_px {
                    cand.push((d, w));
                }
            }
        };
        // The concatenation order: loose a, loose b, group a, group b.
        for ((s, w), m) in lp.iter().zip(&idx.edge_a).zip(&loose_moves) {
            push(s.map(|s| s.0), *w, *m);
        }
        for ((s, w), m) in lp.iter().zip(&idx.edge_b).zip(&loose_moves) {
            push(s.map(|s| s.1), *w, *m);
        }
        for ((s, w), m) in gp.iter().zip(&idx.gedge_a).zip(&group_moves) {
            push(s.map(|s| s.0), *w, *m);
        }
        for ((s, w), m) in gp.iter().zip(&idx.gedge_b).zip(&group_moves) {
            push(s.map(|s| s.1), *w, *m);
        }
        // Stable sort: equal distances keep the concatenation order.
        cand.sort_by(|a, b| a.0.total_cmp(&b.0));
        cand.into_iter()
            .map(|(_, w)| w)
            .find(|&w| !self.is_occluded(w))
    }

    /// Whether geometry sits between the camera and `world` (only VISIBLE
    /// geometry occludes; X-ray and wireframe show everything).
    pub fn is_occluded(&mut self, world: DVec3) -> bool {
        let mode = self.effective_style().face_mode.clone();
        if mode == "xray" || mode == "wireframe" {
            return false;
        }
        self.pick_index();
        if self.pick.index.v0.is_empty() {
            return false;
        }
        let origin = self.eye();
        let delta = world - origin;
        let dist = delta.length();
        if dist < 1e-9 {
            return false;
        }
        let d = delta / dist;
        match active_cut(&self.scene).map(|sp| (sp.normal, sp.normal.dot(sp.point))) {
            None => Self::ray_nearest(&self.pick.index, origin, d, Mask::Visible)
                .is_some_and(|t| t < dist - 1e-3),
            Some((n, c)) => {
                let face_t = Self::ray_hits(&self.pick.index, origin, d, Mask::Visible);
                face_t
                    .iter()
                    .filter(|t| t.is_finite() && **t < dist - 1e-3)
                    .any(|t| (origin + d * *t).dot(n) - c <= 1e-6)
            }
        }
    }

    /// Distance along `direction` to the nearest VISIBLE face.
    pub fn ray_distance(&mut self, origin: DVec3, direction: DVec3) -> Option<f64> {
        self.pick_index();
        Self::ray_nearest(&self.pick.index, origin, direction, Mask::Visible)
    }

    /// The world point the cursor points at: nearest geometry, else the
    /// ground, else the focal plane through the target.
    /// The frame on screen answers first ([`Self::depth_world_at`]); the
    /// ray against the pick index only when there is no frame to read.
    pub fn world_under_cursor(&mut self, x: f64, y: f64) -> Option<DVec3> {
        let (o, d) = self.pixel_to_ray(ScreenPt::new(x, y))?;
        let seen = self.depth_world_at(x, y);
        if let Some(DepthSeen::Hit(p)) = seen {
            return Some(p);
        }
        if seen.is_none() {
            if let Some(t) = self.ray_distance(o, d) {
                return Some(o + d * t);
            }
        }
        if d.z.abs() > 1e-6 {
            let t = -o.z / d.z;
            if t > 0.0 {
                return Some(o + d * t);
            }
        }
        let view = self.camera.target - self.camera.eye();
        if view.length() > 1e-9 {
            let view = view.unit_or_zero();
            let denom = d.dot(view);
            if denom.abs() > 1e-6 {
                let t = (self.camera.target - o).dot(view) / denom;
                if t > 0.0 {
                    return Some(o + d * t);
                }
            }
        }
        None
    }

    /// The point a pan starting at `(x, y)` grabs: the model point under
    /// the cursor, else the middle of the view's central ray through the
    /// model box, else the target.
    /// The model point comes from the frame on screen when it can
    /// ([`Self::depth_world_at`]), from the pick ray otherwise.
    pub fn grab_point_at(&mut self, x: f64, y: f64) -> DVec3 {
        let seen = self.depth_world_at(x, y);
        if let Some(DepthSeen::Hit(p)) = seen {
            return p;
        }
        if let Some((o, d)) = self
            .pixel_to_ray(ScreenPt::new(x, y))
            .filter(|_| seen.is_none())
        {
            self.pick_index();
            if !self.pick.index.ents.is_empty() {
                let face_t = self.hover_face_t(o, d);
                let t = face_t.iter().cloned().fold(f64::INFINITY, f64::min);
                if t.is_finite() && t > 0.0 {
                    return o + d * t;
                }
            }
        }
        if let Some((lo, hi)) = self.scene.bounds() {
            if let Some((o, d)) =
                self.pixel_to_ray(ScreenPt::new(self.width() / 2.0, self.height() / 2.0))
            {
                if let Some((t0, t1)) = ray_aabb_span(o, d, lo, hi) {
                    return o + d * ((t0 + t1) * 0.5);
                }
            }
        }
        self.camera.target
    }

    /// The dimension whose lines pass nearest the cursor.
    pub fn pick_dimension(&mut self, x: f64, y: f64) -> Option<ObjId> {
        self.probe().pick_dimension(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_dimension(&mut self, at: ScreenPt) -> Option<ObjId> {
        let ScreenPt { x, y } = at;
        let mut best = None;
        let mut best_d = self.prefs.pick_threshold_px;
        let doc = self.scene.doc_view();
        for dim in &self.scene.doc.dimensions {
            if !self.scene.entity_selectable(&EntityRef::Dimension(dim.id)) {
                continue;
            }
            let (ap, bp) = dim.line_points(&doc);
            let (da, db) = (dim.a(&doc), dim.b(&doc));
            for (s, e) in [(da, ap), (db, bp), (ap, bp)] {
                let (Some(ps), Some(pe)) = (self.world_to_pixel(s), self.world_to_pixel(e)) else {
                    continue;
                };
                let dd = point_to_segment_distance_2d((x, y), ps.tuple(), pe.tuple());
                if dd < best_d {
                    best_d = dd;
                    best = Some(dim.id);
                }
            }
        }
        best
    }

    /// The construction guide nearest the cursor.
    pub fn pick_guide(&mut self, x: f64, y: f64) -> Option<ObjId> {
        self.probe().pick_guide(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_guide(&mut self, at: ScreenPt) -> Option<ObjId> {
        let ScreenPt { x, y } = at;
        let mut best = None;
        let mut best_d = self.prefs.pick_threshold_px;
        for g in &self.scene.doc.guides {
            let (a, b) = g.segment();
            let Some((a, b)) = self.clip_segment_front(a, b) else {
                continue;
            };
            let (Some(pa), Some(pb)) = (self.world_to_pixel(a), self.world_to_pixel(b)) else {
                continue;
            };
            let dd = point_to_segment_distance_2d((x, y), pa.tuple(), pb.tuple());
            if dd < best_d {
                best_d = dd;
                best = Some(g.id);
            }
        }
        best
    }

    /// The model axis line under the cursor.
    pub fn pick_axis(&self, x: f64, y: f64) -> Option<Axis> {
        self.probe.pick_axis(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_axis(&self, at: ScreenPt) -> Option<Axis> {
        let ScreenPt { x, y } = at;
        let mut best = None;
        let mut best_d = self.prefs.pick_threshold_px;
        let o = xtrude_core::axes::origin();
        for name in Axis::ALL {
            let d = xtrude_core::axes::axis(name) * xtrude_model::guide::GUIDE_HALF_LEN;
            let Some((a, b)) = self.clip_segment_front(o - d, o + d) else {
                continue;
            };
            let (Some(pa), Some(pb)) = (self.world_to_pixel(a), self.world_to_pixel(b)) else {
                continue;
            };
            let dd = point_to_segment_distance_2d((x, y), pa.tuple(), pb.tuple());
            if dd < best_d {
                best_d = dd;
                best = Some(name);
            }
        }
        best
    }
    /// Four world corners of a section plane's drawn frame: the model's
    /// bounds projected onto the plane, with a margin.
    pub fn section_plane_frame(&mut self, point: DVec3, normal: DVec3) -> [DVec3; 4] {
        let (u, v) = xtrude_core::triangulate::plane_axes(normal);
        let (cu, cv, half_u, half_v) = match self.scene.bounds() {
            None => (0.0, 0.0, 1.5, 1.5),
            Some((lo, hi)) => {
                let mut us = Vec::new();
                let mut vs = Vec::new();
                for x in [lo.x, hi.x] {
                    for y in [lo.y, hi.y] {
                        for z in [lo.z, hi.z] {
                            let c = DVec3::new(x, y, z);
                            us.push((c - point).dot(u));
                            vs.push((c - point).dot(v));
                        }
                    }
                }
                let (umin, umax) = us.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, &b| {
                    (a.0.min(b), a.1.max(b))
                });
                let (vmin, vmax) = vs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, &b| {
                    (a.0.min(b), a.1.max(b))
                });
                (
                    (umin + umax) * 0.5,
                    (vmin + vmax) * 0.5,
                    (umax - umin) * 0.56 + 0.2,
                    (vmax - vmin) * 0.56 + 0.2,
                )
            }
        };
        let c = point + u * cu + v * cv;
        [
            c - u * half_u - v * half_v,
            c + u * half_u - v * half_v,
            c + u * half_u + v * half_v,
            c - u * half_u + v * half_v,
        ]
    }

    /// The section plane whose frame is nearest the cursor.
    pub fn pick_section_plane(&mut self, x: f64, y: f64) -> Option<ObjId> {
        self.probe().pick_section_plane(self, ScreenPt::new(x, y))
    }

    pub(super) fn camera_pick_section_plane(&mut self, at: ScreenPt) -> Option<ObjId> {
        let ScreenPt { x, y } = at;
        if self.scene.doc.section_planes.is_empty() || !self.scene.show_section_planes {
            return None;
        }
        let planes: Vec<(ObjId, DVec3, DVec3)> = self
            .scene
            .doc
            .section_planes
            .iter()
            .map(|p| (p.id, p.point, p.normal))
            .collect();
        let mut best = None;
        let mut best_d = self.prefs.pick_threshold_px;
        for (id, point, normal) in planes {
            let corners = self.section_plane_frame(point, normal);
            for i in 0..4 {
                let Some((a, b)) = self.clip_segment_front(corners[i], corners[(i + 1) % 4]) else {
                    continue;
                };
                let (Some(pa), Some(pb)) = (self.world_to_pixel(a), self.world_to_pixel(b)) else {
                    continue;
                };
                let dd = point_to_segment_distance_2d((x, y), pa.tuple(), pb.tuple());
                if dd < best_d {
                    best_d = dd;
                    best = Some(id);
                }
            }
        }
        best
    }

    /// Left edge of a leader-text block whose leader ends at `p_pos`: on
    /// the side away from the anchor.
    pub fn text_block_x(p_pos: (f64, f64), p_anchor: Option<(f64, f64)>, block_w: f64) -> f64 {
        match p_anchor {
            Some(a) if p_pos.0 < a.0 => p_pos.0 - 6.0 - block_w,
            _ => p_pos.0 + 6.0,
        }
    }

    /// The font the leader texts are drawn with.
    pub fn label_font(&self) -> crate::fontmetrics::FontMetrics {
        let size = self.scene.dimension_style.font_size as f64;
        crate::fontmetrics::FontMetrics::new(size, true)
    }

    /// The leader-text label under the cursor: its drawn text block, else
    /// (unless `rect_only`) its position or leader within the threshold.
    pub fn pick_text_label(&mut self, x: f64, y: f64, rect_only: bool) -> Option<ObjId> {
        self.probe()
            .pick_text_label(self, ScreenPt::new(x, y), rect_only)
    }

    pub(super) fn camera_pick_text_label(
        &mut self,
        at: ScreenPt,
        rect_only: bool,
    ) -> Option<ObjId> {
        let ScreenPt { x, y } = at;
        if self.scene.doc.text_labels.is_empty() {
            return None;
        }
        let fm = self.label_font();
        let mut best = None;
        let mut best_d = self.prefs.pick_threshold_px * 2.0;
        for lab in &self.scene.doc.text_labels {
            if !self.scene.entity_selectable(&EntityRef::TextLabel(lab.id)) {
                continue;
            }
            let Some(pp) = self.world_to_pixel(lab.position()) else {
                continue;
            };
            let pa = self.world_to_pixel(lab.anchor);
            let lines: Vec<&str> = if lab.text.is_empty() {
                vec![""]
            } else {
                lab.text.lines().collect()
            };
            let block_w = lines
                .iter()
                .map(|l| fm.horizontal_advance(l))
                .fold(0.0, f64::max);
            let x0 = Self::text_block_x(pp.tuple(), pa.map(|p| p.tuple()), block_w);
            for (i, line) in lines.iter().enumerate() {
                let base = pp.y - 4.0 + i as f64 * fm.height();
                if x0 - 3.0 <= x
                    && x <= x0 + fm.horizontal_advance(line) + 3.0
                    && base - fm.ascent() - 3.0 <= y
                    && y <= base + fm.descent() + 3.0
                {
                    return Some(lab.id);
                }
            }
            if rect_only {
                continue;
            }
            let mut dd = (pp.x - x).hypot(pp.y - y);
            if let Some(a) = pa {
                dd = dd.min(point_to_segment_distance_2d((x, y), a.tuple(), pp.tuple()));
            }
            if dd < best_d {
                best_d = dd;
                best = Some(lab.id);
            }
        }
        if rect_only {
            None
        } else {
            best
        }
    }
}
