// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Measuring tests: a component's parts measured like a cut list. The
//! Parts-tray tests are in xtrude-app.

use crate::common;
use common::*;
use xtrude_core::attrs::Paint;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::{add_mesh, Group, MeshTable};
use xtrude_model::parts::{is_surface, part_material, part_points, part_rows, part_size};
use xtrude_model::units::{fmt_len_fine, fmt_triple, set_override, Units};

const QUADS: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

fn boxm(sx: f64, sy: f64, sz: f64, color: Option<[f64; 3]>) -> Mesh {
    let mut c = Vec::new();
    for dz in [0., 1.] {
        for dy in [0., 1.] {
            for dx in [0., 1.] {
                c.push(V3(dx * sx, dy * sy, dz * sz));
            }
        }
    }
    let mut m = Mesh::new();
    for q in QUADS {
        let f = m.add_face(&q.map(|k| c[k]), &[]);
        if let Some(col) = color {
            m.face_mut(f).attrs.paint.color = Some(col);
        }
    }
    m
}

fn part(t: &mut MeshTable, m: Mesh, name: &str, xform: Option<DMat4>) -> Group {
    let id = add_mesh(t, m);
    let mut g = Group::new(id, Some(name));
    g.xform = Some(xform.unwrap_or(DMat4::IDENTITY));
    g
}

fn approx3(a: (f64, f64, f64), b: (f64, f64, f64), tol: f64) -> bool {
    (a.0 - b.0).abs() < tol && (a.1 - b.1).abs() < tol && (a.2 - b.2).abs() < tol
}

#[test]
fn test_a_board_measures_on_its_own_axes() {
    let mut t = MeshTable::new();
    let b = part(&mut t, boxm(0.8, 0.3, 0.018, None), "board", None);
    assert!(approx3(
        part_size(&part_points(&t, &b)),
        (0.8, 0.3, 0.018),
        1e-6
    ));
}

#[test]
fn test_a_splayed_leg_measures_along_its_length() {
    let mut t = MeshTable::new();
    let turn = rotation_deg(DVec3::Y, 30.0);
    let leg = part(&mut t, boxm(0.04, 0.04, 0.58, None), "leg", Some(turn));
    assert!(approx3(
        part_size(&part_points(&t, &leg)),
        (0.58, 0.04, 0.04),
        1e-5
    ));
}

#[test]
fn test_a_stepped_carcass_measures_on_the_models_axes() {
    let mut pts = Vec::new();
    for x in [0.0, 0.442] {
        for y in [-0.052, 0.56] {
            for z in [0.085, 0.794] {
                pts.push(V3(x, y, z));
            }
        }
    }
    for x in [0.0, 0.442] {
        for y in [0.0, 0.56] {
            for z in [0.0, 0.085] {
                pts.push(V3(x, y, z));
            }
        }
    }
    assert!(approx3(part_size(&pts), (0.794, 0.612, 0.442), 1e-6));
}

#[test]
fn test_the_material_is_the_one_covering_most_of_the_part() {
    let mut t = MeshTable::new();
    let mut m = boxm(1.0, 1.0, 0.02, Some([1., 0., 0.]));
    for f in m.face_ids() {
        if m.face_area(f) < 0.1 {
            m.face_mut(f).attrs.paint.color = Some([0., 0., 1.]); // the thin edges
        }
    }
    let top = part(&mut t, m, "top", None);
    assert_eq!(part_material(&t, &top), "#ff0000");
}

#[test]
fn test_a_part_inherits_its_containers_paint_on_default_faces() {
    let mut t = MeshTable::new();
    let mut cube = part(&mut t, boxm(0.5, 0.5, 0.5, None), "cube", None);
    cube.material = Some(Paint {
        color: Some([0., 1., 0.]),
        ..Paint::default()
    });
    assert_eq!(part_material(&t, &cube), "#00ff00");
}

#[test]
fn test_part_sizes_resolve_the_millimetre_whatever_the_display_precision() {
    set_override(Some(Units::new("m", 2)));
    let coarse = fmt_triple(0.0325, 0.024, 0.0043, false);
    let fine = fmt_triple(0.0325, 0.024, 0.0043, true);
    let len = fmt_len_fine(0.0043);
    set_override(Some(Units::new("mm", 0)));
    let mm = fmt_triple(0.0325, 0.024, 0.0043, true);
    set_override(None);
    assert_eq!(coarse, "0.03 × 0.02 × 0.00 m");
    assert_eq!(fine, "0.033 × 0.024 × 0.004 m");
    assert_eq!(len, "0.004 m");
    assert_eq!(mm, "32 × 24 × 4 mm");
}

fn sheet(sx: f64, sy: f64) -> Mesh {
    let pts = [
        V3(0., 0., 0.),
        V3(sx, 0., 0.),
        V3(sx, sy, 0.),
        V3(0., sy, 0.),
    ];
    let mut m = Mesh::new();
    m.add_face(&pts, &[]);
    let mut rev = pts;
    rev.reverse();
    m.add_face(&rev, &[]);
    m
}

#[test]
fn test_a_part_with_no_thickness_is_a_surface_not_a_zero_board() {
    let mut t = MeshTable::new();
    let skin = part(&mut t, sheet(0.6, 0.135), "baseboard skin", None);
    let shelf = part(&mut t, boxm(0.6, 0.4, 0.018, None), "shelf", None);
    let hid = add_mesh(&mut t, Mesh::new());
    let mut holder = Group::new(hid, None);
    holder.adopt(vec![skin, shelf]);
    let rows = part_rows(&t, &holder);
    assert_eq!(
        rows.iter().map(|r| is_surface(r.size)).collect::<Vec<_>>(),
        vec![true, false]
    );
}
