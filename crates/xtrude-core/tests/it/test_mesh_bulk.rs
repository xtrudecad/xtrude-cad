// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The bulk mesh builders must produce exactly what the sequential add_face
//! path produces.

use xtrude_core::attrs::FaceAttrs;
use xtrude_core::mesh::EdgeFlags;
use xtrude_core::mt19937::Mt19937;
use xtrude_core::{DVec3, Mesh};

type FaceSpec = (Vec<DVec3>, Vec<Vec<DVec3>>, Option<FaceAttrs>);

fn gen_faces(n: usize, seed: u64) -> Vec<FaceSpec> {
    let mut rng = Mt19937::new(seed);
    let mut faces = Vec::new();
    for i in 0..n {
        let (x, y) = ((i % 10) as f64, (i / 10) as f64);
        let quad = [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];
        let quad: Vec<DVec3> = quad
            .iter()
            .map(|&(px, py)| {
                DVec3::new(
                    px + rng.uniform(-9e-5, 9e-5),
                    py + rng.uniform(-9e-5, 9e-5),
                    rng.uniform(-9e-5, 9e-5),
                )
            })
            .collect();
        let attrs = (i % 3 == 0).then(|| {
            let mut a = FaceAttrs::default();
            a.paint.color = Some([0.5, 0.2, 0.1]);
            a
        });
        faces.push((quad, vec![], attrs));
    }
    let mut mat = FaceAttrs::default();
    mat.paint.mat = Some("X".into());
    let p = |x: f64, y: f64| DVec3::new(x, y, 0.0);
    faces.push((
        vec![p(20., 0.), p(24., 0.), p(24., 4.), p(20., 4.)],
        vec![vec![p(21., 1.), p(22., 1.), p(22., 2.), p(21., 2.)]],
        Some(mat),
    ));
    faces.push((
        vec![p(30., 0.), p(30. + 1e-6, 0.), p(30., 1e-6)],
        vec![],
        None,
    ));
    faces
}

fn build_sequential(faces: &[FaceSpec]) -> Mesh {
    let mut m = Mesh::new();
    for (outer, holes, attrs) in faces {
        let f = m.add_face(outer, holes);
        if let Some(a) = attrs {
            m.faces[f].attrs.update(a);
        }
    }
    m
}

fn build_bulk(faces: &[FaceSpec]) -> Mesh {
    let mut m = Mesh::new();
    let mut flat = Vec::new();
    for (outer, holes, _) in faces {
        flat.extend(outer.iter().copied());
        for h in holes {
            flat.extend(h.iter().copied());
        }
    }
    let (vobjs, ids) = m.bulk_weld(&flat);
    let mut k = 0;
    for (outer, holes, attrs) in faces {
        let o: Vec<_> = (0..outer.len()).map(|i| vobjs[ids[k + i]]).collect();
        k += outer.len();
        let mut hs = Vec::new();
        for h in holes {
            hs.push((0..h.len()).map(|i| vobjs[ids[k + i]]).collect());
            k += h.len();
        }
        let f = m.add_face_welded(o, hs);
        if let Some(a) = attrs {
            m.faces[f].attrs.update(a);
        }
    }
    m
}

type Fp = (Vec<String>, Vec<String>, Vec<String>);

fn fingerprint(m: &Mesh) -> Fp {
    let pk = |p: DVec3| format!("{:?}", (p.x, p.y, p.z));
    let fidx: std::collections::HashMap<_, _> = m
        .face_ids()
        .into_iter()
        .enumerate()
        .map(|(i, f)| (f, i))
        .collect();
    let mut verts: Vec<String> = m.vertex_ids().iter().map(|&v| pk(m.pos(v))).collect();
    verts.sort();
    let faces = m
        .face_ids()
        .iter()
        .map(|&f| {
            format!(
                "{:?}|{:?}|{:?}",
                m.face_positions(f)
                    .iter()
                    .map(|&p| pk(p))
                    .collect::<Vec<_>>(),
                m.face_hole_positions(f),
                m.face(f).attrs
            )
        })
        .collect();
    let edges = m
        .edge_ids()
        .iter()
        .map(|&e| {
            let ed = m.edge(e);
            format!(
                "{}{}{}{:?}{:?}",
                pk(m.pos(ed.v0)),
                pk(m.pos(ed.v1)),
                ed.soft,
                ed.curve,
                ed.faces.iter().map(|f| fidx[f]).collect::<Vec<_>>()
            )
        })
        .collect();
    (verts, faces, edges)
}

#[test]
fn test_bulk_faces_match_sequential() {
    for seed in 0..8 {
        let faces = gen_faces(120, seed);
        assert_eq!(
            fingerprint(&build_sequential(&faces)),
            fingerprint(&build_bulk(&faces)),
            "seed {seed}"
        );
    }
}

#[test]
fn test_bulk_edges_dedup_flags_and_degenerates() {
    let mut m = Mesh::new();
    let pos = [
        DVec3::new(0., 0., 0.),
        DVec3::new(1., 0., 0.),
        DVec3::new(1., 0., 0.),
        DVec3::new(0., 0., 0.),
        DVec3::new(2., 0., 0.),
        DVec3::new(2. + 1e-6, 0., 0.),
    ];
    let (vobjs, ids) = m.bulk_weld(&pos);
    let flags = [
        EdgeFlags {
            soft: true,
            curve: Some(7),
            layer: None,
            hidden: false,
        },
        EdgeFlags {
            soft: false,
            curve: None,
            layer: Some("L1".into()),
            hidden: false,
        },
        EdgeFlags::default(),
    ];
    let ia: Vec<usize> = ids.iter().step_by(2).copied().collect();
    let ib: Vec<usize> = ids.iter().skip(1).step_by(2).copied().collect();
    m.add_edges_welded(&vobjs, &ia, &ib, Some(&flags));
    assert_eq!(m.edges.len(), 1);
    let e = m.edge(m.edge_ids()[0]);
    assert!(e.soft && e.curve == Some(7) && e.layer.as_deref() == Some("L1"));
}

#[test]
fn test_bulk_weld_reuses_preexisting_vertices() {
    let mut m = Mesh::new();
    let v0 = m.vertex(DVec3::ZERO);
    let (vobjs, ids) = m.bulk_weld(&[DVec3::ZERO, DVec3::new(5., 0., 0.)]);
    assert_eq!(vobjs[ids[0]], v0);
    assert_eq!(m.verts.len(), 2);
}
