// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Dissolve the seam a flush push/pull leaves behind — an edge bordering
//! exactly two coplanar faces goes and the faces merge; stitches resolve
//! T-junctions in the operation's own neighbourhood only.

use crate::common;
use common::*;
use xtrude_core::topology::{
    classify_push_edge, refine_loop_with_points, PushEdgeKind, SPLIT_TOLERANCE,
};
use xtrude_core::{DVec3, EdgeId, FaceId, Mesh};
use xtrude_model::commands::{CoplanarMergeCommand, EraseSelectionCommand, StitchSolidCommand};
use xtrude_model::edits::build_add_edges;
use xtrude_model::history::History;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::push_face;
use xtrude_model::scene::Scene;

fn shared(m: &Mesh, a: DVec3, b: DVec3) -> Option<EdgeId> {
    m.find_edge(m.vertex_at(a)?, m.vertex_at(b)?)
}

fn key3(p: DVec3) -> (i64, i64, i64) {
    (
        (p.x * 1e3).round() as i64,
        (p.y * 1e3).round() as i64,
        (p.z * 1e3).round() as i64,
    )
}

fn no_coplanar_seam(m: &Mesh) {
    for e in m.edge_ids() {
        let fs = &m.edge(e).faces;
        if fs.len() == 2 {
            let d = m.face_normal(fs[0]).dot(m.face_normal(fs[1]));
            assert!(d <= 0.999, "coplanar seam left at {:?}", m.edge_ab(e));
        }
    }
}

// ---- dissolve_edge (mesh level) --------------------------------------------

#[test]
fn test_two_coplanar_squares_merge_into_one_face() {
    let mut m = Mesh::new();
    m.add_face(&[V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)], &[]);
    m.add_face(&[V(1., 0.), V(2., 0.), V(2., 1.), V(1., 1.)], &[]);
    let seam = shared(&m, V(1., 0.), V(1., 1.)).unwrap();
    assert_eq!(m.edge(seam).faces.len(), 2);
    let merged = m.dissolve_edge(seam).expect("merged");
    assert_eq!(m.faces.len(), 1);
    assert!((m.face_area(merged) - 2.0).abs() < 1e-9); // the full 2×1 rectangle
    assert!(shared(&m, V(1., 0.), V(1., 1.)).is_none()); // seam gone
}

#[test]
fn test_perpendicular_faces_do_not_merge() {
    let mut m = Mesh::new();
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(1., 0., 0.),
            V3(1., 1., 0.),
            V3(0., 1., 0.),
        ],
        &[],
    );
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(1., 0., 0.),
            V3(1., 0., 1.),
            V3(0., 0., 1.),
        ],
        &[],
    );
    let seam = shared(&m, V3(0., 0., 0.), V3(1., 0., 0.)).unwrap();
    assert!(m.dissolve_edge(seam).is_none());
    assert_eq!(m.faces.len(), 2);
}

#[test]
fn test_non_manifold_edge_is_not_dissolved() {
    let mut m = Mesh::new();
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(1., 0., 0.),
            V3(1., 1., 0.),
            V3(0., 1., 0.),
        ],
        &[],
    );
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(1., 0., 0.),
            V3(1., 0., 1.),
            V3(0., 0., 1.),
        ],
        &[],
    );
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(1., 0., 0.),
            V3(1., 0., -1.),
            V3(0., 0., -1.),
        ],
        &[],
    );
    let seam = shared(&m, V3(0., 0., 0.), V3(1., 0., 0.)).unwrap();
    assert_eq!(m.edge(seam).faces.len(), 3);
    assert!(m.dissolve_edge(seam).is_none());
    assert_eq!(m.faces.len(), 3);
}

// ---- CoplanarMergeCommand ----------------------------------------------------

fn two_squares() -> (Scene, History) {
    let (mut s, h) = scene_history();
    s.mesh_mut()
        .add_face(&[V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)], &[]);
    s.mesh_mut()
        .add_face(&[V(1., 0.), V(2., 0.), V(2., 1.), V(1., 1.)], &[]);
    (s, h)
}

#[test]
fn test_command_merges_then_undo_restores() {
    let (mut s, mut h) = two_squares();
    h.execute(
        &mut s,
        &mut CoplanarMergeCommand {
            seed: vec![V(1., 0.), V(1., 1.)],
        },
    );
    assert_eq!(s.mesh().faces.len(), 1);
    assert!(shared(s.mesh(), V(1., 0.), V(1., 1.)).is_none());
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().faces.len(), 2);
    assert!(shared(s.mesh(), V(1., 0.), V(1., 1.)).is_some());
}

#[test]
fn test_command_leaves_unseeded_seam_alone() {
    let (mut s, mut h) = two_squares();
    h.execute(
        &mut s,
        &mut CoplanarMergeCommand {
            seed: vec![V(9., 9.)],
        },
    ); // far away
    assert_eq!(s.mesh().faces.len(), 2);
    assert!(shared(s.mesh(), V(1., 0.), V(1., 1.)).is_some());
}

// ---- integration: a partial flush wall pushed up → a clean L ----------------

fn cube_with_top_strip() -> (Scene, History, FaceId) {
    let (mut s, mut h) = scene_history();
    let m = s.mesh_mut();
    let p = V3;
    m.add_face(
        &[p(0., 0., 0.), p(2., 0., 0.), p(2., 2., 0.), p(0., 2., 0.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 2.), p(2., 0., 2.), p(2., 2., 2.), p(0., 2., 2.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 0.), p(2., 0., 0.), p(2., 0., 2.), p(0., 0., 2.)],
        &[],
    );
    m.add_face(
        &[p(2., 0., 0.), p(2., 2., 0.), p(2., 2., 2.), p(2., 0., 2.)],
        &[],
    );
    m.add_face(
        &[p(0., 2., 0.), p(2., 2., 0.), p(2., 2., 2.), p(0., 2., 2.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 0.), p(0., 2., 0.), p(0., 2., 2.), p(0., 0., 2.)],
        &[],
    );
    let c = [p(0., 0., 2.), p(1., 0., 2.), p(1., 2., 2.), p(0., 2., 2.)];
    let segs: Vec<(DVec3, DVec3)> = (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect();
    let cmd = build_add_edges(&s, &segs, true, vec![], None);
    exec(&mut s, &mut h, cmd);
    let want: std::collections::BTreeSet<_> = c.iter().map(|q| key3(*q)).collect();
    let m = s.mesh();
    let rect = m
        .face_ids()
        .into_iter()
        .find(|&f| {
            m.face_positions(f)
                .iter()
                .map(|q| key3(*q))
                .collect::<std::collections::BTreeSet<_>>()
                == want
        })
        .expect("the strip");
    (s, h, rect)
}

fn push(s: &mut Scene, h: &mut History, f: FaceId, d: f64) -> bool {
    push_face(s, h, MeshId::LOOSE, f, d, false)
}

#[test]
fn test_partial_flush_push_makes_a_step_not_a_slant() {
    let (mut s, mut h, rect) = cube_with_top_strip();
    let d = up(&s, rect, 1.0);
    assert!(push(&mut s, &mut h, rect, d));
    let m = s.mesh();
    let front = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| p.y.abs() < 1e-6) && m.face_area(f) > 1.0)
        .expect("the front wall");
    let keys: Vec<_> = m.face_positions(front).iter().map(|p| key3(*p)).collect();
    assert!(keys.contains(&(1000, 0, 2000))); // the step's inner corner kept
    assert!(keys.contains(&(1000, 0, 3000))); // raised over the strip only
    no_coplanar_seam(m);
}

#[test]
fn test_partial_flush_push_undo_restores() {
    let (mut s, mut h, rect) = cube_with_top_strip();
    let before = s.mesh().faces.len();
    let d = up(&s, rect, 1.0);
    assert!(push(&mut s, &mut h, rect, d));
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().faces.len(), before);
}

#[test]
fn test_overhang_push_consumes_base_despite_unsplit_host_wall() {
    let (x0, x1, xc) = (0.0, 4.0, 2.0);
    let (y0, y1, yc) = (0.0, 6.0, 1.5);
    let (zt, zc) = (3.0, 5.0);
    let (mut s, mut h) = scene_history();
    let p = V3;
    let m = s.mesh_mut();
    m.add_face(
        &[p(x0, y0, 0.), p(x1, y0, 0.), p(x1, y1, 0.), p(x0, y1, 0.)],
        &[],
    );
    m.add_face(
        &[p(x0, y0, 0.), p(x1, y0, 0.), p(x1, y0, zt), p(x0, y0, zt)],
        &[],
    );
    m.add_face(
        &[p(x1, y0, 0.), p(x1, y1, 0.), p(x1, y1, zt), p(x1, y0, zt)],
        &[],
    );
    m.add_face(
        &[p(x0, y1, 0.), p(x1, y1, 0.), p(x1, y1, zt), p(x0, y1, zt)],
        &[],
    );
    m.add_face(
        &[p(x0, y0, 0.), p(x0, y1, 0.), p(x0, y1, zt), p(x0, y0, zt)],
        &[],
    );
    m.add_face(
        &[
            p(xc, y0, zt),
            p(x1, y0, zt),
            p(x1, y1, zt),
            p(x0, y1, zt),
            p(x0, yc, zt),
            p(xc, yc, zt),
        ],
        &[],
    );
    m.add_face(
        &[p(x0, y0, zc), p(xc, y0, zc), p(xc, yc, zc), p(x0, yc, zc)],
        &[],
    );
    let fw = m.add_face(
        &[p(x0, y0, zt), p(xc, y0, zt), p(xc, y0, zc), p(x0, y0, zc)],
        &[],
    );
    m.add_face(
        &[p(xc, y0, zt), p(xc, yc, zt), p(xc, yc, zc), p(xc, y0, zc)],
        &[],
    );
    m.add_face(
        &[p(x0, yc, zt), p(xc, yc, zt), p(xc, yc, zc), p(x0, yc, zc)],
        &[],
    );
    m.add_face(
        &[p(x0, y0, zt), p(x0, yc, zt), p(x0, yc, zc), p(x0, y0, zc)],
        &[],
    );
    let all = m.face_ids();
    let (kind, _) = classify_push_edge(m, fw, p(x0, y0, zt), p(xc, y0, zt), &all);
    assert_eq!(kind, PushEdgeKind::Coplanar); // a sub-segment of the unsplit front
    assert!(push(&mut s, &mut h, fw, -2.0)); // the front wall outward (overhang)
    let m = s.mesh();
    let partition = m.face_ids().into_iter().any(|f| {
        let ps = m.face_positions(f);
        let ys: std::collections::BTreeSet<i64> =
            ps.iter().map(|q| (q.y * 100.).round() as i64).collect();
        let zs: std::collections::BTreeSet<i64> =
            ps.iter().map(|q| (q.z * 100.).round() as i64).collect();
        ys == [0].into_iter().collect() && zs == [300, 500].into_iter().collect()
    });
    assert!(
        !partition,
        "an internal partition wall survived the overhang push"
    );
    no_coplanar_seam(m);
}

#[test]
fn test_stitch_resolves_t_junction_then_merges_coplanar() {
    let (mut s, mut h) = scene_history();
    let m = s.mesh_mut();
    m.add_face(&[V(0., 0.), V(2., 0.), V(2., 2.), V(0., 2.)], &[]);
    m.add_face(&[V(2., 0.), V(4., 0.), V(4., 1.), V(2., 1.)], &[]);
    m.add_face(&[V(2., 1.), V(4., 1.), V(4., 2.), V(2., 2.)], &[]);
    let seam = shared(m, V(2., 0.), V(2., 2.)).unwrap();
    assert_eq!(m.edge(seam).faces.len(), 1); // naked (a T-junction)
    let seed: Vec<DVec3> = m.verts.iter().map(|(_, v)| v.position).collect();
    h.execute(&mut s, &mut StitchSolidCommand { seed });
    assert_eq!(s.mesh().faces.len(), 1); // all three fused
    assert!(shared(s.mesh(), V(2., 0.), V(2., 2.)).is_none());
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().faces.len(), 3);
    assert!(shared(s.mesh(), V(2., 0.), V(2., 2.)).is_some());
}

#[test]
fn test_refine_loop_inserts_collinear_t_junction() {
    let lp = [
        V3(0., -2., 3.),
        V3(0., 2., 3.),
        V3(0., 2., 5.),
        V3(0., -2., 5.),
    ];
    let refined = refine_loop_with_points(&lp, &[V3(0., 0., 3.), V3(9., 9., 9.)], SPLIT_TOLERANCE);
    let keys: Vec<_> = refined.iter().map(|p| key3(*p)).collect();
    assert!(keys.contains(&(0, 0, 3000)));
    assert!(!keys.contains(&(9000, 9000, 9000)));
    assert_eq!(keys.iter().position(|k| *k == (0, 0, 3000)), Some(1));
}

#[test]
fn test_push_wall_whose_base_edge_spans_a_t_junction() {
    let (mut s, mut h) = scene_history();
    let p = V3;
    let m = s.mesh_mut();
    m.add_face(
        &[p(0., 0., 0.), p(4., 0., 0.), p(4., 6., 0.), p(0., 6., 0.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 0.), p(4., 0., 0.), p(4., 0., 3.), p(0., 0., 3.)],
        &[],
    );
    m.add_face(
        &[p(4., 0., 0.), p(4., 6., 0.), p(4., 6., 3.), p(4., 0., 3.)],
        &[],
    );
    m.add_face(
        &[p(0., 6., 0.), p(4., 6., 0.), p(4., 6., 3.), p(0., 6., 3.)],
        &[],
    );
    m.add_face(
        &[p(0., 0., 0.), p(0., 6., 0.), p(0., 6., 3.), p(0., 0., 3.)],
        &[],
    );
    m.add_face(
        &[
            p(2., 0., 3.),
            p(4., 0., 3.),
            p(4., 6., 3.),
            p(0., 6., 3.),
            p(0., 2., 3.),
            p(2., 2., 3.),
        ],
        &[],
    );
    m.add_face(
        &[p(0., -2., 5.), p(2., -2., 5.), p(2., 2., 5.), p(0., 2., 5.)],
        &[],
    );
    m.add_face(
        &[
            p(0., -2., 3.),
            p(2., -2., 3.),
            p(2., -2., 5.),
            p(0., -2., 5.),
        ],
        &[],
    );
    m.add_face(
        &[p(2., -2., 3.), p(2., 2., 3.), p(2., 2., 5.), p(2., -2., 5.)],
        &[],
    );
    m.add_face(
        &[p(0., 2., 3.), p(2., 2., 3.), p(2., 2., 5.), p(0., 2., 5.)],
        &[],
    );
    let left = m.add_face(
        &[p(0., -2., 3.), p(0., 2., 3.), p(0., 2., 5.), p(0., -2., 5.)],
        &[],
    );
    m.add_face(
        &[p(0., -2., 3.), p(2., -2., 3.), p(2., 0., 3.), p(0., 0., 3.)],
        &[],
    );
    assert!(push(&mut s, &mut h, left, -2.0)); // the left wall outward
    let m = s.mesh();
    let partition = m.face_ids().into_iter().any(|f| {
        let ps = m.face_positions(f);
        let xs: std::collections::BTreeSet<i64> =
            ps.iter().map(|q| (q.x * 100.).round() as i64).collect();
        let zs: std::collections::BTreeSet<i64> =
            ps.iter().map(|q| (q.z * 100.).round() as i64).collect();
        m.face_normal(f).x.abs() > 0.9
            && xs == [0].into_iter().collect()
            && zs == [300, 500].into_iter().collect()
    });
    assert!(
        !partition,
        "an internal partition survived the T-junction overhang push"
    );
}

// ---- erase a coplanar divider → faces merge ---------------------------------

#[test]
fn test_erase_coplanar_divider_merges_faces() {
    let (mut s, mut h) = scene_history();
    let m = s.mesh_mut();
    m.add_face(&[V(0., 0.), V(2., 0.), V(2., 2.)], &[]);
    m.add_face(&[V(0., 0.), V(2., 2.), V(0., 2.)], &[]);
    let diag = shared(m, V(0., 0.), V(2., 2.)).unwrap();
    assert_eq!(m.edge(diag).faces.len(), 2);
    let cmd = EraseSelectionCommand::from_entities(s.mesh(), &[diag], &[]);
    exec(&mut s, &mut h, Box::new(cmd));
    let m = s.mesh();
    assert_eq!(m.faces.len(), 1); // reunited
    assert!(shared(m, V(0., 0.), V(2., 2.)).is_none());
    assert!((m.face_area(m.face_ids()[0]) - 4.0).abs() < 1e-9);
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().faces.len(), 2);
    assert!(shared(s.mesh(), V(0., 0.), V(2., 2.)).is_some());
}

#[test]
fn test_erase_non_coplanar_edge_cascades_faces() {
    let (mut s, mut h) = scene_history();
    let m = s.mesh_mut();
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(2., 0., 0.),
            V3(2., 2., 0.),
            V3(0., 2., 0.),
        ],
        &[],
    );
    m.add_face(
        &[
            V3(0., 0., 0.),
            V3(2., 0., 0.),
            V3(2., 0., 2.),
            V3(0., 0., 2.),
        ],
        &[],
    );
    let e = shared(m, V3(0., 0., 0.), V3(2., 0., 0.)).unwrap();
    let cmd = EraseSelectionCommand::from_entities(s.mesh(), &[e], &[]);
    exec(&mut s, &mut h, Box::new(cmd));
    assert_eq!(s.mesh().faces.len(), 0); // both removed
    assert!(shared(s.mesh(), V3(0., 0., 0.), V3(2., 0., 0.)).is_none());
    assert!(h.undo(&mut s));
    assert_eq!(s.mesh().faces.len(), 2);
}

#[test]
fn test_stitch_leaves_a_t_junction_outside_the_operation_alone() {
    let (mut s, mut h) = scene_history();
    let far = 50.0;
    let m = s.mesh_mut();
    for y in [0.0, far] {
        m.add_face(&[V(0., y), V(2., y), V(2., y + 2.), V(0., y + 2.)], &[]);
        m.add_face(&[V(2., y), V(4., y), V(4., y + 1.), V(2., y + 1.)], &[]);
        m.add_face(
            &[V(2., y + 1.), V(4., y + 1.), V(4., y + 2.), V(2., y + 2.)],
            &[],
        );
    }
    let near = shared(m, V(2., 0.), V(2., 2.)).unwrap();
    assert_eq!(m.edge(near).faces.len(), 1);
    let fe = shared(m, V(2., far), V(2., far + 2.)).unwrap();
    assert_eq!(m.edge(fe).faces.len(), 1);
    let seed: Vec<DVec3> = m
        .verts
        .iter()
        .map(|(_, v)| v.position)
        .filter(|p| p.y < far / 2.0)
        .collect();
    h.execute(&mut s, &mut StitchSolidCommand { seed });
    assert!(shared(s.mesh(), V(2., 0.), V(2., 2.)).is_none()); // near: resolved
    let fe = shared(s.mesh(), V(2., far), V(2., far + 2.)).expect("far seam");
    assert_eq!(s.mesh().edge(fe).faces.len(), 1); // far: untouched
}
