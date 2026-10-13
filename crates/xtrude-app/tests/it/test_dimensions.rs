// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The entity's geometry, the commands, the tool's 3-click placement,
//! occlusion, custom text and linear (axis) dimensions. The document round trips
//! live in crates/xtrude-io.

use crate::common;
use crate::common::FakeProbe;
use xtrude_core::Axis;
use xtrude_model::dimension::DimensionStyle;

use common::{stub, v3, MetreDocument};
use glam::DVec3;
use serde_json::{json, Value};
use xtrude_app::input::Key;
use xtrude_app::input::Modifiers;
use xtrude_app::tools::dimension::DimensionTool;
use xtrude_app::tools::select::SelectTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{format_dim_value, Viewport};
use xtrude_core::Mesh;
use xtrude_model::commands::{
    AddDimensionCommand, DeleteDimensionsCommand, EditDimensionTextCommand,
};
use xtrude_model::dimension::Dimension;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::ids::{MeshId, ObjId};
use xtrude_model::style::Style;

fn dv(vp: &Viewport) -> xtrude_model::dimension::DocView<'_> {
    vp.scene.doc.doc_view(MeshId::LOOSE)
}

fn dim(vp: &Viewport, id: ObjId) -> &Dimension {
    vp.scene.doc.dimension(id).expect("the dimension")
}

// ---- Entity ----------------------------------------------------------------

#[test]
fn test_dimension_value_and_line_points() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    let d = Dimension::new(v3(0., 0., 0.), v3(4., 0., 0.), v3(0., 0., 2.));
    let id = d.id;
    vp.scene.doc.dimensions.push(d);
    let doc = dv(&vp);
    let d = dim(&vp, id);
    assert_eq!(d.value(&doc), 4.0);
    assert_eq!(d.label(&doc), "4.00 m");
    let (ap, bp) = d.line_points(&doc);
    assert!(ap == v3(0., 0., 2.) && bp == v3(4., 0., 2.));
    assert_eq!(d.midpoint(&doc), v3(2., 0., 2.));
}

#[test]
fn test_offset_for_cursor_is_perpendicular() {
    // Cursor above the +X segment at (2, 0, 3): only the vertical part.
    let off = Dimension::offset_for_cursor(v3(0., 0., 0.), v3(4., 0., 0.), v3(2., 0., 3.));
    assert_eq!(off, v3(0., 0., 3.));
}

// ---- Commands --------------------------------------------------------------

#[test]
fn test_add_and_delete_dimension_commands() {
    let mut vp = stub();
    let d = Dimension::new(v3(0., 0., 0.), v3(4., 0., 0.), v3(0., 0., 2.));
    let id = d.id;
    vp.execute(&mut AddDimensionCommand::new(d.clone()));
    assert_eq!(vp.scene.doc.dimensions, vec![d.clone()]);
    assert!(vp.undo());
    assert!(vp.scene.doc.dimensions.is_empty());
    assert!(vp.redo());
    assert_eq!(vp.scene.doc.dimensions, vec![d.clone()]);
    vp.execute(&mut DeleteDimensionsCommand { ids: vec![id] });
    assert!(vp.scene.doc.dimensions.is_empty());
    assert!(vp.undo());
    assert_eq!(vp.scene.doc.dimensions, vec![d]); // restored at its index
}

// ---- Tool ------------------------------------------------------------------

fn ctx(p: DVec3) -> ToolContext {
    ToolContext::at(p)
}

#[test]
fn test_dimension_tool_three_clicks_place_a_dimension() {
    let mut vp = stub();
    let mut tool = DimensionTool::new();
    tool.on_click(&mut vp, &ctx(v3(0., 0., 0.))); // first endpoint
    assert!(vp.scene.doc.dimensions.is_empty());
    tool.on_click(&mut vp, &ctx(v3(4., 0., 0.))); // second endpoint
    assert!(vp.scene.doc.dimensions.is_empty()); // still placing the offset
    tool.on_click(&mut vp, &ctx(v3(2., 0., 3.))); // offset placement
    assert_eq!(vp.scene.doc.dimensions.len(), 1);
    let d = vp.scene.doc.dimensions[0].clone();
    assert_eq!(d.value(&dv(&vp)), 4.0);
    assert_eq!(d.offset, v3(0., 0., 3.));
    assert!(tool.a.is_none()); // reset for the next dimension
}

#[test]
fn test_dimension_tool_preview_lines() {
    let mut vp = stub();
    let mut tool = DimensionTool::new();
    tool.on_click(&mut vp, &ctx(v3(0., 0., 0.)));
    tool.on_hover(&mut vp, &ctx(v3(4., 0., 0.)));
    assert_eq!(
        tool.rubber_band_lines(&vp),
        vec![(v3(0., 0., 0.), v3(4., 0., 0.))]
    ); // measuring
    tool.on_click(&mut vp, &ctx(v3(4., 0., 0.)));
    tool.on_hover(&mut vp, &ctx(v3(2., 0., 3.)));
    assert_eq!(tool.rubber_band_lines(&vp).len(), 3); // 2 extension + 1 dimension line
}

#[test]
fn test_select_tool_deletes_a_dimension() {
    let mut vp = stub();
    let d = Dimension::new(v3(0., 0., 0.), v3(4., 0., 0.), v3(0., 0., 2.));
    let id = d.id;
    vp.execute(&mut AddDimensionCommand::new(d));
    vp.scene.selection.insert(EntityRef::Dimension(id));
    let mut tool = SelectTool::new();
    tool.on_key(&mut vp, Key::Delete, Modifiers::NONE);
    assert!(vp.scene.doc.dimension(id).is_none());
    assert!(!vp.scene.selection.contains(&EntityRef::Dimension(id)));
    assert!(vp.undo());
    assert!(vp.scene.doc.dimension(id).is_some()); // undo brings it back
}

// ---- Style (precision / units) ---------------------------------------------

fn style(v: Value) -> DimensionStyle {
    let mut s = DimensionStyle::default();
    s.merge_json(v.as_object().unwrap());
    s
}

#[test]
fn test_format_dim_value_respects_units_and_precision() {
    assert_eq!(
        format_dim_value(4.0, &style(json!({"units": "m", "decimals": 2}))),
        "4.00 m"
    );
    assert_eq!(
        format_dim_value(4.0, &style(json!({"units": "cm", "decimals": 0}))),
        "400 cm"
    );
    assert_eq!(
        format_dim_value(4.0, &style(json!({"units": "mm", "decimals": 1}))),
        "4000.0 mm"
    );
}

// ---- Occlusion ---------------------------------------------------------------

fn wall(m: &mut Mesh, y: f64) {
    m.add_face(
        &[
            v3(-2., y, -2.),
            v3(2., y, -2.),
            v3(2., y, 2.),
            v3(-2., y, 2.),
        ],
        &[],
    );
}

/// A stand-in whose eye is at (0, -10, 0).
fn eyed() -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).eye(Some(v3(0., -10., 0.)));
    vp
}

#[test]
fn test_is_occluded_matches_geometry() {
    let mut vp = eyed();
    wall(vp.scene.mesh_mut(), 0.0);
    vp.scene.version += 1;
    assert!(vp.is_occluded(v3(0., 5., 0.))); // behind the wall
    assert!(!vp.is_occluded(v3(0., -5., 0.))); // in front of it
    assert!(!vp.is_occluded(v3(5., 5., 0.))); // past the wall's edge
    assert!(!vp.is_occluded(v3(0., 0., 0.))); // ON the wall (epsilon)
    wall(vp.scene.mesh_mut(), 2.0);
    vp.scene.version += 1; // cache must refresh
    assert!(vp.is_occluded(v3(0., 5., 0.)));
    assert!(vp.is_occluded(v3(0., 1., 0.))); // behind wall 1 only
}

#[test]
fn test_xray_and_wireframe_let_snaps_through_faces() {
    let mut vp = eyed();
    wall(vp.scene.mesh_mut(), 0.0);
    vp.scene.version += 1;
    let behind = v3(0., 5., 0.);
    assert!(vp.is_occluded(behind)); // default style
    for (name, mode, hidden) in [
        ("X-ray", "xray", false),
        ("Wireframe", "wireframe", false),
        ("Hidden line", "hidden_line", true),
    ] {
        vp.scene.display_style = Style {
            face_mode: mode.into(),
            ..Style::named(name)
        };
        assert_eq!(vp.is_occluded(behind), hidden, "{name}");
    }
}

#[test]
fn test_geometry_inside_a_group_occludes_too() {
    let mut vp = eyed();
    let mut m = Mesh::new();
    wall(&mut m, 0.0);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    vp.scene.doc.groups.push(Group::new(mid, Some("Shell")));
    vp.scene.version += 1;
    assert!(vp.scene.mesh().faces.is_empty()); // nothing loose at all
    assert!(vp.is_occluded(v3(0., 5., 0.))); // behind the grouped wall
    assert!(!vp.is_occluded(v3(0., -5., 0.))); // in front of it
    assert!(!vp.is_occluded(v3(5., 5., 0.))); // past its edge
}

#[test]
fn test_world_under_cursor_uses_cached_pick() {
    let mut vp = eyed();
    wall(vp.scene.mesh_mut(), 0.0);
    wall(vp.scene.mesh_mut(), 3.0);
    vp.scene.version += 1;
    FakeProbe::of(&mut vp).pixel_to_ray(|_, _| Some((v3(0., -10., 0.), v3(0., 1., 0.))));
    let hit = vp.world_under_cursor(0., 0.).unwrap();
    assert!(hit.y.abs() < 1e-6); // nearest wall, not the far one
    vp.scene.mesh_mut().clear();
    vp.scene.version += 1; // cache refresh on scene change
    FakeProbe::of(&mut vp).pixel_to_ray(|_, _| Some((v3(0., 0., 10.), v3(0., 0., -1.))));
    let ground = vp.world_under_cursor(0., 0.).unwrap();
    assert!(ground.z.abs() < 1e-6); // falls back to the ground plane
}

// ---- Custom dimension text -----------------------------------------------------

#[test]
fn test_dimension_custom_text_and_placeholder() {
    let mut d = Dimension::new(v3(0., 0., 0.), v3(2.4, 0., 0.), v3(0., 1., 0.));
    assert_eq!(d.display_text("2.40 m"), "2.40 m"); // automatic
    d.text = Some("H = <> (check)".into());
    assert_eq!(d.display_text("2.40 m"), "H = 2.40 m (check)");
    d.text = Some("VARIABLE".into());
    assert_eq!(d.display_text("2.40 m"), "VARIABLE");
}

#[test]
fn test_edit_dimension_text_command_undo_redo() {
    let mut vp = stub();
    let d = Dimension::new(v3(0., 0., 0.), v3(2., 0., 0.), v3(0., 1., 0.));
    let id = d.id;
    vp.scene.doc.dimensions.push(d);
    vp.execute(&mut EditDimensionTextCommand {
        id,
        text: Some("Ø <>".into()),
    });
    assert_eq!(dim(&vp, id).text.as_deref(), Some("Ø <>"));
    assert!(vp.undo());
    assert!(dim(&vp, id).text.is_none());
    assert!(vp.redo());
    assert_eq!(dim(&vp, id).text.as_deref(), Some("Ø <>"));
    vp.execute(&mut EditDimensionTextCommand { id, text: None }); // back to auto
    assert!(dim(&vp, id).text.is_none());
}

#[test]
fn test_select_double_click_edits_dimension_text() {
    let _m = MetreDocument::new();
    let mut vp = stub();
    let d = Dimension::new(v3(0., 0., 0.), v3(2.4, 0., 0.), v3(0., 1., 0.));
    let id = d.id;
    vp.scene.doc.dimensions.push(d);
    // The metre label, read through the dimension style:
    vp.scene.dimension_style = style(json!({"units": "m", "decimals": 2}));
    FakeProbe::of(&mut vp).pick_dimension(move |_, _| Some(id));
    FakeProbe::of(&mut vp).pick_group(|_, _| None);
    FakeProbe::of(&mut vp).pick_edge(|_, _| None);
    let asked = std::rc::Rc::new(std::cell::RefCell::new((String::new(), String::new())));
    let a2 = asked.clone();
    vp.prompt = Some(Box::new(move |_t, _l, prefill, _m| {
        let mut a = a2.borrow_mut();
        a.0 = prefill.to_string();
        Some(a.1.clone())
    }));
    asked.borrow_mut().1 = "H = <>".into();
    SelectTool::new().on_double_click(&mut vp, &ToolContext::at(DVec3::ZERO));
    assert_eq!(asked.borrow().0, "2.40 m"); // shows the value
    assert_eq!(dim(&vp, id).text.as_deref(), Some("H = <>"));
    assert_eq!(dim(&vp, id).display_text("2.40 m"), "H = 2.40 m");
    assert!(vp.undo());
    assert!(dim(&vp, id).text.is_none());
    assert!(vp.redo());
    asked.borrow_mut().1 = "<>".into(); // back to automatic
    SelectTool::new().on_double_click(&mut vp, &ToolContext::at(DVec3::ZERO));
    assert_eq!(asked.borrow().0, "H = <>");
    assert!(dim(&vp, id).text.is_none());
    // (the document round trip of the text: crates/xtrude-io test_dimensions)
}

// ---- Linear (axis) dimensions ------------------------------------------------

#[test]
fn test_linear_dimension_measures_the_extent_along_the_axis() {
    let (a, b) = (v3(0., 0., 0.), v3(4., 3., 0.));
    assert!((Dimension::value_of(a, b, Some(Axis::X)) - 4.0).abs() < 1e-6); // not the diagonal
    let (ap, bp) = Dimension::line_points_of(a, b, v3(0., 5., 0.), Some(Axis::X));
    assert!((ap - v3(0., 5., 0.)).length() < 1e-6 && (bp - v3(4., 5., 0.)).length() < 1e-6);
    assert!((Dimension::value_of(a, b, Some(Axis::Y)) - 3.0).abs() < 1e-6);
    let (ap, bp) = Dimension::line_points_of(a, b, v3(-2., 0., 0.), Some(Axis::Y));
    assert!((ap - v3(-2., 0., 0.)).length() < 1e-6 && (bp - v3(-2., 3., 0.)).length() < 1e-6);
}

#[test]
fn test_a_segment_along_the_axis_falls_back_to_aligned() {
    let (a, b) = (v3(0., 0., 0.), v3(4., 0., 0.));
    assert!((Dimension::value_of(a, b, Some(Axis::X)) - 4.0).abs() < 1e-6);
    assert_eq!(
        Dimension::line_points_of(a, b, v3(0., 2., 0.), Some(Axis::X)).0,
        v3(0., 2., 0.)
    );
}

#[test]
fn test_placement_reads_where_the_cursor_is_pulled() {
    let (a, b) = (v3(0., 0., 0.), v3(4., 3., 0.));
    let place = |a, b, c| Dimension::placement_for_cursor(a, b, c).1;
    assert_eq!(place(a, b, v3(-2., 1.5, 0.)), Some(Axis::Y)); // to the side → vertical extent
    assert_eq!(place(a, b, v3(2., 6., 0.)), Some(Axis::X)); // above → horizontal extent
    assert_eq!(place(a, b, v3(1.4, 2.3, 0.)), None); // square off it → aligned
    assert_eq!(place(a, b, v3(2. - 0.6 * 4., 1.5 + 0.8 * 4., 0.)), None); // however far
    assert_eq!(place(a, b, v3(-1., 6., 0.)), None); // the corner → aligned
    assert_eq!(place(v3(0., 0., 0.), v3(4., 0.5, 0.), v3(2., 3., 0.)), None);
    let (sa, sb) = (v3(0., 0., 0.), v3(2., 4., 0.));
    assert_eq!(
        place(sa, sb, v3(1. - 0.894 * 3., 2. + 0.447 * 3., 0.)),
        None
    );
    assert_eq!(place(sa, sb, v3(-3., 2., 0.)), Some(Axis::Y));
}

#[test]
fn test_dimension_tool_places_a_linear_dimension_when_pulled_to_the_side() {
    let mut vp = stub();
    let mut tool = DimensionTool::new();
    tool.on_activate(&mut vp);
    for p in [v3(0., 0., 0.), v3(4., 3., 0.), v3(-2., 1.5, 0.)] {
        tool.on_click(&mut vp, &ctx(p));
    }
    assert_eq!(vp.scene.doc.dimensions.len(), 1);
    let d = vp.scene.doc.dimensions[0].clone();
    assert!(d.axis == Some(Axis::Y) && (d.value(&dv(&vp)) - 3.0).abs() < 1e-6);
}
