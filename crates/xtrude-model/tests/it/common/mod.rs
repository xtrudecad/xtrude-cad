// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shared helpers for the document tests.
// V(…) and V3(…) read like the vectors they build.
#![allow(non_snake_case)]

use std::collections::HashSet;

use xtrude_core::mesh::Key;
use xtrude_core::topology::tkey;
use xtrude_core::DVec3;
use xtrude_model::{Command, History, Scene};

pub fn V(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

pub fn V3(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// A fresh scene + history, settings isolated for this thread.
pub fn scene_history() -> (Scene, History) {
    xtrude_model::settings::use_memory_store();
    (Scene::new(), History::new())
}

pub fn exec(scene: &mut Scene, hist: &mut History, mut cmd: Box<dyn Command>) -> bool {
    hist.execute(scene, cmd.as_mut())
}

pub type EdgeKey = (Key, Key);

pub fn ek(a: DVec3, b: DVec3) -> EdgeKey {
    let (ka, kb) = (tkey(a), tkey(b));
    if ka <= kb {
        (ka, kb)
    } else {
        (kb, ka)
    }
}

/// Each edge of the active mesh as an orientation-free key pair.
pub fn edge_keyset(scene: &Scene) -> HashSet<EdgeKey> {
    let m = scene.mesh();
    m.edge_ids()
        .into_iter()
        .map(|e| {
            let (a, b) = m.edge_ab(e);
            ek(a, b)
        })
        .collect()
}

pub fn keyset_from(segs: &[(DVec3, DVec3)]) -> HashSet<EdgeKey> {
    segs.iter().map(|&(a, b)| ek(a, b)).collect()
}

pub fn has_vertex(scene: &Scene, p: DVec3) -> bool {
    let m = scene.mesh();
    m.edge_ids().into_iter().any(|e| {
        let (a, b) = m.edge_ab(e);
        tkey(a) == tkey(p) || tkey(b) == tkey(p)
    })
}

pub fn nfaces(scene: &Scene) -> usize {
    scene.mesh().faces.len()
}

pub fn nedges(scene: &Scene) -> usize {
    scene.mesh().edges.len()
}

/// Sorted vertex counts of the active mesh's faces.
pub fn vcount(scene: &Scene) -> Vec<usize> {
    let m = scene.mesh();
    let mut v: Vec<usize> = m
        .face_ids()
        .iter()
        .map(|&f| m.face(f).outer.len())
        .collect();
    v.sort();
    v
}

pub fn tri_area(t: &[DVec3; 3]) -> f64 {
    (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5
}

pub fn face_area_tris(scene: &Scene, f: xtrude_core::FaceId) -> f64 {
    scene
        .mesh()
        .face_triangulate(f, None)
        .iter()
        .map(tri_area)
        .sum()
}

/// Draw a rectangle the way the Rectangle tool does: its four edges with
/// face detection off, plus its own face.
pub fn rectangle(scene: &mut Scene, hist: &mut History, corners: &[DVec3]) {
    let segs: Vec<(DVec3, DVec3)> = (0..corners.len())
        .map(|i| (corners[i], corners[(i + 1) % corners.len()]))
        .collect();
    let face: Box<dyn Command> = Box::new(xtrude_model::commands::AddFaceCommand::new(
        corners.to_vec(),
    ));
    let c = xtrude_model::edits::build_add_edges(scene, &segs, false, vec![face], None);
    exec(scene, hist, c);
}

pub fn key_set(pts: &[DVec3]) -> Vec<Key> {
    let mut k: Vec<Key> = pts.iter().map(|&p| tkey(p)).collect();
    k.sort();
    k.dedup();
    k
}

/// Push `face` of the active (loose) mesh by `dist`, as the fuzz bench does.
pub fn push(
    s: &mut Scene,
    h: &mut History,
    face: xtrude_core::FaceId,
    dist: f64,
    keep: bool,
) -> bool {
    let mid = s.active_mesh_id();
    xtrude_model::pushpull::push_face(s, h, mid, face, dist, keep)
}

/// Distance signed so the push goes along +Z whatever the drawn winding.
pub fn up(s: &Scene, f: xtrude_core::FaceId, dist: f64) -> f64 {
    if s.mesh().face_normal(f).z > 0.0 {
        dist
    } else {
        -dist
    }
}

/// A `size × size × height` box drawn and pushed like the user would.
pub fn cube_scene(size: f64, height: f64) -> (Scene, History) {
    let (mut s, mut h) = scene_history();
    rectangle(
        &mut s,
        &mut h,
        &[V(0., 0.), V(size, 0.), V(size, size), V(0., size)],
    );
    let f = s.mesh().face_ids()[0];
    let d = up(&s, f, height);
    push(&mut s, &mut h, f, d, false);
    (s, h)
}

/// The first face of the active mesh matching `pred(positions)`.
pub fn find_face(s: &Scene, pred: impl Fn(&[DVec3]) -> bool) -> Option<xtrude_core::FaceId> {
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| pred(&m.face_positions(f)))
}

pub fn centroid(pts: &[DVec3]) -> DVec3 {
    pts.iter().copied().sum::<DVec3>() / pts.len() as f64
}

pub fn all_near(pts: &[DVec3], f: impl Fn(&DVec3) -> f64, v: f64) -> bool {
    pts.iter().all(|p| (f(p) - v).abs() < 1e-6)
}

/// Lock the tool on `face` (no inward
/// clamp), commit with its own history; returns `(history, outcome)`.
/// `outcome != Committed` means the push reported a status.
pub fn push_ux(
    s: &mut Scene,
    face: xtrude_core::FaceId,
    dist: f64,
    keep: bool,
) -> (History, xtrude_model::pushpull::PushOutcome) {
    let mut h = History::new();
    let mid = s.active_mesh_id();
    let mut pp = xtrude_model::pushpull::PushPull::new();
    pp.setup_plain(s.doc.mesh(mid), mid, face);
    pp.limit_in = None;
    pp.keep_base = keep;
    pp.extrusion = dist;
    let out = pp.commit(s, &mut h);
    (h, out)
}

/// A `size` × `size` square pushed up by `height`.
pub fn ux_cube(s: &mut Scene, h: &mut History, size: f64, height: f64) {
    rectangle(s, h, &[V(0., 0.), V(size, 0.), V(size, size), V(0., size)]);
    let f = s.mesh().face_ids()[0];
    push_ux(s, f, height, false);
}

pub fn faces_where(s: &Scene, pred: impl Fn(&DVec3) -> bool) -> Vec<xtrude_core::FaceId> {
    let m = s.mesh();
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(&pred))
        .collect()
}
