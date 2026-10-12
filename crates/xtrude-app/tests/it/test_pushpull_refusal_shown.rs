// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A push the commit would refuse is refused while dragging: no preview,
//! the "not allowed" pointer and a note beside it, and the click does
//! nothing; a distance the commit takes previews and commits as before.
//! Refusals after a commit show beside the pointer too, for a while.

use crate::common;
use xtrude_app::input::ScreenPt;

use common::{click, v3, window};
use std::time::{Duration, Instant};
use xtrude_app::input::Modifiers;
use xtrude_app::tools::pushpull::PushPullTool;
use xtrude_app::tools::Tool;
use xtrude_app::viewport::overlay::Paint;
use xtrude_app::viewport::{Cursor, Viewport};
use xtrude_app::window::MainWindow;
use xtrude_core::Mesh;
use xtrude_model::ids::MeshId;
use xtrude_model::pushpull::push_face;

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

/// Every push breaks the solid (the test hook replacing the push).
fn break_it(m: &mut Mesh) {
    let f = m.face_ids()[0];
    m.remove_face(f);
}

fn note_painted(vp: &mut Viewport) -> bool {
    let note = vp.note_text().map(str::to_string);
    vp.overlay()
        .iter()
        .any(|p| matches!(p, Paint::Text { text, .. } if Some(text) == note.as_ref()))
}

/// A 4 m cube seen from above, the Push/Pull tool locked on its top, and
/// the top's centre on screen.
fn dragging_the_top() -> (MainWindow, ScreenPt) {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(1200., 800.);
    vp.camera.set_aspect(1200, 800);
    vp.camera.target = v3(2., 2., 2.);
    vp.camera.distance = 20.0;
    let base = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
        &[],
    );
    vp.scene.version += 1;
    let (scene, history) = (&mut vp.scene, &mut vp.history);
    assert!(push_face(scene, history, MeshId::LOOSE, base, 4.0, false));
    w.activate_tool("pushpull");
    let vp = &mut w.viewport;
    let at = vp.world_to_pixel(v3(2., 2., 4.)).unwrap();
    hover(vp, at.x, at.y);
    click_at(vp, at.x, at.y);
    assert!(tool(vp).dragging());
    (w, at)
}

#[test]
fn test_a_refused_distance_shows_no_preview_and_the_not_allowed_pointer() {
    let (mut w, at) = dragging_the_top();
    let vp = &mut w.viewport;
    vp.tool_mut::<PushPullTool>().unwrap().pp.inner_override = Some(break_it);
    let before = vp.scene.mesh().clone();
    hover(vp, at.x, at.y - 60.0);
    assert!(tool(vp).pp.extrusion > 0.1);
    assert!(tool(vp).denied);
    assert!(tool(vp).preview_faces().is_empty());
    assert_eq!(vp.scene.mesh().faces.len(), before.faces.len());
    assert_eq!(vp.outbox.cursor, Some(Cursor::NotAllowed));
    assert!(vp
        .note_text()
        .is_some_and(|t| t.contains("break the solid")));
    assert!(note_painted(vp));
    // The click does nothing: the drag goes on, the solid is unchanged.
    let steps = vp.history.undo_len();
    click_at(vp, at.x, at.y - 60.0);
    assert!(tool(vp).dragging());
    assert_eq!(vp.history.undo_len(), steps);
    assert_eq!(vp.scene.mesh().faces.len(), before.faces.len());
    assert!(vp
        .outbox
        .status_flash
        .as_ref()
        .is_some_and(|(t, _)| t.contains("break the solid")));
}

#[test]
fn test_moving_to_a_distance_the_commit_takes_previews_and_commits_again() {
    let (mut w, at) = dragging_the_top();
    let vp = &mut w.viewport;
    vp.tool_mut::<PushPullTool>().unwrap().pp.inner_override = Some(break_it);
    hover(vp, at.x, at.y - 60.0);
    assert!(tool(vp).denied);
    // The breaking hook gone, a new distance is tried and taken.
    vp.tool_mut::<PushPullTool>().unwrap().pp.inner_override = None;
    hover(vp, at.x, at.y - 80.0);
    assert!(!tool(vp).denied);
    assert_eq!(
        vp.outbox.cursor,
        Some(Cursor::Tool {
            key: "pushpull",
            plus: false
        })
    );
    assert_eq!(vp.note_text(), None);
    let d = tool(vp).pp.extrusion;
    click_at(vp, at.x, at.y - 80.0);
    assert!(!tool(vp).dragging());
    let top = vp
        .scene
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| v.position.z)
        .fold(f64::MIN, f64::max);
    assert!((top - (4.0 + d)).abs() < 1e-6, "{top} vs {d}");
}

#[test]
fn test_each_distance_is_tried_once_per_drag() {
    let (mut w, at) = dragging_the_top();
    let vp = &mut w.viewport;
    hover(vp, at.x, at.y - 60.0);
    assert!(!tool(vp).denied);
    // The verdict is kept: breaking the push now changes nothing for the
    // distance already tried...
    vp.tool_mut::<PushPullTool>().unwrap().pp.inner_override = Some(break_it);
    hover(vp, at.x, at.y - 60.0);
    assert!(!tool(vp).denied);
    // ...and a new distance is tried.
    hover(vp, at.x, at.y - 70.0);
    assert!(tool(vp).denied);
}

#[test]
fn test_escape_takes_the_refusal_away() {
    let (mut w, at) = dragging_the_top();
    let vp = &mut w.viewport;
    vp.tool_mut::<PushPullTool>().unwrap().pp.inner_override = Some(break_it);
    hover(vp, at.x, at.y - 60.0);
    assert!(tool(vp).denied);
    vp.with_tool(|t, vp| t.on_cancel(vp));
    assert!(!tool(vp).dragging() && !tool(vp).denied);
    assert_eq!(vp.note_text(), None);
    assert_ne!(vp.outbox.cursor, Some(Cursor::NotAllowed));
}

#[test]
fn test_a_flashed_note_shows_beside_the_pointer_until_its_time() {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(800., 600.);
    vp.input.last_mouse_pos = Some(ScreenPt { x: 300., y: 200. });
    assert_eq!(w.next_tick(Instant::now()), None);
    let vp = &mut w.viewport;
    vp.flash_note("Push stopped at 2 m", 1000);
    assert_eq!(vp.note_text(), Some("Push stopped at 2 m"));
    assert!(vp
        .outbox
        .status_flash
        .as_ref()
        .is_some_and(|(t, ms)| t == "Push stopped at 2 m" && *ms == 1000));
    let painted = vp.overlay();
    let at = painted.iter().find_map(|p| match p {
        Paint::Text { text, at, .. } if text == "Push stopped at 2 m" => Some(*at),
        _ => None,
    });
    let (x, y) = at.expect("the note is painted");
    assert!(
        x > 300. && y > 200. && x < 400. && y < 260.,
        "beside the pointer: {x}, {y}"
    );
    let now = Instant::now();
    let due = w.next_tick(now).expect("the note's timer");
    assert!(due <= Duration::from_millis(1000));
    w.tick(now + Duration::from_millis(1001));
    assert_eq!(w.viewport.note_text(), None);
    assert_eq!(w.next_tick(Instant::now()), None);
}

#[test]
fn test_a_note_near_the_corner_stays_inside_the_viewport() {
    let mut w = window();
    let vp = &mut w.viewport;
    vp.resize(800., 600.);
    vp.input.last_mouse_pos = Some(ScreenPt { x: 790., y: 590. });
    vp.flash_note(
        "Push refused: would break the solid (left it unchanged)",
        1000,
    );
    for p in vp.overlay() {
        if let Paint::Rect { min, max, .. } = p {
            assert!(min.0 >= 0.0 && min.1 >= 0.0 && max.0 <= 800.0 && max.1 <= 600.0);
        }
    }
}
