// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A bordered face is consumed by the push (recess / pocket), a free-standing
//! one keeps its base; the hybrid drag preview leaves the mesh alone.

use crate::common;

use std::collections::HashSet;

use common::{draw_rect, stub, v3};
use glam::DVec3;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_core::orient::signed_volume;
use xtrude_core::topology::{face_is_bordered, loop_edges, same_position, tkey};
use xtrude_core::{FaceId, Mesh};
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::ids::MeshId;

fn v(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

fn big() -> Vec<DVec3> {
    vec![v(0., 0.), v(4., 0.), v(4., 4.), v(0., 4.)]
}

fn small() -> Vec<DVec3> {
    vec![v(1., 1.), v(3., 1.), v(3., 3.), v(1., 3.)]
}

/// Big face with a small face drawn inside it (so big is holed).
fn embedded() -> (Viewport, FaceId, FaceId) {
    let mut vp = stub();
    vp.run(AddFaceCommand::new(big()));
    vp.run(AddFaceCommand::new(small()));
    let ids = vp.scene.mesh().face_ids();
    (vp, ids[0], ids[1])
}

/// The bare push: base, distance, dragging — no classification.
fn push(vp: &mut Viewport, face: FaceId, dist: f64) {
    let mut t = PushPullTool::new();
    t.pp.base_face = Some(face);
    t.pp.target = Some(MeshId::LOOSE);
    t.pp.extrusion = dist;
    t.commit(vp);
}

/// Classified like `on_click`; returns whether it was a prism.
fn push_real(vp: &mut Viewport, face: FaceId, dist: f64) -> bool {
    let mut t = PushPullTool::new();
    {
        let m = vp.scene.mesh();
        t.pp.base_face = Some(face);
        t.pp.target = Some(MeshId::LOOSE);
        t.pp.anchor = Some(m.face_centroid(face));
        t.pp.normal = Some(m.face_normal(face));
        let (a, p) = t.pp.classify_base(m);
        t.pp.attached = a;
        t.pp.prism_cap = p;
        t.pp.cap_positions = xtrude_model::pushpull::cap_loop_positions(m, face);
    }
    let prism = t.pp.prism_cap;
    t.pp.extrusion = dist;
    t.commit(vp);
    prism
}

fn all_z(vp: &Viewport, f: FaceId, z: f64) -> bool {
    vp.scene
        .mesh()
        .face_positions(f)
        .iter()
        .all(|p| (p.z - z).abs() < 1e-9)
}

fn has_face_at_z(vp: &Viewport, z: f64, n: usize) -> bool {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .any(|f| m.face(f).outer.len() == n && all_z(vp, f, z))
}

fn rect(vp: &mut Viewport, ring: &[DVec3]) {
    draw_rect(vp, ring);
}

fn cube(vp: &mut Viewport, size: f64, h: f64) {
    rect(vp, &[v(0., 0.), v(size, 0.), v(size, size), v(0., size)]);
    let f = vp.scene.mesh().face_ids()[0];
    push(vp, f, h);
}

fn find(vp: &Viewport, pred: impl Fn(&[DVec3]) -> bool) -> FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| pred(&m.face_positions(f)))
        .expect("face")
}

fn holes(vp: &Viewport, f: FaceId) -> usize {
    vp.scene.mesh().face(f).holes.len()
}

#[test]
fn test_inward_push_consumes_base_face() {
    let (mut vp, b, s) = embedded();
    push(&mut vp, s, -1.0);
    let m = vp.scene.mesh();
    assert!(!m.has_face(s));
    assert!(m.has_face(b));
    assert_eq!(holes(&vp, b), 1);
}

#[test]
fn test_inward_push_builds_floor_and_walls() {
    let (mut vp, _b, s) = embedded();
    push(&mut vp, s, -1.0);
    assert!(has_face_at_z(&vp, -1.0, 4));
    let m = vp.scene.mesh();
    let walls = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            let ps = m.face_positions(f);
            ps.len() == 4 && ps.iter().any(|p| p.z > -1e-9) && ps.iter().any(|p| p.z < -1e-9)
        })
        .count();
    assert_eq!(walls, 4);
    assert_eq!(m.faces.len(), 6);
}

#[test]
fn test_inward_push_undo_restores_base() {
    let (mut vp, b, s) = embedded();
    push(&mut vp, s, -1.0);
    assert!(vp.undo());
    let m = vp.scene.mesh();
    assert!(m.has_face(s));
    let mut sizes: Vec<usize> = m
        .face_ids()
        .into_iter()
        .map(|f| m.face(f).outer.len())
        .collect();
    sizes.sort();
    assert_eq!(sizes, vec![4, 4]);
    assert_eq!(holes(&vp, b), 1);
}

#[test]
fn test_outward_push_embedded_also_consumes_base() {
    let (mut vp, b, s) = embedded();
    push(&mut vp, s, 1.0);
    assert!(!vp.scene.mesh().has_face(s));
    assert_eq!(holes(&vp, b), 1);
}

#[test]
fn test_standalone_face_keeps_base() {
    let mut vp = stub();
    vp.run(AddFaceCommand::new(vec![
        v(0., 0.),
        v(2., 0.),
        v(2., 2.),
        v(0., 2.),
    ]));
    let sq = vp.scene.mesh().face_ids()[0];
    push(&mut vp, sq, -1.0);
    assert!(vp.scene.mesh().has_face(sq));
    assert!(has_face_at_z(&vp, -1.0, 4));
    assert_eq!(vp.scene.mesh().faces.len(), 6);
}

#[test]
fn test_face_is_bordered_detects_embedded() {
    let (vp, b, s) = embedded();
    let m = vp.scene.mesh();
    let faces = m.face_ids();
    assert!(face_is_bordered(m, s, &faces));
    assert!(!face_is_bordered(m, b, &faces));
}

#[test]
fn test_face_is_bordered_standalone_false() {
    let mut m = Mesh::new();
    let f = m.add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    assert!(!face_is_bordered(&m, f, &[f]));
}

#[test]
fn test_recess_on_real_cube_end_to_end() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 2.0);
    let top = find(&vp, |ps| {
        ps.len() == 4 && ps.iter().all(|p| (p.z - 2.0).abs() < 1e-9)
    });
    rect(
        &mut vp,
        &[
            v3(1., 1., 2.),
            v3(3., 1., 2.),
            v3(3., 3., 2.),
            v3(1., 3., 2.),
        ],
    );
    let m = vp.scene.mesh();
    let s = m
        .face_ids()
        .into_iter()
        .find(|&f| f != top && m.face(f).outer.len() == 4 && all_z(&vp, f, 2.0))
        .unwrap();
    assert_eq!(holes(&vp, top), 1);
    push(&mut vp, s, -1.0);
    assert!(!vp.scene.mesh().has_face(s));
    assert_eq!(holes(&vp, top), 1);
    assert!(has_face_at_z(&vp, 1.0, 4));
}

#[test]
fn test_repush_top_extends_walls_no_seam() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 2.0);
    let top = find(&vp, |ps| {
        ps.len() == 4 && ps.iter().all(|p| (p.z - 2.0).abs() < 1e-9)
    });
    push(&mut vp, top, 2.0);
    let m = vp.scene.mesh();
    assert_eq!(m.faces.len(), 6);
    assert!(m.face_ids().into_iter().all(|f| m.face(f).outer.len() == 4));
    assert_eq!(m.edges.len(), 12);
    assert!(!m.edge_ids().into_iter().any(|e| {
        let (a, b) = m.edge_ab(e);
        (a.z - 2.0).abs() < 1e-9 || (b.z - 2.0).abs() < 1e-9
    }));
    assert!(has_face_at_z(&vp, 4.0, 4));
    assert!(has_face_at_z(&vp, 0.0, 4));
    assert!(vp.undo());
    assert!(has_face_at_z(&vp, 2.0, 4));
}

#[test]
fn test_repush_top_extends_holed_wall_keeping_opening() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    let front = find(&vp, |ps| {
        ps.len() == 4 && ps.iter().all(|p| p.y.abs() < 1e-9)
    });
    rect(
        &mut vp,
        &[
            v3(1., 0., 1.),
            v3(3., 0., 1.),
            v3(3., 0., 2.),
            v3(1., 0., 2.),
        ],
    );
    assert_eq!(holes(&vp, front), 1);
    let m = vp.scene.mesh();
    let win = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            f != front
                && m.face(f).outer.len() == 4
                && m.face_positions(f).iter().all(|p| p.y.abs() < 1e-9)
        })
        .unwrap();
    push(&mut vp, win, 0.5);
    assert_eq!(holes(&vp, front), 1);
    let top = find(&vp, |ps| {
        ps.len() == 4 && ps.iter().all(|p| (p.z - 3.0).abs() < 1e-9)
    });
    push(&mut vp, top, 2.0);
    let m = vp.scene.mesh();
    let fronts: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.y.abs() < 1e-9))
        .collect();
    assert_eq!(fronts.len(), 1);
    assert_eq!(m.face(fronts[0]).holes.len(), 1);
    assert_eq!(
        m.face_positions(fronts[0])
            .iter()
            .map(|p| p.z)
            .fold(f64::MIN, f64::max),
        5.0
    );
    let seam = m
        .edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            a.y.abs() < 1e-9
                && b.y.abs() < 1e-9
                && (a.z - 3.0).abs() < 1e-9
                && (b.z - 3.0).abs() < 1e-9
        })
        .count();
    assert_eq!(seam, 0);
}

fn corner_scene() -> (Viewport, FaceId) {
    let mut vp = stub();
    cube(&mut vp, 10.0, 3.0);
    rect(
        &mut vp,
        &[
            v3(0., 0., 3.),
            v3(4., 0., 3.),
            v3(4., 4., 3.),
            v3(0., 4., 3.),
        ],
    );
    let c = find(&vp, |ps| {
        ps.len() == 4
            && ps.iter().all(|p| (p.z - 3.0).abs() < 1e-9)
            && ps.iter().map(|p| p.x).fold(f64::MIN, f64::max) <= 4.001
            && ps.iter().map(|p| p.y).fold(f64::MIN, f64::max) <= 4.001
    });
    (vp, c)
}

#[test]
fn test_corner_step_notches_adjacent_walls() {
    let (mut vp, corner) = corner_scene();
    push(&mut vp, corner, -1.5);
    assert!(!vp.scene.mesh().has_face(corner));
    assert!(has_face_at_z(&vp, 1.5, 4));
    let plane = |vp: &Viewport, axis: usize| -> Vec<FaceId> {
        let m = vp.scene.mesh();
        m.face_ids()
            .into_iter()
            .filter(|&f| m.face_positions(f).iter().all(|p| p[axis].abs() < 1e-9))
            .collect()
    };
    let m = vp.scene.mesh();
    assert!(plane(&vp, 1).iter().any(|&f| m.face(f).outer.len() == 6));
    assert!(plane(&vp, 0).iter().any(|&f| m.face(f).outer.len() == 6));
    assert!(vp.undo());
    assert!(vp.scene.mesh().has_face(corner));
    let front = plane(&vp, 1);
    let m = vp.scene.mesh();
    assert!(!front.is_empty() && !front.iter().any(|&f| m.face(f).outer.len() == 6));
    let area: f64 = front.iter().map(|&f| m.face_area(f)).sum();
    assert!((area - 30.0).abs() < 1e-3);
}

fn orphan_edges(vp: &Viewport) -> usize {
    let m = vp.scene.mesh();
    let mut face_edges = HashSet::new();
    for f in m.face_ids() {
        face_edges.extend(loop_edges(&m.face_positions(f)));
        for h in m.face_hole_positions(f) {
            face_edges.extend(loop_edges(&h));
        }
    }
    m.edge_ids()
        .into_iter()
        .filter(|&e| {
            let (a, b) = m.edge_ab(e);
            let (ka, kb) = (tkey(a), tkey(b));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            !face_edges.contains(&k)
        })
        .count()
}

#[test]
fn test_corner_step_leaves_no_orphan_edges() {
    let (mut vp, corner) = corner_scene();
    push(&mut vp, corner, -1.5);
    assert_eq!(orphan_edges(&vp), 0);
}

#[test]
fn test_corner_step_splits_corner_vertical_not_deletes_it() {
    let (mut vp, corner) = corner_scene();
    push(&mut vp, corner, -1.5);
    let m = vp.scene.mesh();
    let has_edge = |a: DVec3, b: DVec3| {
        m.edge_ids().into_iter().any(|e| {
            let (p, q) = m.edge_ab(e);
            (same_position(p, a) && same_position(q, b))
                || (same_position(p, b) && same_position(q, a))
        })
    };
    assert!(has_edge(v3(0., 0., 0.), v3(0., 0., 1.5)));
    assert!(!has_edge(v3(0., 0., 0.), v3(0., 0., 3.)));
}

#[test]
fn test_stacked_block_side_pushes_stay_clean() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    rect(
        &mut vp,
        &[
            v3(1., 1., 3.),
            v3(3., 1., 3.),
            v3(3., 3., 3.),
            v3(1., 3., 3.),
        ],
    );
    let inner = find(&vp, |ps| {
        ps.len() == 4
            && ps.iter().all(|p| (p.z - 3.0).abs() < 1e-9)
            && ps.iter().map(|p| p.x).fold(f64::MIN, f64::max) <= 3.001
            && ps.iter().map(|p| p.x).fold(f64::MAX, f64::min) >= 0.999
    });
    push(&mut vp, inner, 2.0);
    let wall = |vp: &Viewport, axis: usize, val: f64| {
        find(vp, |ps| {
            ps.iter().all(|p| (p[axis] - val).abs() < 1e-9) && ps.iter().any(|p| p.z > 3.5)
        })
    };
    let w = wall(&vp, 0, 1.0);
    assert!(
        push_real(&mut vp, w, 1.0),
        "wall on a host hole misread as a free extrusion"
    );
    let w = wall(&vp, 1, 1.0);
    assert!(push_real(&mut vp, w, 1.0));
    assert_eq!(orphan_edges(&vp), 0);
}

#[test]
fn test_push_through_wall_makes_a_real_hole() {
    let mut vp = stub();
    rect(
        &mut vp,
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 0.3, 0.),
            v3(0., 0.3, 0.),
        ],
    );
    let f = vp.scene.mesh().face_ids()[0];
    push(&mut vp, f, 3.0);
    rect(
        &mut vp,
        &[
            v3(1., 0., 1.),
            v3(3., 0., 1.),
            v3(3., 0., 2.),
            v3(1., 0., 2.),
        ],
    );
    let win = find(&vp, |ps| {
        ps.len() == 4
            && ps.iter().all(|p| p.y.abs() < 1e-9)
            && ps.iter().map(|p| p.x).fold(f64::MIN, f64::max) <= 3.001
            && ps.iter().map(|p| p.x).fold(f64::MAX, f64::min) >= 0.999
    });
    push_real(&mut vp, win, -0.4);
    let m = vp.scene.mesh();
    let big_at = |y: f64| -> Vec<FaceId> {
        m.face_ids()
            .into_iter()
            .filter(|&f| {
                let ps = m.face_positions(f);
                let zs: HashSet<i64> = ps.iter().map(|p| p.z.round() as i64).collect();
                ps.len() == 4
                    && ps.iter().all(|p| (p.y - y).abs() < 1e-4)
                    && zs == [0, 3].into_iter().collect()
            })
            .collect()
    };
    let front = big_at(0.0);
    let back = big_at(0.3);
    assert!(front.len() == 1 && m.face(front[0]).holes.len() == 1);
    assert!(back.len() == 1 && m.face(back[0]).holes.len() == 1);
    assert!(!m
        .face_ids()
        .into_iter()
        .any(|f| m.face_positions(f).iter().all(|p| 0.01 < p.y && p.y < 0.29)));
    assert!(!m.face_ids().into_iter().any(|f| {
        let ps = m.face_positions(f);
        let zs: HashSet<i64> = ps.iter().map(|p| p.z.round() as i64).collect();
        ps.iter().all(|p| p.y.abs() < 1e-9) && zs == [1, 2].into_iter().collect()
    }));
}

#[test]
fn test_bump_pushed_flush_back_dissolves_to_clean_cube() {
    let mut vp = stub();
    cube(&mut vp, 4.0, 3.0);
    rect(
        &mut vp,
        &[
            v3(1., 0., 1.),
            v3(2., 0., 1.),
            v3(2., 0., 2.),
            v3(1., 0., 2.),
        ],
    );
    let base = find(&vp, |ps| {
        ps.len() == 4
            && ps.iter().all(|p| p.y.abs() < 1e-9)
            && ps.iter().map(|p| p.x).fold(f64::MIN, f64::max) <= 2.001
            && ps.iter().map(|p| p.x).fold(f64::MAX, f64::min) >= 0.999
    });
    assert!(!push_real(&mut vp, base, 1.0));
    let front = find(&vp, |ps| ps.iter().all(|p| (p.y + 1.0).abs() < 1e-6));
    assert!(push_real(&mut vp, front, -1.0));
    let m = vp.scene.mesh();
    assert_eq!((m.faces.len(), m.edges.len(), m.verts.len()), (6, 12, 8));
    assert!(m.edge_ids().into_iter().all(|e| m.edge(e).faces.len() == 2));
    let fronts: Vec<_> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.y.abs() < 1e-9))
        .collect();
    assert!(fronts.len() == 1 && m.face(fronts[0]).holes.is_empty());
    assert!(signed_volume(m) > 0.0);
}

// ---- hybrid drag preview ------------------------------------------------------------

fn dragging_tool(vp: &Viewport, face: FaceId, d: f64) -> PushPullTool {
    let mut t = PushPullTool::new();
    let m = vp.scene.mesh();
    t.pp.base_face = Some(face);
    t.pp.target = Some(MeshId::LOOSE);
    t.pp.extrusion = d;
    t.pp.anchor = Some(m.face_centroid(face));
    t.pp.normal = Some(m.face_normal(face));
    t
}

#[test]
fn test_light_preview_sweeps_the_base_face() {
    let (vp, _b, s) = embedded();
    let t = dragging_tool(&vp, s, 2.0);
    let m = vp.scene.mesh();
    let (faces, _rings) = t.pp.light_faces(m);
    assert_eq!(faces.len(), 1 + m.face(s).outer.len());
    let n = m.face_normal(s);
    for (p, w) in m.face_positions(s).iter().zip(&faces[0].vertices) {
        assert!((*w - (*p + n * 2.0)).length() < 1e-9);
    }
    assert!(faces[1..].iter().all(|q| q.vertices.len() == 4));
}

#[test]
fn test_light_preview_carries_the_holes() {
    let (vp, b, _s) = embedded();
    assert_eq!(holes(&vp, b), 1);
    let t = dragging_tool(&vp, b, 1.0);
    let m = vp.scene.mesh();
    let (faces, _r) = t.pp.light_faces(m);
    assert_eq!(faces[0].holes.len(), 1);
    assert_eq!(
        faces.len(),
        1 + m.face(b).outer.len() + m.face(b).holes[0].len()
    );
}

#[test]
fn test_light_preview_is_empty_below_the_minimum() {
    let (vp, _b, s) = embedded();
    assert!(dragging_tool(&vp, s, 0.0)
        .pp
        .light_faces(vp.scene.mesh())
        .0
        .is_empty());
}

#[test]
fn test_dragging_leaves_the_mesh_untouched() {
    let (mut vp, _b, s) = embedded();
    let mut t = dragging_tool(&vp, s, -1.0);
    let counts = |vp: &Viewport| (vp.scene.mesh().faces.len(), vp.scene.mesh().edges.len());
    let before = counts(&vp);
    for d in [-0.2, -0.5, -1.0] {
        t.pp.extrusion = d;
        t.show_light_preview(&mut vp);
        assert_eq!(counts(&vp), before);
        assert!(!t.preview_faces().is_empty());
    }
    t.commit(&mut vp);
    assert!(t.preview_faces().is_empty());
    assert_ne!(counts(&vp), before);
}

#[test]
fn test_inward_drag_hides_the_face_it_consumes() {
    let (mut vp, _b, s) = embedded();
    let mut t = dragging_tool(&vp, s, -1.0);
    t.pp.attached = true;
    t.show_light_preview(&mut vp);
    assert_eq!(
        *vp.suppressed_faces(),
        [(MeshId::LOOSE, s)].into_iter().collect()
    );
    t.pp.extrusion = 1.0;
    t.show_light_preview(&mut vp);
    assert!(vp.suppressed_faces().is_empty());
}
