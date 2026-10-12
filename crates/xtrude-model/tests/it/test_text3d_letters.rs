// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Document tests — one group per letter, and the text stays editable. (The
//! .igz round trips live in xtrude-io; the select/menu/place-tool tests with
//! the app.)

use crate::common;
use common::*;
use xtrude_core::math::{rotation_deg, DMat4, MatExt};
use xtrude_core::text3d::{build_text_letters, build_text_mesh};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::{add_mesh, copy_group, iter_placements, Group, MeshTable};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;
use xtrude_model::text3d::{
    letters_fingerprint, make_text_group, rebuild_text_group, text_is_pristine, EditText3DCommand,
    TextParams,
};

fn extent<'a>(meshes: impl Iterator<Item = &'a Mesh>) -> [f64; 4] {
    let mut e = [f64::MAX, f64::MIN, f64::MAX, f64::MIN];
    for m in meshes {
        for (_, v) in m.verts.iter() {
            let p = v.position;
            e = [e[0].min(p.x), e[1].max(p.x), e[2].min(p.z), e[3].max(p.z)];
        }
    }
    e
}

fn params(text: &str) -> TextParams {
    TextParams::new(text, "Sans")
}

#[test]
fn test_letters_lay_out_exactly_like_the_one_piece_text() {
    let whole = build_text_mesh("XtrudeCAD", "Sans", true, false, 0.25, 0.05);
    let letters = build_text_letters("XtrudeCAD", "Sans", true, false, 0.25, 0.05);
    assert_eq!(
        letters.iter().map(|(c, _)| *c).collect::<String>(),
        "XtrudeCAD"
    );
    assert_eq!(
        letters.iter().map(|(_, m)| m.faces.len()).sum::<usize>(),
        whole.faces.len()
    );
    let a = extent(std::iter::once(&whole));
    let b = extent(letters.iter().map(|(_, m)| m));
    assert!(
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6),
        "{a:?} vs {b:?}"
    );
    let lefts: Vec<f64> = letters
        .iter()
        .map(|(_, m)| {
            m.verts
                .iter()
                .map(|(_, v)| v.position.x)
                .fold(f64::MAX, f64::min)
        })
        .collect();
    let mut sorted = lefts.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(lefts, sorted);
}

#[test]
fn test_spaces_make_no_letter_group() {
    let letters = build_text_letters("Ab c", "Sans", false, false, 0.25, 0.05);
    assert_eq!(letters.iter().map(|(c, _)| *c).collect::<String>(), "Abc");
}

#[test]
fn test_text_group_is_a_container_of_letter_groups() {
    let mut t = MeshTable::new();
    let mut p = params("Plaza");
    p.height = 0.4;
    let g = make_text_group(&mut t, &p).unwrap();
    assert!(g.is_instance());
    assert!(t[&g.mesh].faces.is_empty());
    assert_eq!(
        g.children
            .iter()
            .map(|k| k.name.as_str())
            .collect::<String>(),
        "Plaza"
    );
    assert!(g
        .children
        .iter()
        .all(|k| k.is_instance() && !t[&k.mesh].faces.is_empty()));
    let s = g.text3d.as_ref().unwrap();
    assert_eq!(s["text"], "Plaza");
    assert_eq!(s["height"], 0.4);
    assert!(make_text_group(&mut t, &params("   ")).is_none());
}

#[test]
fn test_copy_group_keeps_the_text_parameters() {
    let mut t = MeshTable::new();
    let mut g = make_text_group(&mut t, &params("Ab")).unwrap();
    let c = copy_group(&mut t, &mut g, None, false);
    assert_eq!(c.text3d, g.text3d);
}

fn pose() -> DMat4 {
    DMat4::from_translation(V3(10.0, 5.0, 0.0)) * rotation_deg(DVec3::Z, 90.0)
}

#[test]
fn test_edit_command_relays_the_letters_where_the_text_stands() {
    let mut scene = Scene::new();
    let mut history = History::new();
    let mut g = make_text_group(&mut scene.doc.meshes, &params("Ab")).unwrap();
    for k in &mut g.children {
        k.xform = Some(pose() * k.xform.unwrap());
    }
    let gid = g.id;
    let old_children = g.children.clone();
    scene.doc.groups.push(g);

    assert!(history.execute(
        &mut scene,
        &mut EditText3DCommand {
            group: gid,
            params: params("Hello")
        }
    ));
    let g = scene.doc.find_group(gid).unwrap().clone();
    assert_eq!(
        g.children
            .iter()
            .map(|k| k.name.as_str())
            .collect::<String>(),
        "Hello"
    );
    assert_eq!(g.name, "Hello");
    assert_eq!(g.text3d.as_ref().unwrap()["text"], "Hello");
    for k in &g.children {
        assert!(k.xform.unwrap().fuzzy_eq(&pose(), 1e-12));
    }
    let mut pts = Vec::new();
    for (gg, m) in iter_placements(&g, None) {
        for (_, v) in scene.doc.mesh(gg.mesh).verts.iter() {
            pts.push(m.map_or(v.position, |m| m.project_point(v.position)));
        }
    }
    let min_x = pts.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    let min_y = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    assert!((min_x - (10.0 - 0.05)).abs() < 1e-6, "{min_x}");
    assert!((min_y - 5.0).abs() < 0.05, "{min_y}");

    history.undo(&mut scene);
    let g = scene.doc.find_group(gid).unwrap();
    assert_eq!(g.children, old_children);
    assert_eq!(g.text3d.as_ref().unwrap()["text"], "Ab");
    assert_eq!(g.name, "Ab");
    history.redo(&mut scene);
    let g = scene.doc.find_group(gid).unwrap();
    assert_eq!(
        g.children
            .iter()
            .map(|k| k.name.as_str())
            .collect::<String>(),
        "Hello"
    );
}

#[test]
fn test_rebuild_from_a_container_without_letters_uses_identity() {
    let mut t = MeshTable::new();
    let mut g = make_text_group(&mut t, &params("A")).unwrap();
    g.children.clear();
    let kids = rebuild_text_group(&mut t, &g, &params("B"));
    assert_eq!(
        kids.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
        ["B"]
    );
    assert_eq!(kids[0].xform, Some(DMat4::IDENTITY));
}

fn touch_vertex(t: &mut MeshTable, letter: &Group) {
    let m = std::sync::Arc::make_mut(t.get_mut(&letter.mesh).unwrap());
    let v = m.verts.ids()[0];
    m.move_vertex(v, V3(0.0, 0.0, 0.01));
}

#[test]
fn test_a_letter_pushed_painted_erased_or_moved_makes_the_text_plain() {
    let mut t = MeshTable::new();
    let g = make_text_group(&mut t, &params("Ab")).unwrap();
    touch_vertex(&mut t, &g.children[0]);
    assert!(!text_is_pristine(&t, &g));

    let g = make_text_group(&mut t, &params("Ab")).unwrap();
    let m = std::sync::Arc::make_mut(t.get_mut(&g.children[1].mesh).unwrap());
    let f = m.face_ids()[0];
    m.face_mut(f).attrs.paint.color = Some([1.0, 0.0, 0.0]);
    assert!(!text_is_pristine(&t, &g));

    let mut g = make_text_group(&mut t, &params("Ab")).unwrap();
    g.children.pop();
    assert!(!text_is_pristine(&t, &g));

    let mut g = make_text_group(&mut t, &params("Ab")).unwrap();
    let x = g.children[0].xform.unwrap();
    g.children[0].xform = Some(x * DMat4::from_translation(V3(0.3, 0.0, 0.0)));
    assert!(!text_is_pristine(&t, &g));

    let mut g = make_text_group(&mut t, &params("Ab")).unwrap();
    let pose = rotation_deg(DVec3::Z, 30.0) * DMat4::from_translation(V3(5.0, 5.0, 0.0));
    for k in &mut g.children {
        k.xform = Some(pose * k.xform.unwrap());
    }
    assert!(text_is_pristine(&t, &g));
}

#[test]
fn test_an_edit_command_leaves_the_new_letters_pristine() {
    let mut scene = Scene::new();
    let mut history = History::new();
    let g = make_text_group(&mut scene.doc.meshes, &params("Ab")).unwrap();
    let gid = g.id;
    scene.doc.groups.push(g);
    assert!(history.execute(
        &mut scene,
        &mut EditText3DCommand {
            group: gid,
            params: params("Hello")
        }
    ));
    let g = scene.doc.find_group(gid).unwrap().clone();
    assert!(text_is_pristine(&scene.doc.meshes, &g));
    touch_vertex(&mut scene.doc.meshes, &g.children[0]);
    assert!(!text_is_pristine(&scene.doc.meshes, &g));
    history.undo(&mut scene);
    let g = scene.doc.find_group(gid).unwrap();
    assert!(text_is_pristine(&scene.doc.meshes, g));
}

/// The fingerprint is byte-stable: a reference digest of the same two
/// letters.
#[test]
fn test_fingerprint_matches_the_reference_digest() {
    let mut t = MeshTable::new();
    let mut m = Mesh::new();
    let f = m.add_face(
        &[
            V3(0., 0., 0.),
            V3(0.1, 0., 0.),
            V3(0.1, 0., 0.25),
            V3(0., 0., 0.25),
        ],
        &[],
    );
    m.face_mut(f).attrs.paint.color = Some([0.8, 0.1234, 0.5]);
    let mut m2 = Mesh::new();
    m2.add_face(&[V3(1., 0., 0.), V3(1.5, 0., 0.), V3(1.5, 0.05, 0.3)], &[]);
    let a = Group::new(add_mesh(&mut t, m), Some("Á"));
    let b = Group::new(add_mesh(&mut t, m2), Some("b"));
    assert_eq!(letters_fingerprint(&t, &[a, b]), "9352600db3d086fe");
}
