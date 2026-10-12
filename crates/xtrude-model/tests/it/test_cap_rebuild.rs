// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The deterministic plane rebuild ("path C") rejects
//! phantom regions outside the solid, survives concave footprints, keeps
//! holes, unions the inside regions, is a no-op on a stable plane and
//! independent of plane order.

use xtrude_core::cap_rebuild::{apply_rebuild, rebuild_plane, RebuildOpts, RebuiltFace};
use xtrude_core::math::VecExt;
use xtrude_core::orient::{orient_outward, DEFAULT_SEED};
use xtrude_core::topology::{
    classify_push_edge, refine_loop_with_points, PushEdgeKind, SPLIT_TOLERANCE,
};
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::{extrude_ops, set_plane_order, PushPull};
use xtrude_model::scene::Scene;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn loop_area(lp: &[DVec3]) -> f64 {
    let mut n = DVec3::ZERO;
    for i in 0..lp.len() {
        let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
        n += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    0.5 * n.length()
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Sorted net areas (outer minus holes) of a rebuild result.
fn areas(faces: &[RebuiltFace]) -> Vec<f64> {
    let mut out: Vec<f64> = faces
        .iter()
        .map(|(o, holes, _)| r2(loop_area(o) - holes.iter().map(|h| loop_area(h)).sum::<f64>()))
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

fn rebuild(m: &Mesh, o: DVec3, n: DVec3) -> Vec<RebuiltFace> {
    rebuild_plane(m, o, n, &[], RebuildOpts::default(), None, None).unwrap_or_default()
}

// ---- prism cap with an extruded footprint (a closed solid) ----------------------

const TRI: [(f64, f64); 3] = [(-0.2, -2.8), (2.7, -7.2), (4.1, -2.7)];

fn prism_of(pts: &[(f64, f64)], h: f64) -> Mesh {
    let mut m = Mesh::new();
    let b: Vec<DVec3> = pts.iter().map(|&(x, y)| v(x, y, 0.)).collect();
    let t: Vec<DVec3> = pts.iter().map(|&(x, y)| v(x, y, h)).collect();
    m.add_face(&b, &[]);
    m.add_face(&t, &[]);
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        m.add_face(&[b[i], b[j], t[j], t[i]], &[]);
    }
    orient_outward(&mut m, DEFAULT_SEED, None);
    m
}

#[test]
fn test_plain_prism_cap_is_one_face() {
    let m = prism_of(&TRI, 3.0);
    let faces = rebuild(&m, v(0., 0., 3.), DVec3::Z);
    assert_eq!(faces.len(), 1);
    let top: Vec<DVec3> = TRI.iter().map(|&(x, y)| v(x, y, 3.)).collect();
    assert_eq!(areas(&faces), vec![r2(loop_area(&top))]);
}

// ---- concave L footprint with a phantom region in the notch -------------------

#[test]
fn test_concave_l_cap_rejects_notch_phantom() {
    let mut m = prism_of(
        &[(0., 0.), (4., 0.), (4., 2.), (2., 2.), (2., 4.), (0., 4.)],
        3.0,
    );
    // Stray edges closing the notch: a phantom region OUTSIDE the L.
    m.add_edge(v(2., 4., 3.), v(4., 4., 3.)).unwrap();
    m.add_edge(v(4., 4., 3.), v(4., 2., 3.)).unwrap();
    let faces = rebuild(&m, v(0., 0., 3.), DVec3::Z);
    assert_eq!(areas(&faces), vec![12.0]); // the L, not the notch
}

// ---- a hole in the cap survives --------------------------------------------------

#[test]
fn test_cap_with_hole_keeps_the_hole() {
    let mut m = Mesh::new();
    let outer = [(0., 0.), (6., 0.), (6., 6.), (0., 6.)];
    let hole = [(2., 2.), (4., 2.), (4., 4.), (2., 4.)];
    let at = |pts: &[(f64, f64)], z: f64| -> Vec<DVec3> {
        pts.iter().map(|&(x, y)| v(x, y, z)).collect()
    };
    let (ob, ot, hb, ht) = (at(&outer, 0.), at(&outer, 3.), at(&hole, 0.), at(&hole, 3.));
    m.add_face(&ob, std::slice::from_ref(&hb));
    m.add_face(&ot, std::slice::from_ref(&ht));
    for i in 0..4 {
        let j = (i + 1) % 4;
        m.add_face(&[ob[i], ob[j], ot[j], ot[i]], &[]);
        m.add_face(&[hb[i], hb[j], ht[j], ht[i]], &[]);
    }
    orient_outward(&mut m, DEFAULT_SEED, None);
    let faces = rebuild(&m, v(0., 0., 3.), DVec3::Z);
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].1.len(), 1);
    assert_eq!(areas(&faces), vec![32.0]); // 36 − 4
}

// ---- end to end: a real pushed prism cap matches what the engine commits --------

fn push(sc: &mut Scene, face: FaceId, dist: f64) {
    let mut pp = PushPull::new();
    pp.setup_plain(sc.mesh(), MeshId::LOOSE, face);
    pp.limit_in = None;
    pp.extrusion = dist;
    pp.mutate(sc);
}

fn fresh_prism_scene() -> Scene {
    let mut sc = Scene::new();
    let f = sc.mesh_mut().add_face(&TRI.map(|(x, y)| v(x, y, 0.)), &[]);
    push(&mut sc, f, 3.0);
    sc
}

fn walls(sc: &Scene) -> Vec<FaceId> {
    let m = sc.mesh();
    let mut w: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_normal(f).z.abs() < 0.1)
        .collect();
    let key = |f: &FaceId| {
        let c = m.face_centroid(*f);
        ((c.x * 1000.0).round() as i64, (c.y * 1000.0).round() as i64)
    };
    w.sort_by_key(key);
    w
}

const DISTS: [f64; 3] = [1.5, 1.0, 2.0];

fn cap_origin(normal: DVec3) -> DVec3 {
    if normal.z > 0.0 {
        v(0., 0., 3.)
    } else {
        DVec3::ZERO
    }
}

fn ground_truth_cap_area(perm: [usize; 3], normal: DVec3) -> f64 {
    let mut sc = fresh_prism_scene();
    for (k, &wi) in perm.iter().enumerate() {
        let f = walls(&sc)[wi];
        push(&mut sc, f, DISTS[k]);
    }
    let o = cap_origin(normal);
    let m = sc.mesh();
    m.face_ids()
        .into_iter()
        .filter(|&f| {
            m.face_normal(f).unit_or_zero().dot(normal).abs() > 0.999
                && (m.face_centroid(f) - o).dot(normal).abs() < 1e-3
        })
        .map(|f| r2(m.face_area(f)))
        .sum()
}

const PERMS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

#[test]
fn test_pushed_prism_cap_rebuild_matches_committed_face() {
    // For every push order and both caps: rebuilding the plane at the messy
    // mid-operation state reproduces the area the engine commits.
    for normal in [DVec3::Z, -DVec3::Z] {
        for perm in PERMS {
            let gt_total = ground_truth_cap_area(perm, normal);
            let mut sc = fresh_prism_scene();
            let f0 = walls(&sc)[perm[0]];
            push(&mut sc, f0, DISTS[0]);
            let f1 = walls(&sc)[perm[1]];
            push(&mut sc, f1, DISTS[1]);
            // Only the third push's naive extrusion runs.
            let face = walls(&sc)[perm[2]];
            let d = DISTS[2];
            let m = sc.mesh_mut();
            let fnorm = m.face_normal(face);
            let allv: Vec<DVec3> = m.verts.iter().map(|(_, x)| x.position).collect();
            let base_pos = m.face_positions(face);
            let base = refine_loop_with_points(&base_pos, &allv, SPLIT_TOLERANCE);
            let top: Vec<DVec3> = base.iter().map(|&p| p + fnorm * d).collect();
            let n = base.len();
            let ids = m.face_ids();
            let attached = (0..n).all(|i| {
                classify_push_edge(m, face, base[i], base[(i + 1) % n], &ids).0
                    != PushEdgeKind::Free
            });
            extrude_ops(m, &base_pos, &base, &top, &[], &[], attached);
            let faces = rebuild(sc.mesh(), cap_origin(normal), normal);
            let total: f64 = areas(&faces).iter().sum();
            assert_eq!(r2(total), r2(gt_total), "{perm:?} {normal}");
        }
    }
}

// ---- fixpoint / order independence ----------------------------------------------

#[test]
fn test_apply_rebuild_noop_on_stable_plane() {
    let mut sc = fresh_prism_scene();
    let wall = walls(&sc)[0];
    let m = sc.mesh_mut();
    let n = m.face_normal(wall).unit_or_zero();
    let c = m.face_centroid(wall);
    let before = m.face_ids().len();
    assert!(!apply_rebuild(
        m,
        c,
        n,
        &[],
        RebuildOpts::default(),
        None,
        None
    ));
    assert_eq!(m.face_ids().len(), before);
}

/// The overhang push (a cube flush on an unsplit host).
fn overhang() -> (Scene, FaceId) {
    let (x0, x1, xc) = (0.0, 4.0, 2.0);
    let (y0, y1, yc) = (0.0, 6.0, 1.5);
    let (zt, zc) = (3.0, 5.0);
    let mut sc = Scene::new();
    let m = sc.mesh_mut();
    m.add_face(
        &[v(x0, y0, 0.), v(x1, y0, 0.), v(x1, y1, 0.), v(x0, y1, 0.)],
        &[],
    );
    m.add_face(
        &[v(x0, y0, 0.), v(x1, y0, 0.), v(x1, y0, zt), v(x0, y0, zt)],
        &[],
    );
    m.add_face(
        &[v(x1, y0, 0.), v(x1, y1, 0.), v(x1, y1, zt), v(x1, y0, zt)],
        &[],
    );
    m.add_face(
        &[v(x0, y1, 0.), v(x1, y1, 0.), v(x1, y1, zt), v(x0, y1, zt)],
        &[],
    );
    m.add_face(
        &[v(x0, y0, 0.), v(x0, y1, 0.), v(x0, y1, zt), v(x0, y0, zt)],
        &[],
    );
    m.add_face(
        &[
            v(xc, y0, zt),
            v(x1, y0, zt),
            v(x1, y1, zt),
            v(x0, y1, zt),
            v(x0, yc, zt),
            v(xc, yc, zt),
        ],
        &[],
    );
    m.add_face(
        &[v(x0, y0, zc), v(xc, y0, zc), v(xc, yc, zc), v(x0, yc, zc)],
        &[],
    );
    let fw = m.add_face(
        &[v(x0, y0, zt), v(xc, y0, zt), v(xc, y0, zc), v(x0, y0, zc)],
        &[],
    );
    m.add_face(
        &[v(xc, y0, zt), v(xc, yc, zt), v(xc, yc, zc), v(xc, y0, zc)],
        &[],
    );
    m.add_face(
        &[v(x0, yc, zt), v(xc, yc, zt), v(xc, yc, zc), v(x0, yc, zc)],
        &[],
    );
    m.add_face(
        &[v(x0, y0, zt), v(x0, yc, zt), v(x0, yc, zc), v(x0, y0, zc)],
        &[],
    );
    (sc, fw)
}

#[test]
fn test_rebuild_order_independent_on_overhang() {
    // The fixpoint sorts its planes, so the hook permutes that sorted list.
    for perm in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]] {
        set_plane_order(Some(perm.to_vec()));
        let (mut sc, fw) = overhang();
        push(&mut sc, fw, -2.0);
        set_plane_order(None);
        let m = sc.mesh();
        let seams = m
            .edges
            .iter()
            .filter(|(_, e)| {
                e.faces.len() == 2
                    && m.face_normal(e.faces[0])
                        .unit_or_zero()
                        .dot(m.face_normal(e.faces[1]).unit_or_zero())
                        > 0.999
            })
            .count();
        assert_eq!(seams, 0, "seam left with order {perm:?}");
    }
}
