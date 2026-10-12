// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! CAD linework arrives as tagged layer groups, curves as single contours,
//! units honoured, survey coordinates recentred. The drawings in
//! tests/data/dxf are frozen inputs: never regenerate or edit them.

use crate::common;
use common::*;
use std::collections::BTreeSet;
use xtrude_core::math::MatExt;
use xtrude_io::dxf_in::{
    load_dxf, open_document, read_unit_scale, suggest_unit_scale, DxfStats, RECENTER_BEYOND_M,
};
use xtrude_model::group::Group;
use xtrude_model::history::{CmdError, FnCommand, History};
use xtrude_model::scene::Scene;

fn dxf(name: &str) -> std::path::PathBuf {
    data(&format!("dxf/{name}"))
}

fn load(name: &str, scale: f64) -> (Scene, DxfStats) {
    let mut s = Scene::new();
    let stats = load_dxf(&mut s, &dxf(name), None, scale, None, None).unwrap();
    (s, stats)
}

fn edges(s: &Scene, g: &Group) -> Vec<(Option<i64>, bool)> {
    let m = s.doc.mesh(g.mesh);
    m.edge_ids()
        .iter()
        .map(|&e| (m.edge(e).curve, m.edge(e).soft))
        .collect()
}

#[test]
fn test_lines_and_polylines_become_edges() {
    let (s, stats) = load("lines_and_polylines.dxf", 1.0);
    assert_eq!(stats.groups, 1);
    assert_eq!(s.doc.mesh(s.doc.groups[0].mesh).edges.len(), 4);
    assert!(stats.skipped.is_empty());
}

#[test]
fn test_a_circle_is_one_selectable_contour() {
    let (s, _) = load("circle.dxf", 1.0);
    let e = edges(&s, &s.doc.groups[0]);
    let ids: BTreeSet<i64> = e.iter().filter_map(|x| x.0).collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(e.iter().filter(|x| x.0.is_none()).count(), 1);
    assert!(e.iter().filter(|x| x.0.is_some()).count() >= 8);
}

#[test]
fn test_a_bulged_polyline_counts_as_a_curve() {
    let (s, _) = load("bulged_polyline.dxf", 1.0);
    let e = edges(&s, &s.doc.groups[0]);
    let curved: Vec<_> = e.iter().filter(|x| x.0.is_some()).collect();
    assert!(curved.len() >= 4);
    assert_eq!(curved.iter().map(|x| x.0).collect::<BTreeSet<_>>().len(), 1);
    assert_eq!(e.iter().filter(|x| x.0.is_none()).count(), 2);
}

#[test]
fn test_text_and_dimensions_are_skipped_and_counted() {
    let (_s, stats) = load("text_skipped.dxf", 1.0);
    assert_eq!(
        stats
            .skipped
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>(),
        [("TEXT", 2)]
    );
    assert_eq!(stats.edges, 1);
}

#[test]
fn test_a_drawing_with_no_linework_refuses_loudly() {
    let mut s = Scene::new();
    assert!(load_dxf(&mut s, &dxf("text_only.dxf"), None, 1.0, None, None).is_err());
}

#[test]
fn test_inserts_become_component_instances_sharing_one_proto() {
    let (s, stats) = load("inserts.dxf", 1.0);
    let inst: Vec<&Group> = s.doc.groups.iter().filter(|g| g.xform.is_some()).collect();
    assert_eq!(inst.len(), 2);
    assert_eq!(inst[0].mesh, inst[1].mesh);
    assert_eq!(
        stats
            .components
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>(),
        [("ARBOL", 2)]
    );
    let mut xs: Vec<i64> = inst
        .iter()
        .map(|g| g.xform.unwrap().project_point(v3(0., 0., 0.)).x.round() as i64)
        .collect();
    xs.sort();
    assert_eq!(xs, [10, 20]);
    let m = s.doc.mesh(inst[0].mesh);
    let curves: BTreeSet<i64> = m
        .edge_ids()
        .iter()
        .filter_map(|&e| m.edge(e).curve)
        .collect();
    assert_eq!(curves.len(), 1);
}

#[test]
fn test_a_rotated_scaled_insert_lands_where_autocad_put_it() {
    let (s, _) = load("rotated_insert.dxf", 1.0);
    let inst = s.doc.groups.iter().find(|g| g.xform.is_some()).unwrap();
    let tip = inst.xform.unwrap().project_point(v3(1., 0., 0.));
    assert!(
        (tip.x - 10.0).abs() < 1e-6 && (tip.y - 22.0).abs() < 1e-6,
        "{tip:?}"
    );
}

#[test]
fn test_an_annotation_only_block_places_nothing_quietly() {
    let (s, stats) = load("annotation_block.dxf", 1.0);
    assert!(stats.components.is_empty());
    assert!(s.doc.groups.iter().all(|g| g.xform.is_none()));
}

#[test]
fn test_block_children_on_layer_zero_inherit_the_inserts_layer() {
    let (s, _) = load("layer_zero_inherits.dxf", 1.0);
    assert_eq!(s.doc.groups[0].layer.as_deref(), Some("MOBILIARIO"));
}

#[test]
fn test_each_cad_layer_becomes_a_tagged_group() {
    let (s, stats) = load("t.dxf", 1.0);
    assert_eq!(stats.groups, 3);
    let layers: BTreeSet<Option<String>> = s.doc.groups.iter().map(|g| g.layer.clone()).collect();
    assert_eq!(
        layers,
        [None, Some("EJES".into()), Some("MUROS".into())]
            .into_iter()
            .collect()
    );
    assert_eq!(
        s.doc
            .groups
            .iter()
            .find(|g| g.layer.is_none())
            .unwrap()
            .name,
        "t"
    );
    let tags: BTreeSet<&str> = s.doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(tags.contains("MUROS") && tags.contains("EJES"));
    assert!(!tags.contains("0"));
}

#[test]
fn test_unused_cad_layers_leave_no_empty_tags() {
    let (s, _) = load("unused_layer.dxf", 1.0);
    assert!(s.doc.layers.iter().all(|l| l.name != "VACIA"));
}

#[test]
fn test_units_are_read_from_the_header() {
    for (code, metres) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let doc = open_document(&dxf(&format!("u{code}.dxf"))).unwrap();
        assert_eq!(read_unit_scale(&doc), (Some(metres), code));
    }
}

#[test]
fn test_a_unitless_header_says_so_instead_of_guessing() {
    let doc = open_document(&dxf("unitless.dxf")).unwrap();
    assert_eq!(read_unit_scale(&doc), (None, 0));
}

#[test]
fn test_millimetre_drawings_arrive_in_metres() {
    let (s, _) = load("millimetres.dxf", 0.001);
    let m = s.doc.mesh(s.doc.groups[0].mesh);
    let max_x = m
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MIN, f64::max);
    assert_eq!(max_x, 3.5);
}

#[test]
fn test_utm_coordinates_are_recentred_and_reported() {
    let (s, stats) = load("utm.dxf", 1.0);
    let off = stats.offset.unwrap();
    assert!((off.x - 276_000.0).abs() < 1.0 && (off.y - 8_214_000.0).abs() < 1.0);
    let m = s.doc.mesh(s.doc.groups[0].mesh);
    assert!(m
        .verts
        .iter()
        .all(|(_, v)| v.position.x.abs() < RECENTER_BEYOND_M));
}

#[test]
fn test_local_coordinates_are_left_alone() {
    let (_s, stats) = load("local.dxf", 1.0);
    assert!(stats.offset.is_none());
}

#[test]
fn test_the_whole_import_is_one_undoable_step() {
    let path = dxf("undo.dxf");
    let mut s = Scene::new();
    let mut h = History::new();
    let mut cmd = FnCommand::new("SnapshotImport", move |sc: &mut Scene| {
        load_dxf(sc, &path, None, 1.0, None, None)
            .map(|_| ())
            .map_err(|e| CmdError::msg(e.to_string()))
    });
    h.execute(&mut s, &mut cmd);
    assert_eq!(s.doc.groups.len(), 1);
    assert!(s.doc.layers.iter().any(|l| l.name == "MUROS"));
    h.undo(&mut s);
    assert!(s.doc.groups.is_empty());
    assert!(s.doc.layers.iter().all(|l| l.name != "MUROS"));
    h.redo(&mut s);
    assert_eq!(s.doc.groups.len(), 1);
}

#[test]
fn test_3dfaces_become_faces() {
    let (s, stats) = load("3dface.dxf", 1.0);
    assert_eq!(s.doc.mesh(s.doc.groups[0].mesh).faces.len(), 1);
    assert_eq!(stats.faces, 1);
}

#[test]
fn test_solid_corners_unswap_their_z_order() {
    let (s, _) = load("solid.dxf", 1.0);
    let m = s.doc.mesh(s.doc.groups[0].mesh);
    assert!((m.face_area(m.face_ids()[0]) - 4.0).abs() < 1e-6);
}

#[test]
fn test_a_triangulated_plane_merges_back_to_one_face() {
    let (s, _) = load("triangulated_plane.dxf", 1.0);
    let m = s.doc.mesh(s.doc.groups[0].mesh);
    assert_eq!(m.faces.len(), 1);
    assert!((m.face_area(m.face_ids()[0]) - 12.0).abs() < 1e-6);
}

#[test]
fn test_a_lying_millimetre_header_is_vetoed_by_the_drawing() {
    let doc = open_document(&dxf("lying_mm_header.dxf")).unwrap();
    assert_eq!(suggest_unit_scale(&doc), (Some(1.0), 4));
}

#[test]
fn test_an_honest_header_is_believed() {
    let doc = open_document(&dxf("honest_mm_header.dxf")).unwrap();
    assert_eq!(suggest_unit_scale(&doc), (Some(0.001), 4));
}

#[test]
fn test_the_display_name_survives_a_file_with_blocks() {
    let mut s = Scene::new();
    load_dxf(
        &mut s,
        &dxf("display_name.dxf"),
        None,
        1.0,
        None,
        Some("Detalles Plaza Yanque"),
    )
    .unwrap();
    let loose = s
        .doc
        .groups
        .iter()
        .find(|g| g.layer.is_none() && g.xform.is_none())
        .unwrap();
    assert_eq!(loose.name, "Detalles Plaza Yanque");
}

/// The flattening matches ezdxf's (edge / vertex counts ezdxf's
/// flattening produces on the same files).
#[test]
fn test_flattening_matches_the_ezdxf_counts() {
    for (name, want) in [
        ("circle.dxf", vec![(41, 42, 0)]),
        ("bulged_polyline.dxf", vec![(22, 24, 0)]),
        ("inserts.dxf", vec![(33, 33, 0), (33, 33, 0)]),
        ("rotated_insert.dxf", vec![(1, 2, 0)]),
        ("lines_and_polylines.dxf", vec![(4, 4, 0)]),
        ("triangulated_plane.dxf", vec![(4, 4, 1)]),
    ] {
        let (s, _) = load(name, 1.0);
        let got: Vec<(usize, usize, usize)> = s
            .doc
            .groups
            .iter()
            .map(|g| {
                let m = s.doc.mesh(g.mesh);
                (m.edges.len(), m.verts.len(), m.faces.len())
            })
            .collect();
        assert_eq!(got, want, "{name}");
    }
}
