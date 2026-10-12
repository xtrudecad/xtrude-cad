// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Parts-tray tests: the tray lists a component's parts, selects, hides and
//! renames them, offers Split on a plain group, updates in place, scrolls a
//! part picked in the canvas into view, keeps a rename typed through a refresh
//! and shows a surface as an area. (The measuring tests are in xtrude-model.)

use crate::common;

use common::{v3, window};
use xtrude_app::tray::parts::AllVisible;
use xtrude_app::tray::Tray;
use xtrude_app::window::MainWindow;
use xtrude_core::math::{rotation_deg, DMat4};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;

const QUADS: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

fn boxm(m: &mut Mesh, s: (f64, f64, f64), at: (f64, f64, f64), color: Option<[f64; 3]>) {
    let mut c = Vec::new();
    for dz in [0., 1.] {
        for dy in [0., 1.] {
            for dx in [0., 1.] {
                c.push(v3(at.0 + dx * s.0, at.1 + dy * s.1, at.2 + dz * s.2));
            }
        }
    }
    for q in QUADS {
        let f = m.add_face(&q.map(|k| c[k]), &[]);
        if let Some(col) = color {
            m.face_mut(f).attrs.paint.color = Some(col);
        }
    }
}

fn part(w: &mut MainWindow, m: Mesh, name: &str, xform: DMat4) -> Group {
    let mut g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(name));
    g.xform = Some(xform);
    g
}

fn boxed(w: &mut MainWindow, s: (f64, f64, f64), at: (f64, f64, f64), name: &str) -> Group {
    let mut m = Mesh::new();
    boxm(&mut m, s, at, None);
    part(w, m, name, DMat4::IDENTITY)
}

fn holder(w: &mut MainWindow, name: &str, kids: Vec<Group>) -> Group {
    let mut g = Group::new(
        add_mesh(&mut w.viewport.scene.doc.meshes, Mesh::new()),
        Some(name),
    );
    g.adopt(kids);
    g
}

/// The group in the model, selected.
fn window_with(w: &mut MainWindow, g: Group) -> GroupId {
    let id = g.id;
    let scene = &mut w.viewport.scene;
    scene.doc.groups.push(g);
    scene.selection.clear();
    scene.selection.insert(EntityRef::Group(id));
    scene.version += 1;
    id
}

fn kids(w: &MainWindow, g: GroupId) -> Vec<(GroupId, String, bool)> {
    w.viewport
        .scene
        .doc
        .find_group(g)
        .unwrap()
        .children
        .iter()
        .map(|k| (k.id, k.name.clone(), k.hidden))
        .collect()
}

/// A seat and two identical legs, one of them splayed.
fn stool(w: &mut MainWindow) -> Group {
    let wood = Some([0.6, 0.4, 0.2]);
    let splay = DMat4::from_translation(v3(0.5, 0., 0.)) * rotation_deg(DVec3::Y, 10.0);
    let mut seat = Mesh::new();
    boxm(&mut seat, (0.40, 0.40, 0.03), (0., 0., 0.), wood);
    let mut a = Mesh::new();
    boxm(&mut a, (0.04, 0.04, 0.58), (0., 0., 0.), wood);
    let mut b = Mesh::new();
    boxm(&mut b, (0.04, 0.04, 0.58), (0., 0., 0.), wood);
    let kids = vec![
        part(w, seat, "seat", DMat4::IDENTITY),
        part(w, a, "leg A", DMat4::IDENTITY),
        part(w, b, "leg B", splay),
    ];
    holder(w, "stool", kids)
}

fn many_parts(w: &mut MainWindow, n: usize) -> Group {
    let kids = (0..n)
        .map(|i| {
            boxed(
                w,
                (0.1, 0.1, 0.1),
                (i as f64 * 0.2, 0., 0.),
                &format!("Piece {}", i + 1),
            )
        })
        .collect();
    holder(w, "cabinet", kids)
}

#[test]
fn test_the_tray_lists_selects_hides_and_renames_parts() {
    let mut w = window();
    let g = stool(&mut w);
    let stool = window_with(&mut w, g);
    let mut tray = Tray::default();
    let panel = &mut tray.parts;
    panel.refresh(&w);
    let names: Vec<&str> = panel.lines.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["seat", "leg A", "leg B"]);
    let k = kids(&w, stool);

    panel.click(&mut w, &[k[1].0]); // the model tree's click
    assert_eq!(w.viewport.scene.edit_group(), Some(stool));
    assert_eq!(
        w.viewport
            .scene
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [EntityRef::Group(k[1].0)]
    );
    panel.refresh(&w);
    assert_eq!(panel.lines.len(), 3); // still listed inside it

    panel.set_hidden(&mut w, k[2].0, true);
    assert!(kids(&w, stool)[2].2);
    panel.begin_rename(k[0].0);
    panel.editing.as_mut().unwrap().1 = "Seat board".into();
    panel.commit_rename(&mut w);
    assert_eq!(kids(&w, stool)[0].1, "Seat board");
    w.viewport.undo();
    assert_eq!(kids(&w, stool)[0].1, "seat");
    w.viewport.undo();
    assert!(!kids(&w, stool)[2].2);
}

#[test]
fn test_a_plain_group_offers_to_split() {
    let mut w = window();
    let mut m = Mesh::new();
    boxm(&mut m, (0.1, 0.1, 0.1), (0., 0., 0.), None);
    boxm(&mut m, (0.1, 0.1, 0.1), (1., 0., 0.), None);
    let g = Group::new(
        add_mesh(&mut w.viewport.scene.doc.meshes, m),
        Some("two blocks"),
    );
    let group = window_with(&mut w, g);
    let mut tray = Tray::default();
    let panel = &mut tray.parts;
    panel.refresh(&w);
    assert!(panel.lines.is_empty()); // no list shown
    assert!(panel.split_enabled);
    panel.split(&mut w);
    let names: Vec<String> = kids(&w, group).into_iter().map(|k| k.1).collect();
    assert_eq!(names, ["Piece 1", "Piece 2"]);
    assert_eq!(panel.lines.len(), 2);
}

#[test]
fn test_the_list_updates_in_place_and_keeps_its_scroll() {
    // A click on a row below the fourteenth must not throw the list back to
    // its top: the refresh it triggers must not ask for a scroll.
    let mut w = window();
    let g = many_parts(&mut w, 20);
    let cab = window_with(&mut w, g);
    let mut tray = Tray::default();
    tray.parts.refresh(&w);
    let row = tray.parts.lines[17].part;
    tray.parts.click(&mut w, &[row]);
    tray.refresh(&w);
    assert_eq!(tray.parts.lines[17].part, row);
    assert_eq!(tray.parts.scroll_to, None);
    assert_eq!(
        w.viewport
            .scene
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [EntityRef::Group(kids(&w, cab)[17].0)]
    );
}

#[test]
fn test_a_part_selected_in_the_canvas_scrolls_into_view() {
    let mut w = window();
    let g = many_parts(&mut w, 20);
    let cab = window_with(&mut w, g);
    let mut tray = Tray::default();
    tray.parts.refresh(&w);
    assert_eq!(tray.parts.scroll_to, None);
    let last = kids(&w, cab)[19].0;
    w.viewport.scene.begin_group_edit(cab);
    w.viewport.scene.selection.insert(EntityRef::Group(last));
    w.viewport.scene.version += 1;
    tray.refresh(&w);
    assert!(tray.parts.lines[19].selected);
    assert_eq!(tray.parts.scroll_to, Some(last));
    w.viewport.scene.end_group_edit();
}

#[test]
fn test_all_parts_visible_shows_every_hidden_part() {
    let mut w = window();
    let g = many_parts(&mut w, 4);
    let cab = window_with(&mut w, g);
    let mut tray = Tray::default();
    let panel = &mut tray.parts;
    panel.refresh(&w);
    let k = kids(&w, cab);
    panel.set_hidden(&mut w, k[1].0, true);
    panel.set_hidden(&mut w, k[2].0, true);
    assert_eq!(panel.all_visible, AllVisible::Partial);
    panel.toggle_all_visible(&mut w);
    assert!(!kids(&w, cab).iter().any(|k| k.2));
    assert_eq!(panel.all_visible, AllVisible::Checked);
    w.viewport.undo();
    assert_eq!(kids(&w, cab).iter().filter(|k| k.2).count(), 2);
}

#[test]
fn test_a_rename_survives_a_refresh_while_typing() {
    let mut w = window();
    let g = many_parts(&mut w, 3);
    let cab = window_with(&mut w, g);
    let mut tray = Tray::default();
    tray.parts.refresh(&w);
    let item = tray.parts.lines[2].part;
    tray.parts.begin_rename(item);
    tray.refresh(&w); // an edit elsewhere lands
    let editor = tray
        .parts
        .editing
        .as_mut()
        .expect("the editor is still open");
    assert_eq!(editor.0, item);
    editor.1 = "Right side wall".into();
    tray.parts.commit_rename(&mut w);
    assert_eq!(kids(&w, cab)[2].1, "Right side wall");
    assert_eq!(tray.parts.lines[2].name, "Right side wall");
}

fn sheet(sx: f64, sy: f64) -> Mesh {
    let pts = [
        v3(0., 0., 0.),
        v3(sx, 0., 0.),
        v3(sx, sy, 0.),
        v3(0., sy, 0.),
    ];
    let mut m = Mesh::new();
    m.add_face(&pts, &[]);
    let mut rev = pts;
    rev.reverse();
    m.add_face(&rev, &[]);
    m
}

#[test]
fn test_the_tray_shows_a_surface_as_an_area() {
    let mut w = window();
    let skin = part(&mut w, sheet(0.6, 0.135), "skin", DMat4::IDENTITY);
    let shelf = boxed(&mut w, (0.6, 0.4, 0.018), (0., 0., 0.), "shelf");
    let g = holder(&mut w, "unit", vec![skin, shelf]);
    window_with(&mut w, g);
    let mut tray = Tray::default();
    let panel = &mut tray.parts;
    panel.refresh(&w);
    assert!(panel.lines[0].size.ends_with("surface"));
    assert!(panel.lines[1].size.contains('×'));
    assert!(!panel.lines[1].size.ends_with("surface"));
}
