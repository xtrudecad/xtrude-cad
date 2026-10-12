// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Stamp copied geometry at the cursor, as one undoable step; Copy/Cut
//! snapshots; Paste in Place; the loose preview drawn; the main window's Paste
//! and Paste in Place.

use crate::common;

use common::{stub, v3, window};
use glam::{DMat4, DVec3};
use xtrude_app::tools::paste::PasteTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::math::MatExt;
use xtrude_core::Mesh;
use xtrude_io::clip::ClipData;
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, placement_points, Group};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_render::frame::{Source, Stipple};

fn v(x: f64, y: f64) -> DVec3 {
    v3(x, y, 0.)
}

fn square_clip(x0: f64) -> ClipData {
    ClipData {
        faces: vec![(
            vec![v(x0, 0.), v(x0 + 2., 0.), v(x0 + 2., 2.), v(x0, 2.)],
            vec![],
            FaceAttrs::default(),
        )],
        reference: DVec3::ZERO,
        ..ClipData::default()
    }
}

fn paste_at(vp: &mut Viewport, x: f64, y: f64) -> PasteTool {
    let mut tool = PasteTool::new();
    tool.on_activate(vp);
    tool.on_click(vp, &ToolContext::at(v(x, y)));
    tool
}

fn rounded_xy(vp: &Viewport, f: xtrude_core::FaceId) -> std::collections::BTreeSet<(i64, i64)> {
    vp.scene
        .mesh()
        .face_positions(f)
        .iter()
        .map(|p| (p.x.round() as i64, p.y.round() as i64))
        .collect()
}

#[test]
fn test_paste_stamps_clipboard_at_cursor() {
    let mut vp = stub();
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    vp.clipboard = Some(square_clip(0.));
    paste_at(&mut vp, 5., 5.);
    let ids = vp.scene.mesh().face_ids();
    assert_eq!(ids.len(), 2);
    let want: std::collections::BTreeSet<(i64, i64)> =
        [(5, 5), (7, 5), (7, 7), (5, 7)].into_iter().collect();
    assert_eq!(
        ids.iter().filter(|&&f| rounded_xy(&vp, f) == want).count(),
        1
    );
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().faces.len(), 1);
}

#[test]
fn test_paste_preview_follows_cursor() {
    let mut vp = stub();
    vp.clipboard = Some(square_clip(0.));
    let mut tool = PasteTool::new();
    tool.on_activate(&mut vp);
    tool.on_hover(&mut vp, &ToolContext::at(v(3., 0.)));
    let segs = tool.rubber_band_lines(&vp);
    assert_eq!(segs.len(), 4); // the square's 4 edges, offset by +3 x
    let xs: std::collections::BTreeSet<i64> = segs
        .iter()
        .flat_map(|(a, b)| [a.x.round() as i64, b.x.round() as i64])
        .collect();
    assert_eq!(xs, [3, 5].into_iter().collect());
}

#[test]
fn test_paste_keeps_face_attrs() {
    let mut vp = stub();
    let f = vp
        .scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([1.0, 0.0, 0.0]);
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    assert!(vp.copy_selection());
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.0, 1.0, 0.0]); // snapshot
    paste_at(&mut vp, 5., 0.);
    let m = vp.scene.mesh();
    let pasted = m.face_ids().into_iter().find(|&k| k != f).unwrap();
    assert_eq!(m.face(pasted).attrs.paint.color, Some([1.0, 0.0, 0.0]));
}

// ---- Groups in the clipboard -----------------------------------------------

fn make_group(vp: &mut Viewport, x0: f64) -> GroupId {
    let mut m = Mesh::new();
    let f = m.add_face(&[v(x0, 0.), v(x0 + 2., 0.), v(x0 + 2., 2.), v(x0, 2.)], &[]);
    m.face_mut(f).attrs.paint.color = Some([0.5, 0.5, 0.5]);
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let mut g = Group::new(mid, Some("Box"));
    g.layer = Some("Shells".into());
    let id = g.id;
    vp.scene.doc.groups.push(g);
    id
}

fn group(vp: &Viewport, id: GroupId) -> &Group {
    vp.scene.doc.find_group(id).unwrap()
}

fn group_ids(vp: &Viewport) -> Vec<GroupId> {
    vp.scene.doc.groups.iter().map(|g| g.id).collect()
}

#[test]
fn test_copy_paste_group() {
    let mut vp = stub();
    let g = make_group(&mut vp, 0.);
    vp.scene.selection.insert(EntityRef::Group(g));
    assert!(vp.copy_selection()); // a lone group IS copyable
    paste_at(&mut vp, 5., 0.);
    assert_eq!(vp.scene.doc.groups.len(), 2);
    let pasted = group_ids(&vp).into_iter().find(|&k| k != g).unwrap();
    let pg = group(&vp, pasted).clone();
    assert!(pg.name == "Box" && pg.layer.as_deref() == Some("Shells"));
    assert_ne!(pg.mesh, group(&vp, g).mesh); // snapshot: no aliasing the source
                                             // A pasted classic group is an INSTANCE of the clipboard snapshot.
    let xf = pg.xform.expect("an instance");
    let m = vp.scene.doc.mesh(pg.mesh);
    let mut xs: Vec<i64> = m
        .face_ids()
        .iter()
        .flat_map(|&f| m.face_positions(f))
        .map(|p| xf.project_point(p).x.round() as i64)
        .collect();
    xs.sort();
    assert_eq!(xs, vec![5, 5, 7, 7]); // the ref corner landed at the cursor
    assert!(m.face(m.face_ids()[0]).attrs.paint.color.is_some());
    assert!(vp.scene.selection.contains(&EntityRef::Group(pasted)));

    paste_at(&mut vp, 9., 0.); // stamp again: sibling instance
    let third = group_ids(&vp)
        .into_iter()
        .find(|&k| k != g && k != pasted)
        .unwrap();
    assert_eq!(group(&vp, third).mesh, pg.mesh); // shared prototype
    assert_eq!(
        group(&vp, third)
            .xform
            .unwrap()
            .project_point(DVec3::ZERO)
            .x
            .round(),
        9.0
    );

    assert!(vp.undo()); // undo the second paste
    assert!(vp.undo());
    assert_eq!(group_ids(&vp), vec![g]); // one undoable step per paste
}

#[test]
fn test_copy_paste_instance_shares_prototype() {
    let mut vp = stub();
    let mut proto = Mesh::new();
    proto.add_face(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)], &[]);
    let pid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let mut inst = Group::new(pid, Some("Peg"));
    inst.xform = Some(DMat4::IDENTITY);
    let iid = inst.id;
    vp.scene.doc.groups.push(inst);
    vp.scene.selection.insert(EntityRef::Group(iid));
    assert!(vp.copy_selection());
    paste_at(&mut vp, 10., 0.);
    let pasted = group_ids(&vp).into_iter().find(|&k| k != iid).unwrap();
    let pg = group(&vp, pasted);
    assert_eq!(pg.mesh, pid); // sibling instance, shared prototype
    assert_eq!(pg.xform.unwrap().project_point(DVec3::ZERO).x.round(), 10.0);
}

#[test]
fn test_group_paste_previews_via_frozen_scratch() {
    let mut vp = stub();
    let g = make_group(&mut vp, 0.); // a 2×2 square at the origin
    vp.scene.selection.insert(EntityRef::Group(g));
    assert!(vp.copy_selection());
    let mut tool = PasteTool::new();
    tool.on_activate(&mut vp);
    let snapshots: Vec<GroupId> = vp
        .clipboard
        .as_ref()
        .unwrap()
        .groups
        .iter()
        .map(|t| t.id)
        .collect();
    assert!(vp.preview.external);
    assert_eq!(vp.preview.groups, snapshots); // the snapshots, not g
    tool.on_hover(&mut vp, &ToolContext::at(v(10., 0.)));
    let off = vp.preview.matrix.unwrap().project_point(DVec3::ZERO);
    assert_eq!(off.x.round(), 10.0); // offset = cursor − ref corner
                                     // Groups no longer flow through the per-frame preview paths.
    assert!(tool.preview_faces().is_empty());
    assert!(tool.rubber_band_lines(&vp).is_empty());
    tool.on_deactivate(&mut vp);
    assert!(vp.preview.groups.is_empty()); // scratch preview torn down
}

#[test]
fn test_group_paste_stamp_ends_the_scratch_preview() {
    let mut vp = stub();
    let g = make_group(&mut vp, 0.);
    vp.scene.selection.insert(EntityRef::Group(g));
    assert!(vp.copy_selection());
    let mut tool = paste_at(&mut vp, 5., 0.);
    assert_eq!(vp.scene.doc.groups.len(), 2); // stamped
    assert!(vp.preview.groups.is_empty()); // ended at the stamp…
    tool.on_deactivate(&mut vp); // …and tearing down again is harmless
    assert!(vp.preview.groups.is_empty());
}

#[test]
fn test_clipboard_survives_deleting_the_original() {
    let mut vp = stub();
    let g = make_group(&mut vp, 0.);
    vp.scene.selection.insert(EntityRef::Group(g));
    assert!(vp.copy_selection());
    vp.scene.doc.groups.retain(|k| k.id != g); // the user deletes the original
    vp.scene.selection.clear();
    paste_at(&mut vp, 3., 3.);
    assert_eq!(vp.scene.doc.groups.len(), 1);
    assert_eq!(vp.scene.doc.groups[0].name, "Box");
}

#[test]
fn test_cut_group_removes_it_and_paste_restores() {
    let mut vp = stub();
    let g = make_group(&mut vp, 0.);
    vp.scene.selection.insert(EntityRef::Group(g));
    assert!(vp.cut_selection());
    assert!(vp.scene.doc.groups.is_empty()); // gone, and on the clipboard
    paste_at(&mut vp, 0., 0.);
    assert_eq!(vp.scene.doc.groups.len(), 1);
    assert!(vp.undo()); // undo the paste
    assert!(vp.scene.doc.groups.is_empty());
    assert!(vp.undo()); // undo the cut
    assert_eq!(group_ids(&vp), vec![g]);
}

#[test]
fn test_copy_paste_container_whose_own_mesh_is_empty() {
    let mut vp = stub();
    vp.clipboard = Some(ClipData {
        reference: v(99., 99.),
        ..ClipData::default()
    });
    let mut m = Mesh::new();
    m.add_face(&[v(3., 3.), v(4., 3.), v(4., 4.), v(3., 4.)], &[]);
    let pm = add_mesh(&mut vp.scene.doc.meshes, m);
    let peg = Group::new(pm, Some("Peg"));
    let lm = add_mesh(&mut vp.scene.doc.meshes, Mesh::new());
    let mut lamp = Group::new(lm, Some("Lamp"));
    lamp.adopt(vec![peg]);
    let lid = lamp.id;
    vp.scene.doc.groups.push(lamp);
    vp.scene.selection.insert(EntityRef::Group(lid));
    assert!(vp.copy_selection(), "a container IS copyable");
    let clip = vp.clipboard.as_ref().unwrap();
    assert_eq!(clip.reference, v(3., 3.), "held by the corner of its parts");
    assert_eq!(
        clip.groups
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Lamp"]
    );
    paste_at(&mut vp, 10., 10.);
    let pasted = group_ids(&vp).into_iter().find(|&k| k != lid).unwrap();
    let pg = group(&vp, pasted).clone();
    assert!(pg.name == "Lamp" && pg.children.len() == 1);
    let pts = placement_points(&vp.scene.doc.meshes, &pg);
    let lo = pts
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    assert!((lo.x - 10.0).abs() < 1e-6 && (lo.y - 10.0).abs() < 1e-6);
}

#[test]
fn test_a_face_pasted_over_another_merges_with_it() {
    let mut vp = stub();
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    vp.scene.version += 1;
    let mut tool = PasteTool::with_clip(square_clip(0.));
    tool.on_click(&mut vp, &ToolContext::at(v(1., 1.)));
    // Overlapping 2×2 squares offset by (1,1): three regions, no stacking.
    let m = vp.scene.mesh();
    let mut areas: Vec<i64> = m
        .face_ids()
        .into_iter()
        .map(|f| (m.face_area(f) * 1e6).round() as i64)
        .collect();
    areas.sort();
    assert_eq!(areas, vec![1_000_000, 3_000_000, 3_000_000]);
}

#[test]
fn test_paste_in_place_lands_on_the_original_and_selects_it() {
    let mut vp = stub();
    let mut clip = square_clip(0.);
    clip.edges = vec![(v(5., 5.), v(6., 5.), false, None)];
    vp.clipboard = Some(clip);
    assert!(PasteTool::in_place(&mut vp));
    let m = vp.scene.mesh();
    let coords: std::collections::BTreeSet<(i64, i64)> = m
        .face_ids()
        .iter()
        .flat_map(|&f| m.face_positions(f))
        .map(|p| (p.x.round() as i64, p.y.round() as i64))
        .collect();
    assert_eq!(
        coords,
        [(0, 0), (2, 0), (2, 2), (0, 2)].into_iter().collect()
    ); // no offset
    assert_eq!(vp.scene.selection.len(), 2); // the face and the edge
    assert!(vp.undo()); // one step
    assert!(vp.scene.mesh().faces.is_empty());
    vp.clipboard = None;
    assert!(!PasteTool::in_place(&mut vp));
}

fn group_in_edit(vp: &mut Viewport, m: Mesh) -> GroupId {
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    let g = Group::new(mid, Some("B"));
    let id = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.begin_group_edit(id);
    id
}

fn counts(vp: &Viewport) -> (usize, usize) {
    let m = vp.scene.mesh();
    (m.faces.len(), m.edges.len())
}

#[test]
fn test_undoing_a_paste_leaves_no_loose_edges_behind() {
    let mut vp = stub();
    group_in_edit(&mut vp, Mesh::new());
    let mut clip = square_clip(0.);
    clip.edges = vec![(v(0., 0.), v(2., 0.), false, None)];
    vp.clipboard = Some(clip);
    assert!(PasteTool::in_place(&mut vp));
    assert_eq!(counts(&vp), (1, 4));
    assert!(vp.undo());
    assert_eq!(counts(&vp), (0, 0));
    assert!(vp.redo());
    assert_eq!(counts(&vp), (1, 4));
}

#[test]
fn test_undoing_a_paste_over_the_original_keeps_the_original_edges() {
    let mut vp = stub();
    vp.scene
        .mesh_mut()
        .add_face(&[v(0., 0.), v(2., 0.), v(2., 2.), v(0., 2.)], &[]);
    vp.clipboard = Some(square_clip(0.));
    let before = vp.scene.mesh().edges.len();
    assert!(PasteTool::in_place(&mut vp));
    assert!(vp.undo());
    assert_eq!(vp.scene.mesh().edges.len(), before);
    assert_eq!(before, 4);
}

#[test]
fn test_undoing_a_paste_shrinks_the_group_back_and_redo_restores_it() {
    let mut vp = stub();
    let mut m = Mesh::new();
    m.add_face(&[v(10., 0.), v(12., 0.), v(12., 2.), v(10., 2.)], &[]);
    group_in_edit(&mut vp, m);
    vp.clipboard = Some(square_clip(0.));
    let xs = |vp: &Viewport| {
        let mut x: Vec<i64> = vp
            .scene
            .mesh()
            .verts
            .iter()
            .map(|(_, vx)| vx.position.x.round() as i64)
            .collect();
        x.sort();
        x.dedup();
        x
    };
    assert!(PasteTool::in_place(&mut vp));
    assert_eq!(xs(&vp), vec![0, 2, 10, 12]);
    assert!(vp.undo());
    assert_eq!(xs(&vp), vec![10, 12]); // box back to the original size
    assert!(vp.redo());
    assert_eq!(xs(&vp), vec![0, 2, 10, 12]);
    assert_eq!(counts(&vp), (2, 8));
    // a stamp can still weld onto the restored vertices
    assert!(PasteTool::in_place(&mut vp));
}

#[test]
fn test_preview_faces_survive_viewport_memo_helpers() {
    // A PREVIEW face must not break the paint of a loose paste preview —
    // nothing followed the cursor (user report). Xtrude CAD draws
    // preview faces from their points: the frame of a loose paste holds
    // the square, triangulated, facing +Z, of area 4, at the cursor.
    let mut vp = stub();
    vp.clipboard = Some(square_clip(0.));
    vp.set_active_tool(Some(Box::new(PasteTool::new())));
    vp.with_tool(|t, vp| t.on_hover(vp, &ToolContext::at(v(3., 0.))));
    let f = vp.frame_data([200, 200], false);
    let d = f
        .draws
        .iter()
        // not the sky, not the selection dots
        .find(|d| d.source == Source::Faces && !d.ndc && d.stipple == Stipple::None)
        .expect("the preview's faces are drawn");
    let tris: Vec<[DVec3; 3]> = f.faces[d.first as usize..(d.first + d.count) as usize]
        .chunks_exact(3)
        .map(|t| {
            [0, 1, 2].map(|i| {
                let q = t[i].pos;
                f.origin + DVec3::new(q[0] as f64, q[1] as f64, q[2] as f64)
            })
        })
        .collect();
    assert_eq!(tris.len(), 2);
    let mut area = 0.0;
    for [a, b, c] in &tris {
        let n = (*b - *a).cross(*c - *a);
        assert!((n.normalize().z.abs() - 1.0).abs() < 1e-3);
        area += n.length() / 2.0;
    }
    assert!((area - 4.0).abs() < 1e-6);
    let xs = tris.iter().flatten().map(|p| p.x);
    assert_eq!(xs.clone().fold(f64::MAX, f64::min).round(), 3.0); // at the cursor
    assert_eq!(xs.fold(f64::MIN, f64::max).round(), 5.0);
}

fn unit_square_clip() -> ClipData {
    ClipData {
        faces: vec![(
            vec![v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)],
            vec![],
            FaceAttrs::default(),
        )],
        reference: DVec3::ZERO,
        ..ClipData::default()
    }
}

fn active_key(w: &xtrude_app::window::MainWindow) -> Option<String> {
    w.viewport.active_tool().map(|t| t.info().key.to_string())
}

#[test]
fn test_paste_stamps_once_and_returns_to_select() {
    // One stamp per paste: after the click the Select tool is back,
    // nothing keeps following the cursor. The clipboard survives, so
    // pasting again stamps another copy.
    let mut win = window();
    win.viewport.clipboard = Some(unit_square_clip());
    win.trigger("paste");
    assert_eq!(active_key(&win).as_deref(), Some("paste"));
    let ctx = ToolContext::at(v(5., 5.));
    win.viewport.with_tool(|t, vp| t.on_click(vp, &ctx));
    win.pump_events();
    assert_eq!(active_key(&win).as_deref(), Some("select"));
    assert_eq!(win.viewport.scene.mesh().faces.len(), 1);
    win.trigger("paste"); // clipboard kept: paste again works
    assert_eq!(active_key(&win).as_deref(), Some("paste"));
}

#[test]
fn test_paste_in_place_menu_action_keeps_the_active_tool() {
    let mut win = window();
    win.viewport.clipboard = Some(unit_square_clip());
    let before = active_key(&win);
    win.trigger("paste_in_place");
    assert_eq!(active_key(&win), before);
    assert_eq!(win.viewport.scene.mesh().faces.len(), 1);
}
