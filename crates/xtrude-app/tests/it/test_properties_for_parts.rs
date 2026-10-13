// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Properties for a part: one group's size along the model axes, where its
//! box starts, its area, whether it is a closed solid (and why not, with
//! Show… opening it on the edges to blame), its volume; several groups'
//! combined size, volume and mass; and the print material menu.

use crate::common;

use common::{v3, window};
use glam::{DMat4, DVec3};
use xtrude_app::tray::properties::Properties;
use xtrude_app::window::{CtxItem, MainWindow};
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;
use xtrude_model::scene::SelectMode;

const MM: f64 = 0.001;

/// An axis-aligned box from `lo`, `size` in mm; without its top when
/// `open`.
fn cuboid(lo: DVec3, size: DVec3, open: bool) -> Mesh {
    let (lo, s) = (lo * MM, size * MM);
    let p = |x: f64, y: f64, z: f64| lo + DVec3::new(x * s.x, y * s.y, z * s.z);
    let mut m = Mesh::new();
    let mut quads = vec![
        [p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.)],
        [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)],
        [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)],
        [p(1., 1., 0.), p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.)],
        [p(0., 1., 0.), p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.)],
    ];
    if !open {
        quads.push([p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)]);
    }
    for q in quads {
        m.add_face(&q, &[]);
    }
    m
}

fn group(w: &mut MainWindow, m: Mesh, name: &str) -> Group {
    Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(name))
}

fn add(w: &mut MainWindow, g: Group) -> GroupId {
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.version += 1;
    id
}

fn block(w: &mut MainWindow, at: (f64, f64, f64), size: (f64, f64, f64), open: bool) -> GroupId {
    let m = cuboid(
        DVec3::new(at.0, at.1, at.2),
        DVec3::new(size.0, size.1, size.2),
        open,
    );
    let g = group(w, m, "block");
    add(w, g)
}

fn describe(w: &MainWindow, groups: &[GroupId]) -> String {
    let sel: Vec<EntityRef> = groups.iter().map(|g| EntityRef::Group(*g)).collect();
    Properties::default().describe(w, &sel)
}

fn select(w: &mut MainWindow, groups: &[GroupId]) -> Properties {
    w.viewport.scene.select(
        groups.iter().map(|g| EntityRef::Group(*g)),
        SelectMode::Replace,
    );
    let mut p = Properties::default();
    p.refresh(w);
    p
}

#[test]
fn test_a_closed_block_reads_its_size_origin_area_volume_and_check() {
    let mut w = window();
    let g = block(&mut w, (100., 0., 5.), (20., 10., 30.), false);
    let text = describe(&w, &[g]);
    assert!(text.contains("<b>Solid Group</b>"), "{text}");
    assert!(text.contains("Size: 20.00 × 10.00 × 30.00 mm"), "{text}");
    assert!(
        text.contains("Origin: 100.00 mm, 0.00 mm, 5.00 mm"),
        "{text}"
    );
    assert!(text.contains("Area: 2200.00 mm²"), "{text}");
    assert!(text.contains("Volume: 6.00 cm³"), "{text}");
    assert!(text.contains("✔ Closed solid"), "{text}");
    assert!(!select(&mut w, &[g]).can_show_problems);
}

#[test]
fn test_an_open_block_says_why_it_is_not_closed_and_has_no_volume_or_mass() {
    let mut w = window();
    let g = block(&mut w, (0., 0., 0.), (10., 10., 10.), true);
    let text = describe(&w, &[g]);
    assert!(text.contains("<b>Group</b>"), "{text}");
    assert!(text.contains("✖ Not closed: 4 open borders"), "{text}");
    assert!(!text.contains("Volume"), "{text}");
    assert!(text.contains("Not a closed solid: no mass"), "{text}");
    assert!(text.contains("Area: 500.00 mm²"), "{text}");
    assert!(select(&mut w, &[g]).can_show_problems);
}

#[test]
fn test_show_opens_the_block_on_its_open_borders() {
    let mut w = window();
    let g = block(&mut w, (0., 0., 0.), (10., 10., 10.), true);
    let mut p = select(&mut w, &[g]);
    p.show_problems(&mut w);
    assert_eq!(w.viewport.scene.edit_group(), Some(g));
    let dlg = w.solid_inspector.as_ref().expect("the inspector is open");
    assert_eq!(dlg.current, Some(0));
    let sel = &w.viewport.scene.selection;
    assert_eq!(sel.len(), 4);
    assert!(sel.iter().all(|e| matches!(e, EntityRef::Edge(..))));
}

#[test]
fn test_show_opens_the_nested_body_that_is_not_closed() {
    let mut w = window();
    let closed = group(
        &mut w,
        cuboid(DVec3::ZERO, DVec3::splat(10.), false),
        "closed",
    );
    let open = group(
        &mut w,
        cuboid(DVec3::new(20., 0., 0.), DVec3::splat(10.), true),
        "open",
    );
    let open_id = open.id;
    let mut part = group(&mut w, Mesh::new(), "part");
    part.adopt(vec![closed, open]);
    let part = add(&mut w, part);
    let text = describe(&w, &[part]);
    assert!(text.contains("✖ Not closed: 4 open borders"), "{text}");
    let mut p = select(&mut w, &[part]);
    assert!(p.can_show_problems);
    p.show_problems(&mut w);
    assert_eq!(w.viewport.scene.edit_group(), Some(open_id));
    assert_eq!(w.viewport.scene.selection.len(), 4);
}

#[test]
fn test_a_part_of_closed_bodies_sums_their_volumes_where_they_are() {
    let mut w = window();
    let a = group(&mut w, cuboid(DVec3::ZERO, DVec3::splat(10.), false), "a");
    let mut b = group(&mut w, cuboid(DVec3::ZERO, DVec3::splat(10.), false), "b");
    // doubled along every axis: eight times the volume
    b.xform = Some(DMat4::from_translation(v3(0.05, 0., 0.)) * DMat4::from_scale(DVec3::splat(2.)));
    b.component = false;
    let mut part = group(&mut w, Mesh::new(), "part");
    part.adopt(vec![a, b]);
    part.xform = Some(DMat4::from_translation(v3(0., 0.1, 0.)));
    part.component = false;
    let part = add(&mut w, part);
    let text = describe(&w, &[part]);
    assert!(text.contains("✔ Closed solid"), "{text}");
    assert!(text.contains("Volume: 9.00 cm³"), "{text}");
    assert!(text.contains("Size: 70.00 × 20.00 × 20.00 mm"), "{text}");
    assert!(
        text.contains("Origin: 0.00 mm, 100.00 mm, 0.00 mm"),
        "{text}"
    );
    assert!(text.contains("Area: 3000.00 mm²"), "{text}");
}

#[test]
fn test_several_groups_read_their_combined_size_volume_and_mass() {
    let mut w = window();
    let a = block(&mut w, (0., 0., 0.), (20., 20., 20.), false);
    let b = block(&mut w, (40., 0., 0.), (20., 20., 20.), false);
    let text = describe(&w, &[a, b]);
    assert!(text.contains("2 groups"), "{text}");
    assert!(
        text.contains("Combined size: 60.00 × 20.00 × 20.00 mm"),
        "{text}"
    );
    assert!(text.contains("Total volume: 16.00 cm³"), "{text}");
    // two 20 mm PLA cubes, 9.92 g each
    assert!(
        text.contains("Total mass (100 % infill): 19.84 g"),
        "{text}"
    );
    let c = block(&mut w, (80., 0., 0.), (20., 20., 20.), true);
    let text = describe(&w, &[a, b, c]);
    assert!(
        text.contains("Combined size: 100.00 × 20.00 × 20.00 mm"),
        "{text}"
    );
    assert!(!text.contains("Total volume"), "{text}");
    assert!(
        text.contains("Not all closed solids: no total mass"),
        "{text}"
    );
}

#[test]
fn test_the_print_material_menu_is_the_context_menus_and_sets_the_material() {
    let mut w = window();
    let g = block(&mut w, (0., 0., 0.), (20., 20., 20.), false);
    let p = select(&mut w, &[g]);
    let items = p.material_menu.clone().expect("a menu for a group");
    let petg = items
        .iter()
        .find_map(|i| match i {
            CtxItem::Entry { id, text, .. } if text == "PETG" => Some(id.clone()),
            _ => None,
        })
        .expect("PETG");
    w.trigger(&petg);
    let text = describe(&w, &[g]);
    assert!(text.contains("Print material: PETG<br>"), "{text}");
    w.viewport.scene.clear_selection();
    let mut p = Properties::default();
    p.refresh(&w);
    assert!(p.material_menu.is_none());
}
