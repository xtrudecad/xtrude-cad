// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Print materials: the built-in list, the lookup (own → the part's → the
//! document's), the mass of a group at 100 % infill, the commands and
//! their undo, and the mass readout.

use crate::common;
use common::*;
use xtrude_core::math::DMat4;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::group::{add_mesh, copy_group, Group};
use xtrude_model::ids::GroupId;
use xtrude_model::print_material::{
    default_print_material, group_mass, print_material_from, print_material_of, MaterialFrom,
    PrintMaterial, SetDocumentPrintMaterialCommand, SetPrintMaterialCommand, BUILT_IN,
};
use xtrude_model::scene::Scene;
use xtrude_model::units::{fmt_density, fmt_mass, Units, UnitsOverride};

const MM: f64 = 0.001;

/// A closed axis-aligned box from `lo`, `size` in metres.
fn cuboid(lo: DVec3, size: DVec3) -> Mesh {
    let (a, b, c) = (size.x, size.y, size.z);
    let p = |x: f64, y: f64, z: f64| lo + DVec3::new(x, y, z);
    let mut m = Mesh::new();
    for lp in [
        [p(0., 0., 0.), p(0., b, 0.), p(a, b, 0.), p(a, 0., 0.)],
        [p(0., 0., c), p(a, 0., c), p(a, b, c), p(0., b, c)],
        [p(0., 0., 0.), p(a, 0., 0.), p(a, 0., c), p(0., 0., c)],
        [p(a, 0., 0.), p(a, b, 0.), p(a, b, c), p(a, 0., c)],
        [p(a, b, 0.), p(0., b, 0.), p(0., b, c), p(a, b, c)],
        [p(0., b, 0.), p(0., 0., 0.), p(0., 0., c), p(0., b, c)],
    ] {
        m.add_face(&lp, &[]);
    }
    m
}

fn cube_group(scene: &mut Scene, at: DVec3, side_mm: f64) -> Group {
    let mesh = add_mesh(
        &mut scene.doc.meshes,
        cuboid(at, DVec3::splat(side_mm * MM)),
    );
    Group::new(mesh, Some("cube"))
}

fn add_top(scene: &mut Scene, g: Group) -> GroupId {
    let id = g.id;
    scene.doc.groups.push(g);
    scene.version += 1;
    id
}

fn petg() -> PrintMaterial {
    PrintMaterial::built_in("PETG").unwrap()
}

fn grams(kg: Option<f64>) -> f64 {
    kg.expect("a mass") * 1000.0
}

#[test]
fn test_the_built_in_materials_and_their_densities() {
    let names: Vec<&str> = BUILT_IN.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["PLA", "PETG", "ABS", "ASA", "TPU", "PA (nylon)", "PC"]
    );
    let g_cm3: Vec<f64> = BUILT_IN.iter().map(|(_, d)| d / 1000.0).collect();
    assert_eq!(g_cm3, [1.24, 1.27, 1.04, 1.07, 1.21, 1.14, 1.20]);
    assert_eq!(default_print_material(), PrintMaterial::new("PLA", 1240.0));
    assert!(petg().is_built_in());
    assert!(!PrintMaterial::new("PETG", 1300.0).is_built_in());
    assert!(!PrintMaterial::new("Resin", 0.0).is_valid());
    assert!(!PrintMaterial::new(" ", 1000.0).is_valid());
}

#[test]
fn test_a_new_document_prints_in_pla() {
    let (s, _) = scene_history();
    assert_eq!(s.doc.print_material, default_print_material());
}

#[test]
fn test_a_20_mm_cube_weighs_9_92_g_in_pla_and_10_16_g_in_petg_and_undo_returns() {
    let (mut s, mut h) = scene_history();
    let g = cube_group(&mut s, DVec3::ZERO, 20.0);
    let g = add_top(&mut s, g);
    assert!((grams(group_mass(&s.doc, g)) - 9.92).abs() < 1e-9);
    exec(
        &mut s,
        &mut h,
        Box::new(SetPrintMaterialCommand {
            groups: vec![g],
            material: Some(petg()),
        }),
    );
    assert_eq!(print_material_of(&s.doc, g).name, "PETG");
    assert!((grams(group_mass(&s.doc, g)) - 10.16).abs() < 1e-9);
    assert_eq!(h.undo_len(), 1);
    h.undo(&mut s);
    assert_eq!(print_material_of(&s.doc, g).name, "PLA");
    assert!((grams(group_mass(&s.doc, g)) - 9.92).abs() < 1e-9);
}

#[test]
fn test_use_default_clears_the_groups_own_material() {
    let (mut s, mut h) = scene_history();
    let mut g = cube_group(&mut s, DVec3::ZERO, 20.0);
    g.print_material = Some(petg());
    let g = add_top(&mut s, g);
    exec(
        &mut s,
        &mut h,
        Box::new(SetPrintMaterialCommand {
            groups: vec![g],
            material: None,
        }),
    );
    assert_eq!(s.doc.find_group(g).unwrap().print_material, None);
    assert_eq!(print_material_of(&s.doc, g).name, "PLA");
}

#[test]
fn test_the_document_material_is_one_undo_step() {
    let (mut s, mut h) = scene_history();
    let g = cube_group(&mut s, DVec3::ZERO, 20.0);
    let g = add_top(&mut s, g);
    exec(
        &mut s,
        &mut h,
        Box::new(SetDocumentPrintMaterialCommand {
            material: PrintMaterial::new("Resin", 1100.0),
        }),
    );
    assert_eq!(print_material_of(&s.doc, g).name, "Resin");
    assert!((grams(group_mass(&s.doc, g)) - 8.8).abs() < 1e-9);
    h.undo(&mut s);
    assert_eq!(s.doc.print_material, default_print_material());
}

#[test]
fn test_a_new_document_after_an_empty_one_prints_in_pla_again() {
    let (mut s, _) = scene_history();
    s.doc.print_material = petg();
    s.clear();
    assert_eq!(s.doc.print_material, default_print_material());
}

#[test]
fn test_a_nested_group_takes_its_own_then_its_parts_then_the_documents() {
    let (mut s, _) = scene_history();
    let inner = cube_group(&mut s, DVec3::ZERO, 10.0);
    let deep = cube_group(&mut s, DVec3::new(0.05, 0., 0.), 10.0);
    let (inner_id, deep_id) = (inner.id, deep.id);
    let mut middle = cube_group(&mut s, DVec3::new(0.1, 0., 0.), 10.0);
    middle.adopt(vec![deep]);
    let middle_id = middle.id;
    let mut part = Group::new(add_mesh(&mut s.doc.meshes, Mesh::new()), Some("part"));
    part.adopt(vec![inner, middle]);
    let part_id = add_top(&mut s, part);
    // nothing set: the document's
    assert_eq!(print_material_of(&s.doc, deep_id).name, "PLA");
    assert_eq!(
        print_material_from(&s.doc, deep_id).1,
        MaterialFrom::Document
    );
    // the part's reaches every nested group, at any depth
    s.doc.find_group_mut(part_id).unwrap().print_material = Some(petg());
    assert_eq!(print_material_of(&s.doc, inner_id).name, "PETG");
    assert_eq!(print_material_of(&s.doc, deep_id).name, "PETG");
    assert_eq!(
        print_material_from(&s.doc, deep_id).1,
        MaterialFrom::Part(part_id)
    );
    assert_eq!(print_material_from(&s.doc, part_id).1, MaterialFrom::Own);
    // an intermediate group's material is its own, not its children's:
    // they take their part's
    s.doc.find_group_mut(middle_id).unwrap().print_material = PrintMaterial::built_in("ABS");
    assert_eq!(print_material_of(&s.doc, middle_id).name, "ABS");
    assert_eq!(print_material_of(&s.doc, deep_id).name, "PETG");
    // its own first
    s.doc.find_group_mut(deep_id).unwrap().print_material = PrintMaterial::built_in("PC");
    assert_eq!(print_material_of(&s.doc, deep_id).name, "PC");
}

#[test]
fn test_a_part_made_of_several_bodies_weighs_their_sum() {
    let (mut s, _) = scene_history();
    let a = cube_group(&mut s, DVec3::ZERO, 20.0);
    let mut b = cube_group(&mut s, DVec3::new(0.05, 0., 0.), 10.0);
    b.print_material = Some(petg());
    let mut part = Group::new(add_mesh(&mut s.doc.meshes, Mesh::new()), Some("part"));
    part.adopt(vec![a, b]);
    let part = add_top(&mut s, part);
    // 8 cm³ of PLA and 1 cm³ of PETG
    assert!((grams(group_mass(&s.doc, part)) - (9.92 + 1.27)).abs() < 1e-9);
}

#[test]
fn test_an_open_body_has_no_mass() {
    let (mut s, _) = scene_history();
    let mut m = cuboid(DVec3::ZERO, DVec3::splat(0.02));
    let top = m.face_ids()[1];
    m.remove_face(top);
    let open = Group::new(add_mesh(&mut s.doc.meshes, m), Some("open"));
    let open_id = open.id;
    add_top(&mut s, open);
    assert_eq!(group_mass(&s.doc, open_id), None);
    // nor has a part that holds it
    let solid = cube_group(&mut s, DVec3::new(0.05, 0., 0.), 10.0);
    let open = s.doc.groups.pop().unwrap();
    let mut part = Group::new(add_mesh(&mut s.doc.meshes, Mesh::new()), Some("part"));
    part.adopt(vec![solid, open]);
    let part = add_top(&mut s, part);
    assert_eq!(group_mass(&s.doc, part), None);
    assert_eq!(group_mass(&s.doc, open_id), None);
    // an empty group neither
    let empty = Group::new(add_mesh(&mut s.doc.meshes, Mesh::new()), Some("empty"));
    let empty = add_top(&mut s, empty);
    assert_eq!(group_mass(&s.doc, empty), None);
}

#[test]
fn test_a_scaled_component_weighs_its_scaled_volume() {
    let (mut s, _) = scene_history();
    let mut c = cube_group(&mut s, DVec3::ZERO, 10.0);
    c.xform = Some(DMat4::from_scale(DVec3::new(2.0, 2.0, 2.0)));
    let c = add_top(&mut s, c);
    // a 20 mm cube
    assert!((grams(group_mass(&s.doc, c)) - 9.92).abs() < 1e-9);
}

#[test]
fn test_the_copies_of_a_component_share_its_material() {
    let (mut s, mut h) = scene_history();
    let mut c = cube_group(&mut s, DVec3::ZERO, 10.0);
    c.xform = Some(DMat4::IDENTITY);
    let copy = copy_group(
        &mut s.doc.meshes,
        &mut c.clone(),
        Some(DVec3::new(0.05, 0., 0.)),
        false,
    );
    let other = cube_group(&mut s, DVec3::new(0.1, 0., 0.), 10.0);
    let (c, copy, other) = (
        add_top(&mut s, c),
        add_top(&mut s, copy),
        add_top(&mut s, other),
    );
    exec(
        &mut s,
        &mut h,
        Box::new(SetPrintMaterialCommand {
            groups: vec![copy],
            material: Some(petg()),
        }),
    );
    assert_eq!(print_material_of(&s.doc, c).name, "PETG");
    assert_eq!(print_material_of(&s.doc, copy).name, "PETG");
    assert_eq!(print_material_of(&s.doc, other).name, "PLA");
    // a copy made now carries it
    let mut src = s.doc.find_group(c).unwrap().clone();
    let again = copy_group(&mut s.doc.meshes, &mut src, None, false);
    assert_eq!(again.print_material, Some(petg()));
}

#[test]
fn test_a_group_inside_a_component_reaches_the_other_copies() {
    let (mut s, mut h) = scene_history();
    let body = cube_group(&mut s, DVec3::ZERO, 10.0);
    let mut c = Group::new(add_mesh(&mut s.doc.meshes, Mesh::new()), Some("comp"));
    c.adopt(vec![body]);
    let copy = copy_group(
        &mut s.doc.meshes,
        &mut c,
        Some(DVec3::new(0.05, 0., 0.)),
        false,
    );
    let (c, copy) = (add_top(&mut s, c), add_top(&mut s, copy));
    let body = s.doc.find_group(c).unwrap().children[0].id;
    exec(
        &mut s,
        &mut h,
        Box::new(SetPrintMaterialCommand {
            groups: vec![body],
            material: Some(petg()),
        }),
    );
    let twin = s.doc.find_group(copy).unwrap().children[0].id;
    assert_eq!(print_material_of(&s.doc, body).name, "PETG");
    assert_eq!(print_material_of(&s.doc, twin).name, "PETG");
    assert!((grams(group_mass(&s.doc, copy)) - 1.27).abs() < 1e-9);
}

#[test]
fn test_mass_reads_in_grams_kilograms_or_pounds() {
    let _mm = UnitsOverride::with(Units::new("mm", 2));
    assert_eq!(fmt_mass(0.00992), "9.92 g");
    assert_eq!(fmt_mass(9.5), "9500.00 g");
    assert_eq!(fmt_mass(12.5), "12.50 kg");
    assert_eq!(fmt_density(1240.0), "1.24 g/cm³");
    drop(_mm);
    let _m = UnitsOverride::with(Units::new("m", 3));
    assert_eq!(fmt_mass(0.00992), "9.920 g");
    drop(_m);
    let _in = UnitsOverride::with(Units::new("in", 2));
    assert_eq!(fmt_mass(0.45359237), "1.00 lb");
    assert_eq!(fmt_density(27679.9047), "1.0000 lb/in³");
    drop(_in);
    let _ft = UnitsOverride::with(Units::new("ft-in", 3));
    assert_eq!(fmt_mass(0.45359237 * 2.5), "2.500 lb");
}
