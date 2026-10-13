// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Print material and mass in the window: Properties shows a group's
//! material and mass, the group context menu's Print material submenu
//! sets a group's own, and Model ▸ Document print material… the
//! document's.

use crate::common;

use common::{v3, window};
use glam::DVec3;
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::tray::properties::Properties;
use xtrude_app::window::dialogs::ScriptedDialogs;
use xtrude_app::window::{CtxItem, MainWindow};
use xtrude_core::Mesh;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::GroupId;
use xtrude_model::print_material::{print_material_of, PrintMaterial};
use xtrude_model::scene::SelectMode;
use xtrude_model::units::Units;

fn win() -> (MainWindow, Rc<RefCell<ScriptedDialogs>>) {
    let mut w = window();
    w.viewport.scene.apply_units(&Units::new("mm", 2));
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    (w, dialogs)
}

/// A 20 mm cube's mesh at `x` metres; `open` leaves its top off.
fn cube_mesh(x: f64, open: bool) -> Mesh {
    let (lo, hi) = (DVec3::new(x, 0., 0.), DVec3::new(x + 0.02, 0.02, 0.02));
    let p = [
        v3(lo.x, lo.y, lo.z),
        v3(hi.x, lo.y, lo.z),
        v3(hi.x, hi.y, lo.z),
        v3(lo.x, hi.y, lo.z),
        v3(lo.x, lo.y, hi.z),
        v3(hi.x, lo.y, hi.z),
        v3(hi.x, hi.y, hi.z),
        v3(lo.x, hi.y, hi.z),
    ];
    let mut m = Mesh::new();
    for idx in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        if open && idx == [4, 5, 6, 7] {
            continue;
        }
        let pts: Vec<DVec3> = idx.iter().map(|&i| p[i]).collect();
        m.add_face(&pts, &[]);
    }
    m
}

fn add_group(w: &mut MainWindow, g: Group) -> GroupId {
    let id = g.id;
    w.viewport.scene.doc.groups.push(g);
    w.viewport.scene.version += 1;
    id
}

fn cube(w: &mut MainWindow, x: f64, open: bool, name: &str) -> Group {
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, cube_mesh(x, open));
    Group::new(mesh, Some(name))
}

fn select(w: &mut MainWindow, g: GroupId) {
    w.viewport
        .scene
        .select([EntityRef::Group(g)], SelectMode::Replace);
}

fn properties(w: &MainWindow, g: GroupId) -> String {
    Properties::default().describe(w, &[EntityRef::Group(g)])
}

/// Right-click ▸ Print material: `(id, text, checked)` per entry.
fn submenu(w: &MainWindow) -> Vec<(String, String, bool)> {
    w.context_menu()
        .into_iter()
        .find_map(|i| match i {
            CtxItem::Menu(t, sub) if t == "Print material" => Some(
                sub.into_iter()
                    .filter_map(|s| match s {
                        CtxItem::Entry {
                            id, text, checked, ..
                        } => Some((id, text, checked == Some(true))),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn id_of(entries: &[(String, String, bool)], text: &str) -> String {
    entries
        .iter()
        .find(|(_, t, _)| t == text)
        .map(|(id, _, _)| id.clone())
        .unwrap_or_else(|| panic!("no {text} in {entries:?}"))
}

#[test]
fn test_a_20_mm_pla_cube_shows_9_92_g_then_10_16_g_in_petg_and_undo_returns() {
    let (mut w, _) = win();
    let g = cube(&mut w, 0., false, "Cube");
    let g = add_group(&mut w, g);
    let text = properties(&w, g);
    assert!(text.contains("Print material: PLA (document)"), "{text}");
    assert!(text.contains("Mass (100 % infill): 9.92 g"), "{text}");

    select(&mut w, g);
    let entries = submenu(&w);
    let petg = id_of(&entries, "PETG");
    w.trigger(&petg);
    let text = properties(&w, g);
    assert!(text.contains("Print material: PETG<br>"), "{text}");
    assert!(text.contains("Mass (100 % infill): 10.16 g"), "{text}");

    w.trigger("undo");
    let text = properties(&w, g);
    assert!(text.contains("Print material: PLA (document)"), "{text}");
    assert!(text.contains("Mass (100 % infill): 9.92 g"), "{text}");
}

#[test]
fn test_a_group_that_is_not_closed_has_no_mass() {
    let (mut w, _) = win();
    let g = cube(&mut w, 0., true, "Open");
    let g = add_group(&mut w, g);
    let text = properties(&w, g);
    assert!(text.contains("Not a closed solid: no mass"), "{text}");
    assert!(!text.contains("Mass"), "{text}");
}

#[test]
fn test_a_part_of_several_bodies_shows_their_mass_and_its_bodies_its_material() {
    let (mut w, _) = win();
    let a = cube(&mut w, 0., false, "A");
    let b = cube(&mut w, 0.05, false, "B");
    let body = b.id;
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, Mesh::new());
    let mut part = Group::new(mesh, Some("Arm"));
    part.adopt(vec![a, b]);
    part.print_material = PrintMaterial::built_in("PETG");
    let part = add_group(&mut w, part);
    let text = properties(&w, part);
    assert!(text.contains("Mass (100 % infill): 20.32 g"), "{text}");
    let text = properties(&w, body);
    assert!(text.contains("Print material: PETG (from Arm)"), "{text}");
}

#[test]
fn test_the_submenu_lists_the_built_ins_custom_and_the_default() {
    let (mut w, _) = win();
    let g = cube(&mut w, 0., false, "Cube");
    let g = add_group(&mut w, g);
    // nothing selected, or no group: no submenu
    assert!(submenu(&w).is_empty());
    select(&mut w, g);
    let entries = submenu(&w);
    let texts: Vec<&str> = entries.iter().map(|(_, t, _)| t.as_str()).collect();
    assert_eq!(
        texts,
        [
            "PLA",
            "PETG",
            "ABS",
            "ASA",
            "TPU",
            "PA (nylon)",
            "PC",
            "Custom…",
            "Use default (PLA)"
        ]
    );
    // no own material: Use default is ticked
    let ticked: Vec<&str> = entries
        .iter()
        .filter(|(_, _, c)| *c)
        .map(|(_, t, _)| t.as_str())
        .collect();
    assert_eq!(ticked, ["Use default (PLA)"]);
    w.trigger(&id_of(&entries, "ABS"));
    let entries = submenu(&w);
    let ticked: Vec<&str> = entries
        .iter()
        .filter(|(_, _, c)| *c)
        .map(|(_, t, _)| t.as_str())
        .collect();
    assert_eq!(ticked, ["ABS"]);
    // Use default clears it
    w.trigger(&id_of(&entries, "Use default (PLA)"));
    assert_eq!(
        w.viewport.scene.doc.find_group(g).unwrap().print_material,
        None
    );
}

#[test]
fn test_custom_asks_a_name_and_a_density_in_g_per_cm3() {
    let (mut w, dialogs) = win();
    let g = cube(&mut w, 0., false, "Cube");
    let g = add_group(&mut w, g);
    select(&mut w, g);
    dialogs.borrow_mut().texts = vec![Some("  Resin ".into())];
    dialogs.borrow_mut().numbers = vec![Some(1.1)];
    w.trigger(&id_of(&submenu(&w), "Custom…"));
    assert_eq!(
        print_material_of(&w.viewport.scene.doc, g),
        &PrintMaterial::new("Resin", 1100.0)
    );
    let text = properties(&w, g);
    assert!(text.contains("Mass (100 % infill): 8.80 g"), "{text}");
    // the entry names it, ticked
    let entries = submenu(&w);
    assert!(entries.contains(&(
        "print_material:custom".into(),
        "Custom: Resin…".into(),
        true
    )));
    // cancelled: nothing changes
    let undo = w.viewport.history.undo_len();
    dialogs.borrow_mut().texts = vec![None];
    w.trigger("print_material:custom");
    assert_eq!(w.viewport.history.undo_len(), undo);
}

#[test]
fn test_a_component_copy_gives_its_material_to_every_copy() {
    let (mut w, _) = win();
    let mesh = add_mesh(&mut w.viewport.scene.doc.meshes, cube_mesh(0., false));
    let mut a = Group::new(mesh, Some("Bracket"));
    a.xform = Some(glam::DMat4::IDENTITY);
    let mut b = Group::new(mesh, Some("Bracket"));
    b.xform = Some(glam::DMat4::from_translation(DVec3::X * 0.05));
    let (a, b) = (add_group(&mut w, a), add_group(&mut w, b));
    select(&mut w, b);
    w.trigger(&id_of(&submenu(&w), "PC"));
    assert_eq!(print_material_of(&w.viewport.scene.doc, a).name, "PC");
    let text = properties(&w, a);
    assert!(text.contains("Print material: PC<br>"), "{text}");
}

#[test]
fn test_the_document_print_material_is_asked_from_the_model_menu() {
    let (mut w, dialogs) = win();
    let g = cube(&mut w, 0., false, "Cube");
    let g = add_group(&mut w, g);
    // the list: the built-ins, then Custom…; PETG picked
    dialogs.borrow_mut().choices = vec![Some(1)];
    w.trigger("document_print_material");
    assert_eq!(w.viewport.scene.doc.print_material.name, "PETG");
    let text = properties(&w, g);
    assert!(text.contains("Print material: PETG (document)"), "{text}");
    assert!(text.contains("10.16 g"), "{text}");
    // Custom…, the eighth entry
    dialogs.borrow_mut().choices = vec![Some(7)];
    dialogs.borrow_mut().texts = vec![Some("CF nylon".into())];
    dialogs.borrow_mut().numbers = vec![Some(1.2)];
    w.trigger("document_print_material");
    assert_eq!(
        w.viewport.scene.doc.print_material,
        PrintMaterial::new("CF nylon", 1200.0)
    );
    w.trigger("undo");
    assert_eq!(w.viewport.scene.doc.print_material.name, "PETG");
    // a group of its own keeps it
    select(&mut w, g);
    w.trigger(&id_of(&submenu(&w), "TPU"));
    dialogs.borrow_mut().choices = vec![Some(2)];
    w.trigger("document_print_material");
    assert_eq!(print_material_of(&w.viewport.scene.doc, g).name, "TPU");
    // the default entry names the document's
    assert!(submenu(&w).iter().any(|(_, t, _)| t == "Use default (ABS)"));
}

#[test]
fn test_an_imperial_document_shows_pounds() {
    let (mut w, _) = win();
    w.viewport.scene.apply_units(&Units::new("in", 3));
    let g = cube(&mut w, 0., false, "Cube");
    let g = add_group(&mut w, g);
    let text = properties(&w, g);
    // 9.92 g
    assert!(text.contains("Mass (100 % infill): 0.022 lb"), "{text}");
}
