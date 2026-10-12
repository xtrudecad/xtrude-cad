// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A component's parts pulled apart and put back exactly. The Parts-tray
//! slider test is in xtrude-app.

use crate::common;
use common::*;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::commands::{
    ExplodeGroupCommand, ExplodeViewCommand, MoveGroupCommand, SplitIntoPiecesCommand,
};
use xtrude_model::explode::rotate_offsets;
use xtrude_model::group::{add_mesh, copy_group, world_mesh, Exploded, Group, MeshTable};
use xtrude_model::history::{Command, History};
use xtrude_model::ids::GroupId;
use xtrude_model::parts::part_points;
use xtrude_model::pieces::split_into_pieces;
use xtrude_model::scene::Scene;

const QUADS: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

fn cube(t: &mut MeshTable, at: (f64, f64, f64)) -> Group {
    let mut c = Vec::new();
    for dz in [0., 1.] {
        for dy in [0., 1.] {
            for dx in [0., 1.] {
                c.push(v3(at.0 + dx, at.1 + dy, at.2 + dz));
            }
        }
    }
    let mut m = Mesh::new();
    for q in QUADS {
        m.add_face(&q.map(|k| c[k]), &[]);
    }
    let id = add_mesh(t, m);
    let mut g = Group::new(id, None);
    g.xform = Some(DMat4::IDENTITY);
    g
}

/// A centre cube, one on each side along red, one above.
fn assembly(placed: bool) -> (Scene, GroupId) {
    let mut s = Scene::new();
    let t = &mut s.doc.meshes;
    let kids = vec![
        cube(t, (0., 0., 0.)),
        cube(t, (-3., 0., 0.)),
        cube(t, (3., 0., 0.)),
        cube(t, (0., 0., 3.)),
    ];
    let bm = add_mesh(t, Mesh::new());
    let mut b = Group::new(bm, Some("box"));
    b.adopt(kids);
    if placed {
        b.xform = Some(DMat4::from_translation(v3(10., 5., 0.)) * rotation_deg(DVec3::Z, 35.0));
    }
    let id = b.id;
    s.doc.groups.push(b);
    (s, id)
}

fn world(s: &Scene, g: GroupId) -> Vec<(i64, i64, i64)> {
    let wm = world_mesh(&s.doc.meshes, s.doc.find_group(g).unwrap());
    let mut v: Vec<_> = wm
        .verts
        .iter()
        .map(|(_, x)| {
            let p = x.position;
            (
                (p.x * 1e5).round() as i64,
                (p.y * 1e5).round() as i64,
                (p.z * 1e5).round() as i64,
            )
        })
        .collect();
    v.sort();
    v
}

fn centre(s: &Scene, part: &Group) -> DVec3 {
    let pts = part_points(&s.doc.meshes, part);
    let lo = pts
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = pts
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    (lo + hi) / 2.0
}

fn centres(s: &Scene, b: GroupId) -> Vec<DVec3> {
    let g = s.doc.find_group(b).unwrap();
    g.children.iter().map(|k| centre(s, k)).collect()
}

fn view(b: GroupId, factor: f64, mode: &str) -> ExplodeViewCommand {
    ExplodeViewCommand {
        container: b,
        factor,
        mode: mode.into(),
    }
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-5
}

#[test]
fn test_parts_move_away_from_the_centre_in_proportion() {
    let (mut s, b) = assembly(false);
    view(b, 1.0, "outward").apply(&mut s).unwrap();
    let want = [
        v3(0.5, 0.5, -1.0),
        v3(-5.5, 0.5, -1.0),
        v3(6.5, 0.5, -1.0),
        v3(0.5, 0.5, 5.0),
    ];
    for (c, w) in centres(&s, b).iter().zip(want) {
        assert!(close(*c, w), "{c:?} vs {w:?}");
    }
}

#[test]
fn test_along_one_axis_only_that_axis_moves() {
    let (mut s, b) = assembly(false);
    view(b, 1.0, "z").apply(&mut s).unwrap();
    let c = centres(&s, b);
    let want_xy = [(0.5, 0.5), (-2.5, 0.5), (3.5, 0.5), (0.5, 0.5)];
    for (p, (x, y)) in c.iter().zip(want_xy) {
        assert!((p.x - x).abs() < 1e-5 && (p.y - y).abs() < 1e-5);
    }
    assert!((c[3].z - 5.0).abs() < 1e-6);
}

#[test]
fn test_reassemble_and_undo_are_exact() {
    let (mut s, b) = assembly(true);
    let mut h = History::new();
    let assembled = world(&s, b);
    h.execute(&mut s, &mut view(b, 1.5, "outward"));
    assert_ne!(world(&s, b), assembled);
    h.execute(&mut s, &mut view(b, 0.4, "y")); // amount and way changed
    h.execute(&mut s, &mut view(b, 0.0, "outward"));
    let g = s.doc.find_group(b).unwrap();
    assert!(world(&s, b) == assembled && g.exploded.is_none());
    assert!(g.children.iter().all(|k| k.explode_offset.is_none()));
    h.execute(&mut s, &mut view(b, 0.4, "y"));
    for _ in 0..4 {
        assert!(h.undo(&mut s));
    }
    assert_eq!(world(&s, b), assembled);
}

#[test]
fn test_a_part_moved_by_hand_keeps_that_move() {
    let (mut s, b) = assembly(false);
    view(b, 1.0, "outward").apply(&mut s).unwrap();
    let part = s.doc.find_group(b).unwrap().children[1].id;
    MoveGroupCommand {
        group: part,
        delta: v3(0., 2., 0.),
    }
    .apply(&mut s)
    .unwrap();
    view(b, 0.0, "outward").apply(&mut s).unwrap();
    let p = s.doc.find_group(part).unwrap().clone();
    assert!(close(centre(&s, &p), v3(-2.5, 2.5, 0.5))); // assembled + the hand move
}

#[test]
fn test_reassembling_after_opening_the_placed_component() {
    let (mut s, b) = assembly(true);
    let assembled = world(&s, b);
    view(b, 1.0, "outward").apply(&mut s).unwrap();
    s.begin_group_edit(b);
    s.end_group_edit();
    view(b, 0.0, "outward").apply(&mut s).unwrap();
    assert_eq!(world(&s, b), assembled);
}

#[test]
fn test_the_document_remembers_the_exploded_view() {
    let (mut s, b) = assembly(true);
    let assembled = world(&s, b);
    view(b, 0.8, "z").apply(&mut s).unwrap();
    let mut again = roundtrip(&s);
    let b2 = again.doc.groups[0].id;
    assert_eq!(
        again.doc.groups[0].exploded,
        Some(Exploded {
            factor: 0.8,
            mode: "z".into()
        })
    );
    view(b2, 0.0, "outward").apply(&mut again).unwrap();
    assert_eq!(world(&again, b2), assembled);
}

#[test]
fn test_a_copy_reassembles_on_its_own() {
    let (mut s, b) = assembly(false);
    let assembled = world(&s, b);
    view(b, 1.0, "outward").apply(&mut s).unwrap();
    let mut src = s.doc.find_group(b).unwrap().clone();
    let twin = copy_group(&mut s.doc.meshes, &mut src, None, false);
    let tid = twin.id;
    s.doc.groups.push(twin);
    view(tid, 0.0, "outward").apply(&mut s).unwrap();
    assert_eq!(world(&s, tid), assembled);
    assert!(s.doc.find_group(b).unwrap().exploded.is_some()); // the original stays apart
}

#[test]
fn test_split_starts_a_fresh_assembly() {
    let (mut s, b) = assembly(false);
    let mut h = History::new();
    view(b, 1.0, "outward").apply(&mut s).unwrap();
    let g = s.doc.find_group(b).unwrap().clone();
    let pieces = split_into_pieces(&mut s.doc.meshes, &g);
    h.execute(&mut s, &mut SplitIntoPiecesCommand { group: b, pieces });
    assert!(s.doc.find_group(b).unwrap().exploded.is_none());
    assert!(h.undo(&mut s));
    assert_eq!(
        s.doc.find_group(b).unwrap().exploded,
        Some(Exploded {
            factor: 1.0,
            mode: "outward".into()
        })
    );
}

#[test]
fn test_exploding_the_group_frees_the_parts_where_they_stand() {
    let (mut s, b) = assembly(false);
    let mut h = History::new();
    view(b, 1.0, "outward").apply(&mut s).unwrap();
    let spread = world(&s, b);
    h.execute(&mut s, &mut ExplodeGroupCommand { group: b });
    assert!(s.doc.groups.iter().all(|g| g.explode_offset.is_none()));
    let ids: Vec<GroupId> = s.doc.groups.iter().map(|g| g.id).collect();
    let mut freed: Vec<_> = ids.iter().flat_map(|g| world(&s, *g)).collect();
    freed.sort();
    assert_eq!(freed, spread);
    assert!(h.undo(&mut s));
    let g = s.doc.find_group(b).unwrap();
    assert!(g.children[1..].iter().all(|k| k.explode_offset.is_some()));
}

#[test]
fn test_offsets_turn_with_a_pushed_down_matrix() {
    let mut t = MeshTable::new();
    let mut part = cube(&mut t, (0., 0., 0.));
    part.explode_offset = Some(v3(1., 0., 0.));
    let hm = add_mesh(&mut t, Mesh::new());
    let mut holder = Group::new(hm, None);
    holder.adopt(vec![part]);
    rotate_offsets(&mut holder, &rotation_deg(DVec3::Z, 90.0));
    let o = holder.children[0].explode_offset.unwrap();
    assert!(close(o, v3(0., 1., 0.)));
}
