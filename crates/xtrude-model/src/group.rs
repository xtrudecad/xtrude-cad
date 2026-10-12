// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Group: a self-contained chunk of geometry with its own mesh.
//!
//! The scene's main mesh welds every coincident vertex (sticky geometry). A
//! Group isolates geometry into its *own* mesh: nothing welds across the
//! group boundary, so it moves and edits as a unit.
//!
//! A classic group owns its mesh in world coordinates; a component instance
//! shares its prototype mesh (same [`MeshId`]) and carries `xform`. The
//! copies of a component share their contents too (see
//! [`crate::definitions`]).
//!
//! Design note: a group holds a [`MeshId`] into the scene's mesh table,
//! not the mesh itself, so sharing a prototype is sharing
//! an id, and a copy-on-write `Arc` keeps undo snapshots cheap.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{Map, Value};
use xtrude_core::attrs::Paint;
use xtrude_core::math::{DMat4, DVec3, MatExt};
use xtrude_core::mesh::{EdgeFlags, Mesh};
use xtrude_core::topology::maximal_holes;

use crate::ids::{next_id, GroupId, MeshId};

/// The scene's mesh table.
pub type MeshTable = HashMap<MeshId, Arc<Mesh>>;

static GROUP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A fresh "Group N" name.
pub fn next_group_name() -> String {
    format!("Group {}", GROUP_COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Move the automatic "Group N" counter past every N in `names`.
pub fn reserve_group_names<'a>(names: impl IntoIterator<Item = &'a str>) {
    let mut top = 0u64;
    for n in names {
        if let Some(rest) = n.strip_prefix("Group ") {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(k) = rest.parse::<u64>() {
                    top = top.max(k);
                }
            }
        }
    }
    GROUP_COUNTER.fetch_max(top + 1, Ordering::SeqCst);
}

/// A fresh group identity: random, so two documents merged never collide.
pub fn new_uid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x =
        nanos ^ next_id().wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (std::process::id() as u64) << 32;
    // splitmix64
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    format!("{z:016x}")
}

/// Exploded view state of a container.
#[derive(Clone, Debug, PartialEq)]
pub struct Exploded {
    pub factor: f64,
    pub mode: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Runtime identity (selection, commands).
    pub id: GroupId,
    pub mesh: MeshId,
    pub name: String,
    pub layer: Option<String>,
    /// 3D text parameters this container was generated from.
    pub text3d: Option<Map<String, Value>>,
    /// Extensions' own parameters (kept and saved, never read).
    pub ext: Option<Map<String, Value>>,
    pub hidden: bool,
    /// Stable identity that survives save/load.
    pub uid: String,
    /// The container's own paint.
    pub material: Option<Paint>,
    pub exploded: Option<Exploded>,
    pub explode_offset: Option<DVec3>,
    /// Component instance placement (local → world); `None` = classic group.
    pub xform: Option<DMat4>,
    /// Nested placements the group owns.
    pub children: Vec<Group>,
    /// Local axes of a classic group.
    pub axes: Option<DMat4>,
    /// Whether this instance is a COMPONENT (vs a group carrying a matrix).
    pub component: bool,
}

impl Group {
    pub fn new(mesh: MeshId, name: Option<&str>) -> Group {
        Group {
            id: next_id(),
            mesh,
            name: name.map(str::to_string).unwrap_or_else(next_group_name),
            layer: None,
            text3d: None,
            ext: None,
            hidden: false,
            uid: new_uid(),
            material: None,
            exploded: None,
            explode_offset: None,
            xform: None,
            children: Vec::new(),
            axes: None,
            component: true,
        }
    }

    /// A component instance, as the user knows it.
    pub fn is_component(&self) -> bool {
        self.xform.is_some() && self.component
    }

    pub fn is_instance(&self) -> bool {
        self.xform.is_some()
    }

    /// Take `children` as nested placements: a group that owns placements
    /// is always an instance.
    pub fn adopt(&mut self, children: Vec<Group>) {
        self.children = children;
        if !self.children.is_empty() && self.xform.is_none() {
            self.xform = Some(DMat4::IDENTITY);
        }
    }

    /// Depth-first search for a group by id in this subtree.
    pub fn find(&self, id: GroupId) -> Option<&Group> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }

    pub fn find_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(id))
    }

    /// Every group in this subtree, depth first (self first).
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Group>) {
        out.push(self);
        for c in &self.children {
            c.walk(out);
        }
    }
}

/// A classic group's geometry was just transformed by `m` in world space:
/// its axes go with it. Instances are left alone.
pub fn carry_axes(group: &mut Group, m: &DMat4) {
    if group.xform.is_some() {
        return;
    }
    let base = group.axes.unwrap_or(DMat4::IDENTITY);
    group.axes = Some(*m * base);
}

/// The world matrix of `group`'s own axes, or `None` for the world axes.
pub fn group_frame(group: &Group) -> Option<DMat4> {
    match (group.xform, group.axes) {
        (None, axes) => axes,
        (Some(x), None) => Some(x),
        (Some(x), Some(a)) => Some(x * a),
    }
}

/// `(group, world_matrix)` for `group` and every nested placement below it,
/// depth first, each matrix composed from the root down. `None` only when
/// the whole chain is untransformed.
pub fn iter_placements(group: &Group, base: Option<DMat4>) -> Vec<(&Group, Option<DMat4>)> {
    let mut out = Vec::new();
    fn rec<'a>(g: &'a Group, base: Option<DMat4>, out: &mut Vec<(&'a Group, Option<DMat4>)>) {
        let m = match (base, g.xform) {
            (None, x) => x,
            (Some(b), None) => Some(b),
            (Some(b), Some(x)) => Some(b * x),
        };
        out.push((g, m));
        for c in &g.children {
            rec(c, m, out);
        }
    }
    rec(group, base, &mut out);
    out
}

/// A DEEP copy of `src` with every position mapped through `m`, carrying
/// face attrs and soft/curve flags.
pub fn transformed_mesh(src: &Mesh, m: &DMat4) -> Mesh {
    let mut new = Mesh::new();
    let w = |p: DVec3| m.project_point(p);
    if src.edges.len() * 2 + src.faces.len() * 4 < 1024 {
        // Sequential per-entity copy — exact semantics for small meshes.
        for (fid, f) in src.faces.iter() {
            let outer: Vec<DVec3> = src.face_positions(fid).into_iter().map(w).collect();
            let holes: Vec<Vec<DVec3>> = src
                .face_hole_positions(fid)
                .into_iter()
                .map(|h| h.into_iter().map(w).collect())
                .collect();
            let nf = new.add_face(&outer, &holes);
            if !f.attrs.is_empty() {
                new.faces[nf].attrs.update(&f.attrs);
            }
        }
        for (eid, e) in src.edges.iter() {
            let (a, b) = src.edge_ab(eid);
            let (wa, wb) = (w(a), w(b));
            let existing = match (new.vertex_at(wa), new.vertex_at(wb)) {
                (Some(v0), Some(v1)) => new.find_edge(v0, v1),
                _ => None,
            };
            let ne = match existing {
                Some(ne) => ne,
                None => match new.add_edge(wa, wb) {
                    Ok(ne) => ne,
                    Err(_) => continue,
                },
            };
            let nd = new.edge_mut(ne);
            nd.soft = e.soft;
            nd.curve = e.curve;
        }
        new.resplit_curves();
        return new;
    }
    // Bulk path: every edge endpoint and face corner welds in order.
    let mut flat: Vec<DVec3> = Vec::new();
    let mut flags: Vec<EdgeFlags> = Vec::new();
    for (eid, e) in src.edges.iter() {
        let (a, b) = src.edge_ab(eid);
        flat.push(w(a));
        flat.push(w(b));
        flags.push(EdgeFlags {
            soft: e.soft,
            curve: e.curve,
            layer: None,
            hidden: false,
        });
    }
    let n_edge_pts = flat.len();
    let mut rings: Vec<(Vec<DVec3>, Vec<Vec<DVec3>>, xtrude_core::attrs::FaceAttrs)> = Vec::new();
    for (fid, f) in src.faces.iter() {
        let mut holes = src.face_hole_positions(fid);
        if holes.len() > 1 {
            holes = maximal_holes(&holes);
        }
        rings.push((src.face_positions(fid), holes, f.attrs.clone()));
    }
    for (outer, holes, _) in &rings {
        flat.extend(outer.iter().map(|&p| w(p)));
        for h in holes {
            flat.extend(h.iter().map(|&p| w(p)));
        }
    }
    let (vobjs, ids) = new.bulk_weld(&flat);
    if n_edge_pts > 0 {
        let ia: Vec<usize> = ids[..n_edge_pts].iter().step_by(2).copied().collect();
        let ib: Vec<usize> = ids[..n_edge_pts]
            .iter()
            .skip(1)
            .step_by(2)
            .copied()
            .collect();
        new.add_edges_welded(&vobjs, &ia, &ib, Some(&flags));
    }
    let mut k = n_edge_pts;
    for (outer, holes, attrs) in rings {
        let o: Vec<_> = (0..outer.len()).map(|i| vobjs[ids[k + i]]).collect();
        k += outer.len();
        let mut hs = Vec::new();
        for h in &holes {
            hs.push((0..h.len()).map(|i| vobjs[ids[k + i]]).collect());
            k += h.len();
        }
        let nf = new.add_face_welded(o, hs);
        if !attrs.is_empty() {
            new.faces[nf].attrs.update(&attrs);
        }
    }
    new.resplit_curves();
    new
}

/// Every placement of `group` welded into ONE world-space mesh.
pub fn merged_placements(meshes: &MeshTable, group: &Group) -> Mesh {
    let mut new = Mesh::new();
    let mut e_pts: Vec<DVec3> = Vec::new();
    let mut flags: Vec<EdgeFlags> = Vec::new();
    let mut rings: Vec<(Vec<DVec3>, Vec<Vec<DVec3>>, xtrude_core::attrs::FaceAttrs)> = Vec::new();
    for (g, m) in iter_placements(group, None) {
        let Some(src) = meshes.get(&g.mesh) else {
            continue;
        };
        let w = |p: DVec3| m.map_or(p, |mm| mm.project_point(p));
        for (eid, e) in src.edges.iter() {
            let (a, b) = src.edge_ab(eid);
            e_pts.push(w(a));
            e_pts.push(w(b));
            flags.push(EdgeFlags {
                soft: e.soft,
                curve: e.curve,
                layer: None,
                hidden: false,
            });
        }
        for (fid, f) in src.faces.iter() {
            let mut holes = src.face_hole_positions(fid);
            if holes.len() > 1 {
                holes = maximal_holes(&holes);
            }
            rings.push((
                src.face_positions(fid).into_iter().map(w).collect(),
                holes
                    .into_iter()
                    .map(|h| h.into_iter().map(w).collect())
                    .collect(),
                f.attrs.clone(),
            ));
        }
    }
    if e_pts.is_empty() && rings.is_empty() {
        return new;
    }
    let n_edge_pts = e_pts.len();
    let mut flat = e_pts;
    for (o, hs, _) in &rings {
        flat.extend(o.iter().copied());
        for h in hs {
            flat.extend(h.iter().copied());
        }
    }
    let (vobjs, ids) = new.bulk_weld(&flat);
    if n_edge_pts > 0 {
        let ia: Vec<usize> = ids[..n_edge_pts].iter().step_by(2).copied().collect();
        let ib: Vec<usize> = ids[..n_edge_pts]
            .iter()
            .skip(1)
            .step_by(2)
            .copied()
            .collect();
        new.add_edges_welded(&vobjs, &ia, &ib, Some(&flags));
    }
    let mut k = n_edge_pts;
    for (outer, holes, attrs) in rings {
        let o: Vec<_> = (0..outer.len()).map(|i| vobjs[ids[k + i]]).collect();
        k += outer.len();
        let mut hs = Vec::new();
        for h in &holes {
            hs.push((0..h.len()).map(|i| vobjs[ids[k + i]]).collect());
            k += h.len();
        }
        let nf = new.add_face_welded(o, hs);
        if !attrs.is_empty() {
            new.faces[nf].attrs.update(&attrs);
        }
    }
    new.resplit_curves();
    new
}

/// A world-space mesh for a group: the mesh itself for classic groups, a
/// transformed DEEP copy for instances; nested placements folded in.
/// The paint a placement's default faces show (`effective_material`): its
/// own, else the paint of the object that owns it (a nested piece of a
/// painted component). `None` for an unpainted one.
pub fn effective_material<'a>(
    own: Option<&'a Paint>,
    owner: Option<&'a Paint>,
) -> Option<&'a Paint> {
    match own {
        Some(p) if !p.is_empty() => Some(p),
        _ => owner.filter(|p| !p.is_empty()),
    }
}

pub fn world_mesh(meshes: &MeshTable, group: &Group) -> Mesh {
    if group.children.is_empty() {
        let Some(src) = meshes.get(&group.mesh) else {
            return Mesh::new();
        };
        return match &group.xform {
            None => (**src).clone(),
            Some(m) => transformed_mesh(src, m),
        };
    }
    merged_placements(meshes, group)
}

/// Copy a mesh into the table under a fresh id.
pub fn add_mesh(meshes: &mut MeshTable, mesh: Mesh) -> MeshId {
    let id = MeshId::fresh();
    meshes.insert(id, Arc::new(mesh));
    id
}

/// A pastable duplicate of `group`, optionally translated by `delta`.
///
/// A component instance stays an instance: the duplicate SHARES the
/// prototype mesh and gets its own transform. A classic group gets a deep
/// mesh copy. Everything inside a COMPONENT belongs to its definition, so
/// the groups nested in it are shared too, at every depth.
pub fn copy_group(
    meshes: &mut MeshTable,
    group: &mut Group,
    delta: Option<DVec3>,
    in_definition: bool,
) -> Group {
    let share = in_definition || (group.xform.is_some() && group.component);
    if share {
        for child in group.children.iter_mut() {
            if child.xform.is_none() {
                child.xform = Some(DMat4::IDENTITY);
                child.component = false;
            }
        }
    }
    let t = delta.map_or(DMat4::IDENTITY, DMat4::from_translation);
    let mut g;
    if let Some(x) = group.xform {
        g = Group::new(group.mesh, Some(&group.name));
        g.xform = Some(t * x);
        g.axes = group.axes;
    } else {
        let src = meshes.get(&group.mesh).cloned().unwrap_or_default();
        let id = add_mesh(meshes, transformed_mesh(&src, &t));
        g = Group::new(id, Some(&group.name));
        if let Some(a) = group.axes {
            g.axes = Some(t * a);
        }
    }
    g.layer = group.layer.clone();
    g.text3d = group.text3d.clone();
    g.ext = group.ext.clone();
    g.hidden = group.hidden;
    g.material = group.material.clone();
    g.component = group.component;
    g.exploded = group.exploded.clone();
    g.explode_offset = group.explode_offset;
    let mut kids = Vec::new();
    for c in group.children.iter_mut() {
        kids.push(copy_group(meshes, c, None, share));
    }
    g.children = kids;
    g
}

/// `copy_group` without the sharing: every mesh in the subtree is a copy of
/// its own. The copy keeps the uid and id — it REPLACES the original.
pub fn independent_copy(meshes: &mut MeshTable, group: &mut Group) -> Group {
    let mut g = copy_group(meshes, group, None, false);
    if group.xform.is_some() {
        let src = meshes.get(&group.mesh).cloned().unwrap_or_default();
        g.mesh = add_mesh(meshes, transformed_mesh(&src, &DMat4::IDENTITY));
    }
    g.uid = group.uid.clone();
    g.id = group.id;
    let mut kids = Vec::new();
    for c in group.children.iter_mut() {
        kids.push(independent_copy(meshes, c));
    }
    g.children = kids;
    g
}

/// Bake this instance into its OWN world-space mesh ('make independent').
pub fn materialize(meshes: &mut MeshTable, group: &mut Group) {
    if group.xform.is_none() && group.children.is_empty() {
        return;
    }
    let frame = group_frame(group);
    let wm = world_mesh(meshes, group);
    group.mesh = add_mesh(meshes, wm);
    group.xform = None;
    group.axes = frame;
    group.children.clear();
}

/// Make Independent: this instance stops sharing with its siblings.
pub fn make_independent(meshes: &mut MeshTable, group: &mut Group) {
    if group.children.is_empty() {
        materialize(meshes, group);
        return;
    }
    let src = meshes.get(&group.mesh).cloned().unwrap_or_default();
    group.mesh = add_mesh(meshes, transformed_mesh(&src, &DMat4::IDENTITY));
    let mut kids = std::mem::take(&mut group.children);
    group.children = kids
        .iter_mut()
        .map(|c| independent_copy(meshes, c))
        .collect();
    group.component = false;
}

/// Convex hull of 2D points (monotone chain), counter-clockwise.
pub fn hull_2d(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut p: Vec<(f64, f64)> = pts.to_vec();
    p.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let half = |seq: &mut dyn Iterator<Item = (f64, f64)>| -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for q in seq {
            while out.len() >= 2 {
                let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
                if (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0) > 0.0 {
                    break;
                }
                out.pop();
            }
            out.push(q);
        }
        out
    };
    let lower = half(&mut p.iter().copied());
    let upper = half(&mut p.iter().rev().copied());
    let mut out: Vec<(f64, f64)> = lower[..lower.len() - 1].to_vec();
    out.extend_from_slice(&upper[..upper.len() - 1]);
    out
}

/// An orthonormal frame for a group, derived from its vertices: the yaw
/// about Z whose footprint is tightest (minimum-area rectangle, exact over
/// the hull edges), with Z kept upright.
pub fn frame_from_points(positions: &[DVec3]) -> (DVec3, DVec3, DVec3) {
    let world = (DVec3::X, DVec3::Y, DVec3::Z);
    if positions.len() < 3 {
        return world;
    }
    let pts: Vec<(f64, f64)> = positions.iter().map(|p| (p.x, p.y)).collect();
    let hull = hull_2d(&pts);
    if hull.len() < 3 {
        return world;
    }
    let mut best: Option<(f64, (f64, f64))> = None;
    for i in 0..hull.len() {
        let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
        let d = (b.0 - a.0, b.1 - a.1);
        let length = d.0.hypot(d.1);
        if length < 1e-12 {
            continue;
        }
        let u = (d.0 / length, d.1 / length);
        let v = (-u.1, u.0);
        let (mut amin, mut amax, mut bmin, mut bmax) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for q in &hull {
            let pa = q.0 * u.0 + q.1 * u.1;
            let pb = q.0 * v.0 + q.1 * v.1;
            amin = amin.min(pa);
            amax = amax.max(pa);
            bmin = bmin.min(pb);
            bmax = bmax.max(pb);
        }
        let area = (amax - amin) * (bmax - bmin);
        if best.is_none_or(|(ba, _)| area < ba) {
            best = Some((area, u));
        }
    }
    let Some((_, u)) = best else { return world };
    (
        DVec3::new(u.0, u.1, 0.0),
        DVec3::new(-u.1, u.0, 0.0),
        DVec3::Z,
    )
}

/// Every vertex position of `group` and its nested placements, in WORLD.
pub fn placement_points(meshes: &MeshTable, group: &Group) -> Vec<DVec3> {
    let mut out = Vec::new();
    for (g, m) in iter_placements(group, None) {
        let Some(mesh) = meshes.get(&g.mesh) else {
            continue;
        };
        for (_, v) in mesh.verts.iter() {
            out.push(m.map_or(v.position, |mm| mm.project_point(v.position)));
        }
    }
    out
}

/// `(frame, lo, hi)` — the points' extent along `frame`'s axes.
pub fn oriented_bounds(
    points: &[DVec3],
    frame: Option<(DVec3, DVec3, DVec3)>,
) -> ((DVec3, DVec3, DVec3), DVec3, DVec3) {
    let frame = frame.unwrap_or((DVec3::X, DVec3::Y, DVec3::Z));
    if points.is_empty() {
        return (frame, DVec3::ZERO, DVec3::ZERO);
    }
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in points {
        let q = DVec3::new(p.dot(frame.0), p.dot(frame.1), p.dot(frame.2));
        lo = lo.min(q);
        hi = hi.max(q);
    }
    (frame, lo, hi)
}

/// The eight corners of an oriented box, in world coordinates.
pub fn oriented_box_corners(frame: (DVec3, DVec3, DVec3), lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    let (u, v, w) = frame;
    (0..8)
        .map(|i| {
            u * if i & 1 != 0 { hi.x } else { lo.x }
                + v * if i & 2 != 0 { hi.y } else { lo.y }
                + w * if i & 4 != 0 { hi.z } else { lo.z }
        })
        .collect()
}
