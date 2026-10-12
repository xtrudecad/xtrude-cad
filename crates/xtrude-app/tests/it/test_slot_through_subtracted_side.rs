// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A slot drawn on the bottom line of a shell made with Subtract pushes right
//! through its side, whatever order the faces come in; the bottom ring
//! survives as a C — whose interior point really is inside it.

use crate::common;

use common::{stub, v3};
use glam::DVec3;
use std::sync::Arc;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_core::arrangement::{interior_point, point_in_polygon};
use xtrude_core::cap_rebuild::region_test_point;
use xtrude_core::mt19937::Mt19937;
use xtrude_core::orient::is_closed;
use xtrude_core::Mesh;
use xtrude_model::edits::build_add_edges;
use xtrude_model::group::{Group, MeshTable};
use xtrude_model::ids::MeshId;
use xtrude_model::solids::{run, SolidOp};

const T: f64 = 0.3;
const RED: [f64; 3] = [1.0, 0.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 1.0];

/// The bottom ring of a 10 m shell with 0.30 m sides once a 1 m slot is cut
/// through its front side: a C whose FIRST edge is the notch side.
const C_SHAPE: [(f64, f64); 12] = [
    (5.0, 0.3),
    (5.0, 0.0),
    (10.0, 0.0),
    (10.0, 10.0),
    (0.0, 10.0),
    (0.0, 0.0),
    (4.0, 0.0),
    (4.0, 0.3),
    (0.3, 0.3),
    (0.3, 9.7),
    (9.7, 9.7),
    (9.7, 0.3),
];

#[test]
fn test_the_interior_point_of_a_c_is_inside_it() {
    assert!(point_in_polygon(interior_point(&C_SHAPE), &C_SHAPE));
    assert!(point_in_polygon(region_test_point(&C_SHAPE, &[]), &C_SHAPE));
}

fn box_group(t: &mut MeshTable, lo: DVec3, hi: DVec3, color: [f64; 3], name: &str) -> Group {
    let (x0, y0, z0, x1, y1, z1) = (lo.x, lo.y, lo.z, hi.x, hi.y, hi.z);
    let faces = [
        [
            v3(x0, y0, z0),
            v3(x0, y1, z0),
            v3(x1, y1, z0),
            v3(x1, y0, z0),
        ],
        [
            v3(x0, y0, z1),
            v3(x1, y0, z1),
            v3(x1, y1, z1),
            v3(x0, y1, z1),
        ],
        [
            v3(x0, y0, z0),
            v3(x1, y0, z0),
            v3(x1, y0, z1),
            v3(x0, y0, z1),
        ],
        [
            v3(x1, y0, z0),
            v3(x1, y1, z0),
            v3(x1, y1, z1),
            v3(x1, y0, z1),
        ],
        [
            v3(x1, y1, z0),
            v3(x0, y1, z0),
            v3(x0, y1, z1),
            v3(x1, y1, z1),
        ],
        [
            v3(x0, y1, z0),
            v3(x0, y0, z0),
            v3(x0, y0, z1),
            v3(x0, y1, z1),
        ],
    ];
    let mut m = Mesh::new();
    for f in faces {
        let id = m.add_face(&f, &[]);
        m.face_mut(id).attrs.paint.color = Some(color);
    }
    let id = MeshId::fresh();
    t.insert(id, Arc::new(m));
    Group::new(id, Some(name))
}

/// The shell's sides: a 10 m box minus its inside.
fn tube() -> Mesh {
    let mut t = MeshTable::new();
    let cutter = box_group(&mut t, v3(T, T, -0.5), v3(10. - T, 10. - T, 3.5), RED, "A");
    let shell = box_group(&mut t, v3(0., 0., 0.), v3(10., 10., 3.), BLUE, "B");
    let (_, made) = run(SolidOp::Subtract, &mut t, &[&cutter, &shell]).unwrap();
    (*t[&made[0].mesh]).clone()
}

fn reordered(m: &Mesh, order: &[usize]) -> Mesh {
    let ids = m.face_ids();
    let mut out = Mesh::new();
    for &i in order {
        let f = ids[i];
        out.add_face(&m.face_positions(f), &m.face_hole_positions(f));
    }
    out
}

fn push_slot(mesh: Mesh) -> Mesh {
    let mut vp = stub();
    *vp.scene.mesh_mut() = mesh;
    vp.scene.version += 1;
    let p = [
        v3(4., 0., 0.),
        v3(4., 0., 2.),
        v3(5., 0., 2.),
        v3(5., 0., 0.),
    ];
    let cmd = build_add_edges(
        &vp.scene,
        &[(p[0], p[1]), (p[1], p[2]), (p[2], p[3])],
        true,
        vec![],
        None,
    );
    vp.execute_boxed(cmd);
    let m = vp.scene.mesh();
    let slot = m
        .face_ids()
        .into_iter()
        .filter(|&f| {
            m.face_positions(f).iter().all(|q| {
                q.y.abs() < 1e-6 && (4. - 1e-6..=5. + 1e-6).contains(&q.x) && q.z <= 2. + 1e-6
            })
        })
        .min_by(|&a, &b| m.face_area(a).total_cmp(&m.face_area(b)))
        .expect("the slot");
    let mut t = PushPullTool::new();
    t.pp.setup_plain(m, MeshId::LOOSE, slot);
    t.pp.compute_inward_limit(vp.scene.mesh());
    t.pp.extrusion = -5.0;
    t.clamp(&mut vp);
    t.commit(&mut vp);
    vp.scene.mesh().clone()
}

#[test]
fn test_a_bottom_slot_goes_through_a_subtracted_side_in_any_face_order() {
    let mut rng = Mt19937::new(1);
    let base = tube();
    for _ in 0..12 {
        let mut order: Vec<usize> = (0..base.face_ids().len()).collect();
        rng.shuffle(&mut order);
        let mesh = push_slot(reordered(&base, &order));
        assert!(is_closed(&mesh), "{order:?}");
        let bottom: Vec<(usize, usize)> = mesh
            .face_ids()
            .into_iter()
            .filter(|&f| mesh.face_positions(f).iter().all(|q| q.z.abs() < 1e-6))
            .map(|f| (mesh.face(f).outer.len(), mesh.face(f).holes.len()))
            .collect();
        // The bottom ring survives as the C around the slot.
        assert_eq!(bottom, vec![(12, 0)], "{order:?}");
    }
}
