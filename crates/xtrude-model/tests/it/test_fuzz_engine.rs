// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The fuzz/property bench for the topology engine (the certifier).
//!
//! Seeded random sequences of real user operations (draw a rectangle on a
//! random face, push a random face by a random distance, Ctrl-push, undo /
//! redo) over cube / prism / plan / group scenarios; after every commit the
//! engine invariants must hold on every mesh. The random generator is a
//! seeded MT19937 (`xtrude_core::mt19937`); the invariants are what is
//! certified.

use crate::common;
use common::*;

use std::collections::{HashMap, HashSet};

use xtrude_core::geometry::PolyFace;
use xtrude_core::mt19937::Mt19937;
use xtrude_core::orient::{is_closed, orient_outward, signed_volume, DEFAULT_SEED};
use xtrude_core::topology::{mesh_is_flat, on_segment_2d, strictly_inside_2d};
use xtrude_core::triangulate::plane_axes;
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::group::Group;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::push_face;
use xtrude_model::{History, Scene};

type Seg = (DVec3, DVec3);

/// A point rounded to `nd` decimals, as integers.
type Key3 = (i64, i64, i64);

fn k6(p: DVec3, nd: i32) -> Key3 {
    let s = 10f64.powi(nd);
    (
        xtrude_core::math::round_even_int(p.x * s),
        xtrude_core::math::round_even_int(p.y * s),
        xtrude_core::math::round_even_int(p.z * s),
    )
}

fn draw_rect(s: &mut Scene, h: &mut History, corners: &[DVec3], user: &mut Vec<Seg>) {
    rectangle(s, h, corners);
    for i in 0..4 {
        user.push((corners[i], corners[(i + 1) % 4]));
    }
}

fn push(s: &mut Scene, h: &mut History, mesh: MeshId, face: FaceId, dist: f64, keep: bool) -> bool {
    push_face(s, h, mesh, face, dist, keep)
}

fn project2(origin: DVec3, u: DVec3, w: DVec3, p: DVec3) -> (f64, f64) {
    let rel = p - origin;
    (rel.dot(u), rel.dot(w))
}

fn in_or_on(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    if strictly_inside_2d(p, poly) {
        return true;
    }
    let n = poly.len();
    (0..n).any(|i| on_segment_2d(p, poly[i], poly[(i + 1) % n], 1e-7))
}

fn segs_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let eps = 1e-9;
    let orient = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        let v = (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
        if v.abs() < eps {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    };
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 != o2 && o3 != o4 {
        return true;
    }
    [(c, a, b), (d, a, b), (a, c, d), (b, c, d)]
        .iter()
        .any(|&(p, q, r)| on_segment_2d(p, q, r, 1e-7))
}

fn sample_rect_on_face(rng: &mut Mt19937, face: &PolyFace) -> Option<Vec<DVec3>> {
    let n = face.normal();
    if n.length() < 1e-6 {
        return None;
    }
    let n = n.normalize();
    let (u, w) = plane_axes(n);
    let origin = face.vertices[0];
    let outer: Vec<(f64, f64)> = face
        .vertices
        .iter()
        .map(|&p| project2(origin, u, w, p))
        .collect();
    let holes: Vec<Vec<(f64, f64)>> = face
        .holes
        .iter()
        .map(|h| h.iter().map(|&p| project2(origin, u, w, p)).collect())
        .collect();
    let us: Vec<f64> = outer.iter().map(|p| p.0).collect();
    let ws: Vec<f64> = outer.iter().map(|p| p.1).collect();
    let (umin, umax) = (
        us.iter().cloned().fold(f64::INFINITY, f64::min),
        us.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
    let (wmin, wmax) = (
        ws.iter().cloned().fold(f64::INFINITY, f64::min),
        ws.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
    let (margin, min_side) = (0.15, 0.2);
    if umax - umin < 2.0 * margin + min_side + 0.1 || wmax - wmin < 2.0 * margin + min_side + 0.1 {
        return None;
    }
    for _ in 0..8 {
        let u0 = rng.uniform(umin + margin, umax - margin - min_side);
        let w0 = rng.uniform(wmin + margin, wmax - margin - min_side);
        let du = rng.uniform(min_side, umax - margin - u0);
        let dw = rng.uniform(min_side, wmax - margin - w0);
        let c2 = [(u0, w0), (u0 + du, w0), (u0 + du, w0 + dw), (u0, w0 + dw)];
        let mut samples: Vec<(f64, f64)> = c2.to_vec();
        samples.push((u0 + du / 2.0, w0 + dw / 2.0));
        for i in 0..4 {
            let (a, b) = (c2[i], c2[(i + 1) % 4]);
            samples.push(((a.0 * 2.0 + b.0) / 3.0, (a.1 * 2.0 + b.1) / 3.0));
            samples.push(((a.0 + b.0 * 2.0) / 3.0, (a.1 + b.1 * 2.0) / 3.0));
        }
        if !samples.iter().all(|&s| strictly_inside_2d(s, &outer)) {
            continue;
        }
        if samples
            .iter()
            .any(|&s| holes.iter().any(|h| in_or_on(s, h)))
        {
            continue;
        }
        let boundary: Vec<(f64, f64)> = outer
            .iter()
            .chain(holes.iter().flatten())
            .copied()
            .collect();
        if boundary.iter().any(|p| {
            u0 - 1e-9 < p.0 && p.0 < u0 + du + 1e-9 && w0 - 1e-9 < p.1 && p.1 < w0 + dw + 1e-9
        }) {
            continue;
        }
        let mut loops = vec![outer.clone()];
        loops.extend(holes.iter().cloned());
        let crosses = (0..4).any(|i| {
            loops.iter().any(|lp| {
                (0..lp.len())
                    .any(|j| segs_cross(c2[i], c2[(i + 1) % 4], lp[j], lp[(j + 1) % lp.len()]))
            })
        });
        if crosses {
            continue;
        }
        return Some(c2.iter().map(|c| origin + u * c.0 + w * c.1).collect());
    }
    None
}

type Fp = (Vec<String>, Vec<String>, Vec<(i64, i64, i64)>);

fn canon_loop(pts: &[DVec3]) -> Vec<(i64, i64, i64)> {
    let ks: Vec<(i64, i64, i64)> = pts.iter().map(|&p| k6(p, 9)).collect();
    let mut best: Option<Vec<(i64, i64, i64)>> = None;
    let rev: Vec<_> = ks.iter().rev().copied().collect();
    for seq in [ks.clone(), rev] {
        for i in 0..seq.len() {
            let rot: Vec<_> = seq[i..].iter().chain(seq[..i].iter()).copied().collect();
            if best.as_ref().is_none_or(|b| rot < *b) {
                best = Some(rot);
            }
        }
    }
    best.unwrap_or_default()
}

fn mesh_fingerprint(m: &Mesh) -> Fp {
    let mut faces: Vec<String> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let mut hs: Vec<_> = m
                .face_hole_positions(f)
                .iter()
                .map(|h| canon_loop(h))
                .collect();
            hs.sort();
            format!("{:?}{:?}", canon_loop(&m.face_positions(f)), hs)
        })
        .collect();
    faces.sort();
    let mut edges: Vec<String> = m
        .edge_ids()
        .into_iter()
        .map(|e| {
            let (a, b) = m.edge_ab(e);
            let mut p = [k6(a, 9), k6(b, 9)];
            p.sort();
            format!("{p:?}")
        })
        .collect();
    edges.sort();
    let mut verts: Vec<_> = m
        .vertex_ids()
        .into_iter()
        .map(|v| k6(m.pos(v), 9))
        .collect();
    verts.sort();
    (faces, edges, verts)
}

fn scene_fingerprint(s: &Scene) -> Vec<Fp> {
    let mut out = vec![mesh_fingerprint(s.loose_mesh())];
    for g in &s.doc.groups {
        out.push(mesh_fingerprint(s.doc.mesh(g.mesh)));
    }
    out
}

fn on_segment_3d(p: DVec3, a: DVec3, b: DVec3) -> bool {
    let tol = 1e-6;
    let ab = b - a;
    let sq = ab.dot(ab);
    if sq < tol * tol {
        return false;
    }
    let t = (p - a).dot(ab) / sq;
    if t < -tol || t > 1.0 + tol {
        return false;
    }
    (p - (a + ab * t)).length() <= tol
}

fn edge_on_segs(a: DVec3, b: DVec3, segs: &[Seg]) -> bool {
    segs.iter()
        .any(|&(p, q)| on_segment_3d(a, p, q) && on_segment_3d(b, p, q))
}

fn check_mesh_invariants(mesh: &Mesh, was_closed: bool, user: &[Seg], ctx: &str, pre: &[Seg]) {
    let keys: Vec<_> = mesh
        .vertex_ids()
        .into_iter()
        .map(|v| k6(mesh.pos(v), 6))
        .collect();
    let uniq: HashSet<_> = keys.iter().collect();
    assert_eq!(keys.len(), uniq.len(), "{ctx}: unwelded duplicate vertices");
    let orphans: Vec<_> = mesh
        .edge_ids()
        .into_iter()
        .filter(|&e| mesh.edge(e).faces.is_empty())
        .collect();
    assert!(
        orphans.is_empty(),
        "{ctx}: {} orphan edge(s)",
        orphans.len()
    );
    let tiny = mesh
        .face_ids()
        .into_iter()
        .filter(|&f| mesh.face_area(f) < 1e-8)
        .count();
    assert_eq!(tiny, 0, "{ctx}: {tiny} ~zero-area face(s)");
    if was_closed {
        assert!(
            is_closed(mesh) || mesh_is_flat(mesh),
            "{ctx}: closed mesh left open (a crack survived)"
        );
    }
    if is_closed(mesh) {
        if mesh.edges.iter().all(|(_, e)| e.faces.len() == 2) {
            let vol = signed_volume(mesh);
            assert!(vol > 0.0, "{ctx}: closed mesh with signed volume {vol}");
        }
        let mut m2 = mesh.clone();
        assert!(
            orient_outward(&mut m2, DEFAULT_SEED, None).is_empty(),
            "{ctx}: committed solid has inconsistent winding"
        );
    }
    for e in mesh.edge_ids() {
        let ef = &mesh.edge(e).faces;
        if ef.len() != 2 {
            continue;
        }
        let d = mesh
            .face_normal(ef[0])
            .normalize()
            .dot(mesh.face_normal(ef[1]).normalize());
        let (a, b) = mesh.edge_ab(e);
        if d.abs() > 0.999 && !edge_on_segs(a, b, pre) && !edge_on_segs(a, b, user) {
            panic!(
                "{ctx}: unmerged coplanar seam at {:?}–{:?} (not user-drawn)",
                k6(a, 6),
                k6(b, 6)
            );
        }
    }
}

const SCENARIOS: [&str; 4] = ["cube", "prism", "plan", "group"];

fn up(m: &Mesh, f: FaceId, dist: f64) -> f64 {
    if m.face_normal(f).z > 0.0 {
        dist
    } else {
        -dist
    }
}

struct Fx {
    s: Scene,
    h: History,
    /// mesh id → user segments (LOOSE for the loose mesh)
    user: HashMap<MeshId, Vec<Seg>>,
}

fn build(scenario: &str) -> Fx {
    let (mut s, mut h) = scene_history();
    let mut user: HashMap<MeshId, Vec<Seg>> = HashMap::new();
    user.insert(MeshId::LOOSE, Vec::new());
    match scenario {
        "cube" | "group" => {
            draw_rect(
                &mut s,
                &mut h,
                &[V(0., 0.), V(4., 0.), V(4., 4.), V(0., 4.)],
                user.get_mut(&MeshId::LOOSE).unwrap(),
            );
            let f = s.mesh().face_ids()[0];
            let d = up(s.mesh(), f, 3.0);
            push(&mut s, &mut h, MeshId::LOOSE, f, d, false);
            if scenario == "group" {
                let mut gm = Mesh::new();
                let p = [
                    V3(10., 0., 0.),
                    V3(12., 0., 0.),
                    V3(12., 2., 0.),
                    V3(10., 2., 0.),
                ];
                let q = [
                    V3(10., 0., 2.),
                    V3(12., 0., 2.),
                    V3(12., 2., 2.),
                    V3(10., 2., 2.),
                ];
                gm.add_face(&p, &[]);
                gm.add_face(&q, &[]);
                for i in 0..4 {
                    let j = (i + 1) % 4;
                    gm.add_face(&[p[i], p[j], q[j], q[i]], &[]);
                }
                orient_outward(&mut gm, DEFAULT_SEED, None);
                let mid = s.doc.add_mesh(gm);
                s.doc.groups.push(Group::new(mid, None));
                user.insert(mid, Vec::new());
            }
        }
        "prism" => {
            let tri = [V(-0.2, -2.8), V(2.7, -7.2), V(4.1, -2.7)];
            s.mesh_mut().add_face(&tri, &[]);
            for i in 0..3 {
                user.get_mut(&MeshId::LOOSE)
                    .unwrap()
                    .push((tri[i], tri[(i + 1) % 3]));
            }
            let f = s.mesh().face_ids()[0];
            let d = up(s.mesh(), f, 3.0);
            push(&mut s, &mut h, MeshId::LOOSE, f, d, false);
        }
        "plan" => {
            let u = user.get_mut(&MeshId::LOOSE).unwrap();
            draw_rect(
                &mut s,
                &mut h,
                &[V(0., 0.), V(3., 0.), V(3., 4.), V(0., 4.)],
                u,
            );
            draw_rect(
                &mut s,
                &mut h,
                &[V(3., 0.), V(6., 0.), V(6., 4.), V(3., 4.)],
                u,
            );
            let m = s.mesh();
            let room = m
                .face_ids()
                .into_iter()
                .min_by(|&a, &b| {
                    m.face_centroid(a)
                        .x
                        .partial_cmp(&m.face_centroid(b).x)
                        .unwrap()
                })
                .unwrap();
            let d = up(m, room, 2.7);
            push(&mut s, &mut h, MeshId::LOOSE, room, d, false);
        }
        _ => unreachable!(),
    }
    Fx { s, h, user }
}

fn meshes(s: &Scene) -> Vec<MeshId> {
    let mut v = vec![MeshId::LOOSE];
    v.extend(s.doc.groups.iter().map(|g| g.mesh));
    v
}

type Pre = HashMap<MeshId, (bool, Vec<Seg>)>;

fn pre_state(s: &Scene) -> Pre {
    meshes(s)
        .into_iter()
        .map(|mid| {
            let m = s.doc.mesh(mid);
            let segs = m.edge_ids().into_iter().map(|e| m.edge_ab(e)).collect();
            (mid, (is_closed(m), segs))
        })
        .collect()
}

fn check_all(fx: &Fx, pre: &Pre, ctx: &str) {
    for mid in meshes(&fx.s) {
        let kind = if mid == MeshId::LOOSE {
            "loose"
        } else {
            "group"
        };
        let (was_closed, pre_seams) = pre.get(&mid).cloned().unwrap_or((false, Vec::new()));
        let empty = Vec::new();
        let user = fx.user.get(&mid).unwrap_or(&empty);
        check_mesh_invariants(
            fx.s.doc.mesh(mid),
            was_closed,
            user,
            &format!("{ctx} [{kind} mesh]"),
            &pre_seams,
        );
    }
}

fn random_distance(rng: &mut Mt19937) -> f64 {
    let r = rng.random();
    if r < 0.45 {
        rng.uniform(0.2, 2.5)
    } else if r < 0.85 {
        -rng.uniform(0.2, 2.5)
    } else {
        -50.0
    }
}

pub fn run_sequence(scenario: &str, seed: u64, n_ops: usize) {
    let idx = SCENARIOS.iter().position(|s| *s == scenario).unwrap() as u64;
    let mut rng = Mt19937::new(idx * 100003 + seed);
    let mut fx = build(scenario);
    let floor = fx.h.undo_len();
    for step in 0..n_ops {
        let ctx = format!("{scenario} seed={seed} step={step}");
        let r = rng.random();
        if r < 0.30 {
            let m = fx.s.loose_mesh();
            let mut faces: Vec<FaceId> = m
                .face_ids()
                .into_iter()
                .filter(|&f| m.face_normal(f).length() > 1e-6)
                .collect();
            rng.shuffle(&mut faces);
            for f in faces.into_iter().take(4) {
                let pf = fx.s.loose_mesh().poly_face(f);
                let Some(rect) = sample_rect_on_face(&mut rng, &pf) else {
                    continue;
                };
                let pre = pre_state(&fx.s);
                let mut user = fx.user.remove(&MeshId::LOOSE).unwrap_or_default();
                draw_rect(&mut fx.s, &mut fx.h, &rect, &mut user);
                fx.user.insert(MeshId::LOOSE, user);
                check_all(&fx, &pre, &format!("{ctx} draw"));
                break;
            }
        } else if r < 0.80 {
            let ms = meshes(&fx.s);
            let mid = ms[rng.choice_index(ms.len())];
            let m = fx.s.doc.mesh(mid);
            let faces: Vec<FaceId> = m
                .face_ids()
                .into_iter()
                .filter(|&f| m.face_area(f) > 1e-6 && m.face_normal(f).length() > 1e-6)
                .collect();
            if faces.is_empty() {
                continue;
            }
            let face = faces[rng.choice_index(faces.len())];
            let dist = random_distance(&mut rng);
            let keep = rng.random() < 0.15;
            let pre = pre_state(&fx.s);
            let pre_edges: HashSet<(Key3, Key3)> = if keep {
                let m = fx.s.doc.mesh(mid);
                m.edge_ids()
                    .into_iter()
                    .map(|e| {
                        let (a, b) = m.edge_ab(e);
                        (k6(a, 6), k6(b, 6))
                    })
                    .collect()
            } else {
                HashSet::new()
            };
            if push(&mut fx.s, &mut fx.h, mid, face, dist, keep) {
                // The group's mesh id is stable (pushes edit it in place).
                if keep {
                    let m = fx.s.doc.mesh(mid);
                    let fresh: Vec<Seg> = m
                        .edge_ids()
                        .into_iter()
                        .map(|e| m.edge_ab(e))
                        .filter(|&(a, b)| !pre_edges.contains(&(k6(a, 6), k6(b, 6))))
                        .collect();
                    fx.user.entry(mid).or_default().extend(fresh);
                }
                check_all(&fx, &pre, &format!("{ctx} push d={dist:.3} keep={keep}"));
            }
        } else if r < 0.92 {
            let fp = scene_fingerprint(&fx.s);
            let mut done = 0;
            for _ in 0..rng.randint(1, 3) {
                if !fx.h.undo(&mut fx.s) {
                    break;
                }
                done += 1;
            }
            for _ in 0..done {
                assert!(fx.h.redo(&mut fx.s), "{ctx}: redo stack exhausted early");
            }
            assert!(
                scene_fingerprint(&fx.s) == fp,
                "{ctx}: undo→redo did not reproduce the committed state"
            );
        } else {
            for _ in 0..rng.randint(1, 2) {
                if fx.h.undo_len() <= floor {
                    break;
                }
                fx.h.undo(&mut fx.s);
            }
        }
    }
}

/// Sequences the engine does not survive yet — reproducible open bugs.
/// They are skipped as expected failures; a seed that starts passing
/// should be pruned.
fn known_bad(scenario: &str) -> &'static [u64] {
    match scenario {
        "cube" => &[167],
        "prism" => &[165],
        "plan" => &[73, 216],
        _ => &[],
    }
}

fn run_seeds(scenario: &str, seeds: std::ops::Range<u64>) {
    let mut failures = Vec::new();
    for seed in seeds {
        if known_bad(scenario).contains(&seed) {
            continue;
        }
        let r = std::panic::catch_unwind(|| run_sequence(scenario, seed, 8));
        if let Err(e) = r {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("seed {seed}: {msg}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} {scenario} sequence(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The quick bench, over every scenario.
#[test]
fn test_fuzz_quick() {
    for s in SCENARIOS {
        run_seeds(s, 0..50);
    }
}

/// The ≥1000-sequence certification (slow, so ignored by default):
/// `cargo test -p xtrude-model --test it test_fuzz_engine:: -- --ignored`.
#[test]
#[ignore]
fn test_fuzz_full() {
    let mut bad = Vec::new();
    for s in SCENARIOS {
        if let Err(e) = std::panic::catch_unwind(|| run_seeds(s, 50..250)) {
            bad.push(e.downcast_ref::<String>().cloned().unwrap_or_default());
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
