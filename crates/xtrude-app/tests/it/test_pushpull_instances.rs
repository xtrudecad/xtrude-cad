// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! From outside, a closed group's face is not pushed (nor shaded, nor
//! double-clicked) and the refusal says why; inside a component the push
//! reaches every copy, as one undo step. The real pick finds the copy's top
//! face under the cursor.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{v3, window};
use glam::DMat4;
use xtrude_app::tools::pushpull::{set_last_distance, PushPullTool};
use xtrude_app::tools::ToolContext;
use xtrude_app::viewport::ViewportEvent;
use xtrude_app::window::MainWindow;
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, world_mesh, Group};
use xtrude_model::ids::{GroupId, MeshId};

fn unit_box() -> Mesh {
    let mut m = Mesh::new();
    let p = [
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        v3(1., 1., 0.),
        v3(0., 1., 0.),
        v3(0., 0., 1.),
        v3(1., 0., 1.),
        v3(1., 1., 1.),
        v3(0., 1., 1.),
    ];
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        m.add_face(&q.map(|i| p[i]), &[]);
    }
    m
}

/// Two placements of one unit box ("Post"), the second 3 m along X; the
/// camera looks down at them.
fn instances() -> (MainWindow, MeshId, GroupId, GroupId) {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1000., 700.);
    vp.camera.set_aspect(1000, 700);
    vp.camera.set_view("top");
    vp.camera.target = v3(2., 0.5, 0.);
    vp.camera.distance = 10.0;
    let proto = add_mesh(&mut vp.scene.doc.meshes, unit_box());
    let mut a = Group::new(proto, Some("Post"));
    let mut b = Group::new(proto, Some("Post"));
    a.xform = Some(DMat4::IDENTITY);
    b.xform = Some(DMat4::from_translation(v3(3., 0., 0.)));
    let (ia, ib) = (a.id, b.id);
    vp.scene.doc.groups.extend([a, b]);
    vp.scene.version += 1;
    (w, proto, ia, ib)
}

fn proto_volume(w: &MainWindow, proto: MeshId) -> f64 {
    signed_volume(w.viewport.scene.doc.mesh(proto))
}

/// A context over b's top face.
fn over_b(w: &MainWindow) -> ToolContext {
    let ScreenPt { x, y } = w.viewport.world_to_pixel(v3(3.5, 0.5, 1.)).unwrap();
    let mut c = ToolContext::at(v3(3.5, 0.5, 1.));
    c.screen = (x, y);
    c
}

#[test]
fn test_a_closed_instance_is_not_pushed() {
    let (mut w, proto, _a, b) = instances();
    let ctx = over_b(&w);
    w.viewport
        .set_active_tool(Some(Box::new(PushPullTool::new())));
    let vp = &mut w.viewport;
    vp.with_tool(|t, vp| {
        t.on_hover(vp, &ctx);
        t.on_click(vp, &ctx);
    });
    assert!(
        !vp.tool::<PushPullTool>().unwrap().dragging(),
        "a push started in a closed group"
    );
    assert!(
        vp.scene.edit_group().is_none(),
        "opened a session behind the user's back"
    );
    let g = vp.scene.doc.find_group(b).unwrap();
    assert!(g.mesh == proto && g.xform.is_some()); // still shared
    assert!((proto_volume(&w, proto) - 1.0).abs() < 1e-6); // untouched
    assert_eq!(w.viewport.history.undo_len(), 0);
    let said: Vec<String> = w
        .viewport
        .outbox
        .events
        .iter()
        .filter_map(|e| match e {
            ViewportEvent::Flash(t, _) => Some(t.to_lowercase()),
            _ => None,
        })
        .collect();
    assert!(
        said.iter()
            .any(|s| s.contains("group") && s.contains("double-click")),
        "refused without saying why: {said:?}"
    );
}

#[test]
fn test_nor_is_it_by_double_click() {
    let (mut w, proto, _a, _b) = instances();
    let ctx = over_b(&w);
    w.viewport
        .set_active_tool(Some(Box::new(PushPullTool::new())));
    set_last_distance(Some(1.5));
    w.viewport.with_tool(|t, vp| t.on_double_click(vp, &ctx));
    set_last_distance(None);
    let vp = &w.viewport;
    assert!(!vp.tool::<PushPullTool>().unwrap().dragging() && vp.scene.edit_group().is_none());
    assert!((proto_volume(&w, proto) - 1.0).abs() < 1e-6);
}

#[test]
fn test_a_closed_groups_face_is_not_even_highlighted() {
    let (mut w, _proto, _a, _b) = instances();
    let ctx = over_b(&w);
    w.viewport
        .set_active_tool(Some(Box::new(PushPullTool::new())));
    w.viewport.with_tool(|t, vp| t.on_hover(vp, &ctx));
    assert_eq!(
        w.viewport.hover_entity(),
        None,
        "shaded a face it cannot push"
    );
}

#[test]
fn test_inside_the_component_the_push_reaches_every_copy() {
    let (mut w, proto, a, b) = instances();
    let vp = &mut w.viewport;
    vp.begin_group_edit(b); // the user's double-click
    assert_eq!(vp.scene.edit_group(), Some(b));
    let m = vp.scene.mesh();
    let top_face = m
        .face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(|p| (p.z - 1.0).abs() < 1e-9))
        .unwrap();
    assert!(
        (m.face_centroid(top_face).x - 3.5).abs() < 1e-6,
        "the copy in the world"
    );
    let mid = vp.scene.active_mesh_id();
    let mut pp = PushPullTool::new();
    pp.begin(vp, mid, top_face);
    assert!(pp.dragging());
    pp.pp.extrusion = 2.0;
    pp.commit(vp);
    vp.end_group_edit(); // Esc

    let doc = &vp.scene.doc;
    let gb = doc.find_group(b).unwrap();
    assert!(gb.mesh == proto && gb.xform.is_some()); // shared again
    assert_eq!(doc.find_group(a).unwrap().mesh, proto);
    assert!(is_closed(doc.mesh(proto)) && (signed_volume(doc.mesh(proto)) - 3.0).abs() < 1e-6);
    let wa = world_mesh(&doc.meshes, doc.find_group(a).unwrap());
    let wb = world_mesh(&doc.meshes, gb);
    assert!((signed_volume(&wa) - 3.0).abs() < 1e-6);
    assert!((signed_volume(&wb) - 3.0).abs() < 1e-6);
    let max_x = wb
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MIN, f64::max);
    assert!((max_x - 4.0).abs() < 1e-6);
    assert!(vp.undo());
    assert!((proto_volume(&w, proto) - 1.0).abs() < 1e-6);
    assert!(w.viewport.redo());
    assert!((proto_volume(&w, proto) - 3.0).abs() < 1e-6);
}
