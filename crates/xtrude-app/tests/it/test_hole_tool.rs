// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hole: click a face of a solid and a round hole is cut into it, square
//! to the face — through or blind, with an optional countersink or
//! counterbore; `M3` / `M3c` type a clearance hole. The solid stays
//! closed; one undo step; a hole that cannot be is refused with the
//! reason.

use crate::common;

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use common::{add_box, sized_viewport, v3, FakeProbe};
use glam::DVec3;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::hole::{hole_spec, set_hole_spec, HoleTool};
use xtrude_app::tools::{ToolContext, ToolValue};
use xtrude_app::viewport::{FaceHit, Viewport, ViewportEvent};
use xtrude_app::window::hole_form::{fields, spec_from};
use xtrude_app::window::ScriptedDialogs;
use xtrude_core::hole::{Depth, Head, HoleSpec};
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::FaceId;
use xtrude_ext::ParamValue;
use xtrude_model::ids::MeshId;

const MM: f64 = 0.001;

fn spec(diameter_mm: f64, depth: Depth, head: Head) -> HoleSpec {
    HoleSpec {
        diameter: diameter_mm * MM,
        depth,
        head,
    }
}

/// A 20 × 20 mm plate, 5 mm thick, with the Hole tool.
fn plate() -> Viewport {
    let mut vp = sized_viewport(1000.0, 700.0);
    add_box(&mut vp, DVec3::ZERO, v3(20., 20., 5.) * MM);
    vp.set_active_tool(Some(Box::new(HoleTool::new())));
    vp
}

fn top(vp: &Viewport) -> FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_normal(f).z > 0.5)
        .unwrap()
}

/// Click `face` at `at`, as if it were under the cursor.
fn click(vp: &mut Viewport, face: FaceId, at: DVec3) {
    FakeProbe::of(vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face,
        placement: None,
        xform: None,
        owner: None,
    }));
    let c = ToolContext::at(at);
    vp.with_tool(|t, vp| t.on_hover(vp, &c));
    vp.with_tool(|t, vp| t.on_click(vp, &c));
}

/// Drill `s` in the middle of the plate's top.
fn drill(vp: &mut Viewport, s: HoleSpec) {
    set_hole_spec(s);
    let f = top(vp);
    click(vp, f, v3(10., 10., 5.) * MM);
}

/// The vertices `r` mm from the hole's axis (x = y = 10 mm), by height in
/// µm.
fn ring_heights(vp: &Viewport, r: f64) -> Vec<i64> {
    let mut zs: Vec<i64> = vp
        .scene
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| v.position / MM)
        .filter(|p| ((p.x - 10.0).hypot(p.y - 10.0) - r).abs() < 1e-6)
        .map(|p| (p.z * 1000.0).round() as i64)
        .collect();
    zs.sort();
    zs.dedup();
    zs
}

/// How many vertices lie `r` mm from the axis at height `z` mm.
fn ring_count(vp: &Viewport, r: f64, z: f64) -> usize {
    vp.scene
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| v.position / MM)
        .filter(|p| ((p.x - 10.0).hypot(p.y - 10.0) - r).abs() < 1e-6 && (p.z - z).abs() < 1e-6)
        .count()
}

fn volume_mm3(vp: &Viewport) -> f64 {
    signed_volume(vp.scene.mesh()).abs() / MM.powi(3)
}

/// The area of an `n`-gon on a circle of radius `r`.
fn polygon_area(r: f64, n: usize) -> f64 {
    0.5 * n as f64 * r * r * (2.0 * PI / n as f64).sin()
}

fn flashes(vp: &mut Viewport) -> Vec<String> {
    vp.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn test_an_m3_through_a_5_mm_plate() {
    let mut vp = plate();
    set_hole_spec(HoleSpec {
        diameter: 1.0 * MM,
        ..HoleSpec::default()
    });
    let depth = vp.history.undo_len();
    assert!(vp
        .with_tool(|t, vp| t.on_value(
            vp,
            &ToolValue::Screw {
                nominal: 3.0,
                close: false
            }
        ))
        .unwrap());
    assert_eq!(hole_spec().diameter, 3.4 * MM);
    assert_eq!(hole_spec().depth, Depth::Through);
    let f = top(&vp);
    click(&mut vp, f, v3(10., 10., 5.) * MM);
    assert_eq!(vp.history.undo_len(), depth + 1);
    assert!(is_closed(vp.scene.mesh()), "the solid stays closed");
    // a 3.4 mm hole, round enough: at least 24 sides, top and bottom
    let n = ring_count(&vp, 1.7, 5.0);
    assert!(n >= 24, "{n} sides");
    assert_eq!(ring_count(&vp, 1.7, 0.0), n);
    assert_eq!(ring_heights(&vp, 1.7), vec![0, 5000]);
    let v = volume_mm3(&vp);
    let expected = 20.0 * 20.0 * 5.0 - polygon_area(1.7, n) * 5.0;
    assert!((v - expected).abs() < 1e-3, "{v} vs {expected}");
    // the hole's circles are curves
    let m = vp.scene.mesh();
    let curved = m.edges.iter().filter(|(_, e)| e.curve.is_some()).count();
    assert_eq!(curved, 2 * n);
    // one undo step takes it back to the plate
    vp.undo();
    assert_eq!(vp.scene.mesh().face_ids().len(), 6);
}

#[test]
fn test_a_close_fit_and_a_typed_diameter_and_depth() {
    let mut vp = plate();
    let screw = ToolValue::Screw {
        nominal: 3.0,
        close: true,
    };
    vp.with_tool(|t, vp| t.on_value(vp, &screw));
    assert_eq!(hole_spec().diameter, 3.2 * MM);
    vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Pair(2.5 * MM, 3.0 * MM)));
    assert_eq!(hole_spec().diameter, 2.5 * MM);
    assert_eq!(hole_spec().depth, Depth::Blind(3.0 * MM));
    // a depth of 0 is through
    vp.with_tool(|t, vp| t.on_value(vp, &ToolValue::Pair(2.5 * MM, 0.0)));
    assert_eq!(hole_spec().depth, Depth::Through);
    // a size the table does not hold changes nothing
    let m7 = ToolValue::Screw {
        nominal: 7.0,
        close: false,
    };
    assert!(!vp.with_tool(|t, vp| t.on_value(vp, &m7)).unwrap());
    assert_eq!(hole_spec().diameter, 2.5 * MM);
}

#[test]
fn test_a_blind_hole_keeps_its_floor() {
    let mut vp = plate();
    drill(&mut vp, spec(3.4, Depth::Blind(3.0 * MM), Head::None));
    assert!(is_closed(vp.scene.mesh()));
    assert_eq!(ring_heights(&vp, 1.7), vec![2000, 5000]);
    // the bottom of the plate is untouched
    let m = vp.scene.mesh();
    let bottom: Vec<FaceId> = m
        .face_ids()
        .into_iter()
        .filter(|&f| m.face_positions(f).iter().all(|p| p.z.abs() < 1e-9))
        .collect();
    assert_eq!(bottom.len(), 1);
    assert!(m.face(bottom[0]).holes.is_empty());
}

#[test]
fn test_a_countersink_is_as_deep_as_its_cone() {
    let mut vp = plate();
    let head = Head::Countersink {
        diameter: 6.4 * MM,
        angle_deg: 90.0,
    };
    drill(&mut vp, spec(3.4, Depth::Through, head));
    assert!(is_closed(vp.scene.mesh()));
    // the cone runs from Ø6.4 at the face to the bore 1.5 mm down
    assert_eq!(ring_heights(&vp, 3.2), vec![5000]);
    assert_eq!(ring_heights(&vp, 1.7), vec![0, 3500]);
}

#[test]
fn test_a_counterbore_steps_down_to_the_bore() {
    let mut vp = plate();
    let head = Head::Counterbore {
        diameter: 6.0 * MM,
        depth: 2.0 * MM,
    };
    drill(&mut vp, spec(3.4, Depth::Through, head));
    assert!(is_closed(vp.scene.mesh()));
    assert_eq!(ring_heights(&vp, 3.0), vec![3000, 5000]);
    assert_eq!(ring_heights(&vp, 1.7), vec![0, 3000]);
}

#[test]
fn test_a_blind_hole_past_the_far_side_is_refused() {
    let mut vp = plate();
    let depth = vp.history.undo_len();
    vp.take_events();
    drill(&mut vp, spec(3.4, Depth::Blind(6.0 * MM), Head::None));
    assert_eq!(vp.history.undo_len(), depth);
    assert_eq!(vp.scene.mesh().face_ids().len(), 6);
    let said = flashes(&mut vp);
    assert!(said.iter().any(|t| t.contains("far side")), "{said:?}");
    // the preview says so too, before the click
    let f = top(&vp);
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
    let c = ToolContext::at(v3(10., 10., 5.) * MM);
    vp.with_tool(|t, vp| t.on_hover(vp, &c));
    let (label, _) = vp.with_tool(|t, vp| t.value_label(vp)).unwrap().unwrap();
    assert!(label.contains("far side"), "{label}");
    assert!(vp
        .with_tool(|t, vp| t.rubber_band_lines(vp))
        .unwrap()
        .is_empty());
}

#[test]
fn test_a_head_deeper_than_the_plate_is_refused() {
    let mut vp = plate();
    let depth = vp.history.undo_len();
    let head = Head::Countersink {
        diameter: 16.0 * MM,
        angle_deg: 90.0,
    };
    drill(&mut vp, spec(3.4, Depth::Through, head));
    assert_eq!(vp.history.undo_len(), depth);
    let head = Head::Countersink {
        diameter: 3.0 * MM,
        angle_deg: 90.0,
    };
    vp.take_events();
    drill(&mut vp, spec(3.4, Depth::Through, head));
    assert_eq!(vp.history.undo_len(), depth);
    let said = flashes(&mut vp);
    assert!(said.iter().any(|t| t.contains("wider")), "{said:?}");
}

#[test]
fn test_a_face_that_is_not_a_solid_is_refused() {
    let mut vp = sized_viewport(1000.0, 700.0);
    let quad = [
        v3(0., 0., 0.),
        v3(20., 0., 0.),
        v3(20., 20., 0.),
        v3(0., 20., 0.),
    ]
    .map(|p| p * MM);
    let f = vp.scene.mesh_mut().add_face(&quad, &[]);
    vp.scene.version += 1;
    vp.set_active_tool(Some(Box::new(HoleTool::new())));
    let depth = vp.history.undo_len();
    vp.take_events();
    set_hole_spec(HoleSpec::default());
    click(&mut vp, f, v3(10., 10., 0.) * MM);
    assert_eq!(vp.history.undo_len(), depth);
    let said = flashes(&mut vp);
    assert!(said.iter().any(|t| t.contains("closed solid")), "{said:?}");
}

#[test]
fn test_the_preview_shows_the_hole_where_it_would_go() {
    let mut vp = plate();
    set_hole_spec(spec(3.4, Depth::Through, Head::None));
    let f = top(&vp);
    FakeProbe::of(&mut vp).pick(Some(FaceHit {
        mesh: MeshId::LOOSE,
        face: f,
        placement: None,
        xform: None,
        owner: None,
    }));
    let c = ToolContext::at(v3(10., 10., 5.) * MM);
    vp.with_tool(|t, vp| t.on_hover(vp, &c));
    let lines = vp.with_tool(|t, vp| t.rubber_band_lines(vp)).unwrap();
    assert!(!lines.is_empty());
    // within the plate's height, around the axis
    for (a, b) in lines {
        for p in [a, b] {
            let p = p / MM;
            assert!(p.z > -1.0 && p.z < 5.0 + 1e-9, "{p}");
            assert!(((p.x - 10.0).hypot(p.y - 10.0) - 1.7).abs() < 1e-6);
        }
    }
}

fn key(w: &mut xtrude_app::window::MainWindow, k: Key, text: &str) {
    w.key_press(&KeyEvent::new(k, text, Modifiers::NONE));
}

#[test]
fn test_typing_m3c_in_the_window_sets_the_close_fit() {
    let mut w = common::window();
    set_hole_spec(HoleSpec::default());
    w.trigger("hole");
    assert_eq!(w.active_tool_key(), Some("hole"));
    key(&mut w, Key::Char('M'), "m");
    // M is the Move tool's shortcut, but here it starts a screw size
    assert_eq!(w.active_tool_key(), Some("hole"));
    key(&mut w, Key::Char('3'), "3");
    key(&mut w, Key::Char('C'), "c");
    assert_eq!(w.viewport.value_box.buffer, "M3c");
    key(&mut w, Key::Return, "");
    assert_eq!(hole_spec().diameter, 3.2 * MM);
    assert_eq!(w.active_tool_key(), Some("hole"));
    // outside the Hole tool, M is still Move
    w.trigger("select");
    key(&mut w, Key::Char('M'), "m");
    assert_eq!(w.active_tool_key(), Some("move"));
}

#[test]
fn test_tab_asks_for_the_whole_hole() {
    let mut w = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    set_hole_spec(HoleSpec::default());
    w.trigger("hole");
    // in mm: Ø4.5, 0 deep (through), a counterbore Ø8 × 4.4
    dialogs.borrow_mut().forms.push(Some(vec![
        ParamValue::Length(4.5),
        ParamValue::Length(0.0),
        ParamValue::Choice(2),
        ParamValue::Length(8.0),
        ParamValue::Number(90.0),
        ParamValue::Length(4.4),
    ]));
    key(&mut w, Key::Tab, "");
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked, vec![("form".to_string(), "Hole".to_string())]);
    let s = hole_spec();
    assert!((s.diameter - 4.5 * MM).abs() < 1e-12);
    assert_eq!(s.depth, Depth::Through);
    let Head::Counterbore { diameter, depth } = s.head else {
        panic!("{:?}", s.head);
    };
    assert!((diameter - 8.0 * MM).abs() < 1e-12 && (depth - 4.4 * MM).abs() < 1e-12);
    // the form offered what the hole was: Ø3.4 mm, through, no head
    let offered = dialogs.borrow().form_values[0].clone();
    assert_eq!(offered[2], ParamValue::Choice(0));
    let ParamValue::Length(d) = offered[0] else {
        panic!("{:?}", offered[0]);
    };
    assert!((d - 3.4).abs() < 1e-9);
}

/// Equal within rounding.
fn same(a: &HoleSpec, b: &HoleSpec) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() < 1e-12;
    let depth = match (a.depth, b.depth) {
        (Depth::Through, Depth::Through) => true,
        (Depth::Blind(x), Depth::Blind(y)) => near(x, y),
        _ => false,
    };
    let head = match (a.head, b.head) {
        (Head::None, Head::None) => true,
        (
            Head::Countersink {
                diameter: d1,
                angle_deg: a1,
            },
            Head::Countersink {
                diameter: d2,
                angle_deg: a2,
            },
        ) => near(d1, d2) && near(a1, a2),
        (
            Head::Counterbore {
                diameter: d1,
                depth: h1,
            },
            Head::Counterbore {
                diameter: d2,
                depth: h2,
            },
        ) => near(d1, d2) && near(h1, h2),
        _ => false,
    };
    near(a.diameter, b.diameter) && depth && head
}

#[test]
fn test_the_form_reads_back_what_it_offers() {
    for s in [
        spec(3.4, Depth::Through, Head::None),
        spec(
            2.9,
            Depth::Blind(4.0 * MM),
            Head::Countersink {
                diameter: 5.5 * MM,
                angle_deg: 82.0,
            },
        ),
        spec(
            6.6,
            Depth::Through,
            Head::Counterbore {
                diameter: 11.0 * MM,
                depth: 6.4 * MM,
            },
        ),
    ] {
        let values: Vec<ParamValue> = fields(&s, MM).into_iter().map(|f| f.default).collect();
        let back = spec_from(&values, MM).unwrap();
        assert!(same(&back, &s), "{back:?} vs {s:?}");
    }
}

#[test]
fn test_the_window_makes_it_after_push_pull() {
    let t = xtrude_app::window::make_tool("hole").expect("a Hole tool");
    assert_eq!(t.info().name, "Hole");
    assert_eq!(t.info().icon, Some("hole"));
}
