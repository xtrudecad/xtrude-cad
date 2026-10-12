// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! While a push goes into the solid, the frame leaves out what the push
//! replaces — the base face, and the walls it cuts down with their edges —
//! so the preview shows the shape the push will make; the rest of the
//! model stays drawn, and everything is back after a commit or a cancel.

use crate::common;

use common::{add_box, click, draw_rect, frame_triangles, v3, window, Corner};
use glam::DVec3;
use xtrude_app::input::{Modifiers, ScreenPt};
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::Viewport;
use xtrude_app::window::MainWindow;
use xtrude_core::geometry::PolyFace;
use xtrude_core::topology::point_in_face_solid;
use xtrude_core::FaceId;
use xtrude_render::frame::FrameData;

fn hover(vp: &mut Viewport, x: f64, y: f64) {
    vp.input.last_mouse_pos = Some(ScreenPt { x, y });
    vp.process_hover(x, y, Modifiers::NONE);
}

fn click_at(vp: &mut Viewport, x: f64, y: f64) {
    vp.input.last_mouse_pos = Some(ScreenPt { x, y });
    click(vp, x, y, Modifiers::NONE);
}

fn tool(vp: &Viewport) -> &PushPullTool {
    vp.tool::<PushPullTool>().unwrap()
}

/// A 4 × 3 × 2 box seen from the front (−Y), above, with the Push/Pull
/// tool active; `split` draws on its top first.
fn scene(split: impl FnOnce(&mut Viewport)) -> MainWindow {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1200., 800.);
    vp.camera.set_aspect(1200, 800);
    let c = &mut vp.camera;
    c.target = v3(2., 1.5, 1.);
    c.distance = 14.0;
    c.pitch = 35f64.to_radians();
    c.yaw = (-75f64).to_radians();
    add_box(vp, v3(0., 0., 0.), v3(4., 3., 2.));
    split(vp);
    w.activate_tool("pushpull");
    w
}

/// Lock onto the face under `on` and drag 80 px down the screen: into
/// the box.
fn push_in(vp: &mut Viewport, on: DVec3) -> FaceId {
    let ScreenPt { x, y } = vp.world_to_pixel(on).unwrap();
    hover(vp, x, y);
    let (_, face) = tool(vp).hovered_face.expect("a face under the cursor");
    click_at(vp, x, y);
    assert!(tool(vp).dragging());
    hover(vp, x, y + 80.0);
    let e = tool(vp).pp.extrusion;
    assert!(e < -0.05 && e > -1.9, "not a push into the box: {e}");
    face
}

fn frame(vp: &mut Viewport) -> (FrameData, Vec<[Corner; 3]>) {
    let f = vp.frame_data([1200, 800], false);
    let tris = frame_triangles(&f, 0, f.scene_faces.data.len() as u32);
    (f, tris)
}

/// Whether a scene triangle lies in `face` (as it stands now).
fn drawn(tris: &[[Corner; 3]], face: &PolyFace) -> bool {
    let n = face.normal().normalize();
    let o = face.vertices[0];
    tris.iter().any(|t| {
        let c = (t[0].pos + t[1].pos + t[2].pos) / 3.0;
        t.iter().all(|k| (k.pos - o).dot(n).abs() < 1e-4) && point_in_face_solid(face, c)
    })
}

fn every_face_drawn(vp: &mut Viewport) -> bool {
    let (_, tris) = frame(vp);
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .all(|f| drawn(&tris, &m.poly_face(f)))
}

/// Whether a scene line runs from `a` to `b` (either way).
fn line_drawn(f: &FrameData, a: DVec3, b: DVec3) -> bool {
    let at = |v: &xtrude_render::frame::LineVertex| {
        f.origin + DVec3::new(v.pos[0] as f64, v.pos[1] as f64, v.pos[2] as f64)
    };
    f.scene_lines.data.chunks_exact(2).any(|l| {
        let (p, q) = (at(&l[0]), at(&l[1]));
        ((p - a).length() < 1e-4 && (q - b).length() < 1e-4)
            || ((p - b).length() < 1e-4 && (q - a).length() < 1e-4)
    })
}

fn face_where(vp: &Viewport, pred: impl Fn(&DVec3) -> bool) -> FaceId {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .find(|&f| m.face_positions(f).iter().all(&pred))
        .expect("the face")
}

#[test]
fn test_a_recess_hides_its_base_face_and_keeps_the_rest() {
    let mut w = scene(|vp| {
        draw_rect(
            vp,
            &[
                v3(1., 1., 2.),
                v3(3., 1., 2.),
                v3(3., 2., 2.),
                v3(1., 2., 2.),
            ],
        )
    });
    let vp = &mut w.viewport;
    let base = push_in(vp, v3(2., 1.5, 2.));
    let base_poly = vp.scene.mesh().poly_face(base);
    let others: Vec<FaceId> = vp
        .scene
        .mesh()
        .face_ids()
        .into_iter()
        .filter(|&f| f != base)
        .collect();
    let (_, tris) = frame(vp);
    assert!(!drawn(&tris, &base_poly), "the base face covers the recess");
    for f in others {
        let p = vp.scene.mesh().poly_face(f);
        assert!(drawn(&tris, &p), "an untouched face went missing");
    }
    // Commit: the new model, every face drawn.
    let ScreenPt { x, y } = vp.world_to_pixel(v3(2., 1.5, 2.)).unwrap();
    click_at(vp, x, y + 80.0);
    assert!(!tool(vp).dragging());
    assert!(vp.suppressed_faces().is_empty() && vp.suppressed_edges().is_empty());
    assert!(every_face_drawn(vp));
}

#[test]
fn test_a_push_down_the_walls_hides_the_strips_it_cuts_off() {
    let mut w = scene(|vp| {
        let segs = [(v3(0., 1.5, 2.), v3(4., 1.5, 2.))];
        let cmd = xtrude_model::edits::build_add_edges(&vp.scene, &segs, true, vec![], None);
        vp.execute_boxed(cmd);
    });
    let vp = &mut w.viewport;
    let front_wall = face_where(vp, |v| v.y == 0.0);
    let back_wall = face_where(vp, |v| v.y == 3.0);
    let bottom = face_where(vp, |v| v.z == 0.0);
    let base = push_in(vp, v3(2., 0.75, 2.));
    let e = tool(vp).pp.extrusion;
    let m = vp.scene.mesh();
    let (front_poly, back_poly, bottom_poly, base_poly) = (
        m.poly_face(front_wall),
        m.poly_face(back_wall),
        m.poly_face(bottom),
        m.poly_face(base),
    );
    let (f, tris) = frame(vp);
    assert!(!drawn(&tris, &base_poly), "the base face is still drawn");
    assert!(!drawn(&tris, &front_poly), "the cut wall is still drawn");
    assert!(drawn(&tris, &back_poly) && drawn(&tris, &bottom_poly));
    // The front wall's top edge goes; its bottom edge stays (the bottom
    // face still has it).
    assert!(!line_drawn(&f, v3(0., 0., 2.), v3(4., 0., 2.)));
    assert!(line_drawn(&f, v3(0., 3., 2.), v3(4., 3., 2.)));
    // The preview stands in for what stays of the front wall: up to the
    // moved cap, no higher.
    let stand_in = tool(vp)
        .preview_faces()
        .into_iter()
        .find(|p| p.vertices.iter().all(|v| v.y.abs() < 1e-9))
        .expect("what stays of the front wall");
    let top = stand_in
        .vertices
        .iter()
        .map(|v| v.z)
        .fold(f64::MIN, f64::max);
    assert!((top - (2.0 + e)).abs() < 1e-6);
    // Cancel: everything is back.
    vp.escape();
    assert!(!tool(vp).dragging());
    assert!(vp.suppressed_faces().is_empty() && vp.suppressed_edges().is_empty());
    assert!(every_face_drawn(vp));
    let (f, _) = frame(vp);
    assert!(line_drawn(&f, v3(0., 0., 2.), v3(4., 0., 2.)));
}
