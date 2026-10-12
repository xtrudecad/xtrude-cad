// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! System-clipboard tests: Copy/Paste between Xtrude CAD windows.
//! The wire-format tests are in xtrude-io.

use crate::common;

use common::{v3, window};
use std::cell::RefCell;
use std::rc::Rc;

use xtrude_app::window::clipboard::{
    available, bytes_to_text, flush, foreign, is_clip_text, publish, text_to_bytes, ClipOffer,
    MemoryClipboard, SystemClipboard, TextClipboard, TextStore, MIME, TEXT_HEADER,
};
use xtrude_app::window::MainWindow;
use xtrude_core::attrs::{FaceAttrs, Paint};
use xtrude_core::math::DMat4;
use xtrude_core::Mesh;
use xtrude_io::clip::{decode, encode, ClipData};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, Group, MeshTable};

fn box_group(t: &mut MeshTable, x: f64) -> Group {
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(x, 0., 0.),
            v3(x + 1., 0., 0.),
            v3(x + 1., 1., 0.),
            v3(x, 1., 0.),
        ],
        &[],
    );
    let mut g = Group::new(add_mesh(t, m), None);
    g.name = format!("box {x}");
    g
}

fn clip_data() -> ClipData {
    let mut t = MeshTable::new();
    let square = vec![
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(2., 2., 0.),
        v3(0., 2., 0.),
    ];
    let hole = vec![
        v3(0.5, 0.5, 0.),
        v3(1., 0.5, 0.),
        v3(1., 1., 0.),
        v3(0.5, 1., 0.),
    ];
    let classic = box_group(&mut t, 5.0);
    let mut src = box_group(&mut t, 9.0);
    let mut placed = copy_group(&mut t, &mut src, None, false);
    placed.xform = Some(DMat4::from_translation(v3(0., 0., 3.)));
    ClipData {
        faces: vec![(
            square,
            vec![hole],
            FaceAttrs {
                paint: Paint {
                    color: Some([0.8, 0.2, 0.1]),
                    ..Default::default()
                },
                ..Default::default()
            },
        )],
        edges: vec![(v3(0., 0., 0.), v3(0., 0., 4.), true, None)],
        groups: vec![classic, placed],
        meshes: t,
        reference: v3(0., 0., 0.),
    }
}

#[test]
fn test_this_windows_own_copy_is_not_foreign() {
    let mut cb = MemoryClipboard::default();
    publish(&mut cb, clip_data());
    assert!(available(&cb));
    assert!(foreign(&cb).is_none());
}

#[test]
fn test_the_offer_is_rendered_on_demand() {
    let offer = ClipOffer::new(clip_data());
    assert_eq!(offer.formats(), vec![MIME]);
    let data = offer.retrieve_data(MIME).unwrap();
    assert!(decode(data).is_some());
    assert!(offer.retrieve_data("text/plain").is_none());
}

#[test]
fn test_quitting_leaves_the_copy_as_plain_data() {
    let mut cb = MemoryClipboard::default();
    publish(&mut cb, clip_data());
    flush(&mut cb);
    assert!(cb.offer().is_none());
    assert!(decode(&cb.data(MIME).unwrap()).is_some());
}

/// A text clipboard two windows share, as the platform's is.
#[derive(Clone, Default)]
struct SharedText(Rc<RefCell<Option<String>>>);

impl TextStore for SharedText {
    fn get_text(&mut self) -> Option<String> {
        self.0.borrow().clone()
    }
    fn set_text(&mut self, text: String) -> bool {
        *self.0.borrow_mut() = Some(text);
        true
    }
}

fn window_on(store: &SharedText) -> MainWindow {
    let mut w = window();
    w.system_clipboard = Box::new(TextClipboard::new(store.clone()));
    w
}

/// A unit square at `x` in `w`, selected.
fn square_selected(w: &mut MainWindow, x: f64) {
    let mid = w.viewport.scene.active_mesh_id();
    let f = w.viewport.scene.mesh_mut().add_face(
        &[
            v3(x, 0., 0.),
            v3(x + 1., 0., 0.),
            v3(x + 1., 1., 0.),
            v3(x, 1., 0.),
        ],
        &[],
    );
    w.viewport.scene.selection.clear();
    w.viewport.scene.selection.insert(EntityRef::Face(mid, f));
}

fn xs(w: &MainWindow) -> Vec<i64> {
    let m = w.viewport.scene.mesh();
    let mut v: Vec<i64> = m
        .face_ids()
        .iter()
        .flat_map(|&f| m.face_positions(f))
        .map(|p| p.x.round() as i64)
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn test_the_text_survives_the_trip() {
    // The platform clipboard carries the payload as text.
    let bytes = encode(&clip_data()).unwrap();
    let text = bytes_to_text(&bytes);
    assert!(text.starts_with(TEXT_HEADER) && is_clip_text(&text));
    assert_eq!(text_to_bytes(&text).as_deref(), Some(&bytes[..]));
    // a platform that turns line ends into CRLF or wraps the base64
    let crlf = text.replacen('\n', "\r\n", 1);
    assert_eq!(text_to_bytes(&crlf).as_deref(), Some(&bytes[..]));
    let (head, b64) = text.split_once('\n').unwrap();
    let wrapped = format!("{head}\n{}\n{}\n", &b64[..10], &b64[10..]);
    assert_eq!(text_to_bytes(&wrapped).as_deref(), Some(&bytes[..]));
    assert!(decode(&text_to_bytes(&text).unwrap()).is_some());
}

#[test]
fn test_other_text_is_not_a_clipboard() {
    assert!(text_to_bytes("hello").is_none());
    assert!(text_to_bytes(TEXT_HEADER).is_none());
    assert!(!is_clip_text("hello"));
    let store = SharedText::default();
    *store.0.borrow_mut() = Some("hello".into());
    let cb = TextClipboard::new(store);
    assert!(!available(&cb));
    assert!(foreign(&cb).is_none());
}

#[test]
fn test_a_copy_crosses_to_another_window_when_it_loses_focus() {
    let store = SharedText::default();
    let mut a = window_on(&store);
    let mut b = window_on(&store);
    square_selected(&mut a, 0.);
    a.trigger("copy");
    // Ctrl+C only takes the clipboard over: the bytes wait for the switch
    assert_eq!(store.0.borrow().as_deref(), Some(TEXT_HEADER));
    assert!(foreign(b.system_clipboard.as_ref()).is_none());
    a.on_focus_lost();
    assert!(store.0.borrow().as_deref().is_some_and(is_clip_text));
    assert!(
        foreign(a.system_clipboard.as_ref()).is_none(),
        "still A's own"
    );

    // B has no copy of its own: Paste takes A's
    assert!(b.context_menu().iter().any(|i| i.text() == "Paste"));
    b.trigger("paste_in_place");
    assert_eq!(xs(&b), [0, 1]);

    // B copies in turn; back in A, Paste takes B's newer copy
    square_selected(&mut b, 5.);
    b.trigger("copy");
    b.on_focus_lost();
    a.viewport.scene.selection.clear();
    a.trigger("paste_in_place");
    assert_eq!(xs(&a), [0, 1, 5, 6]);
}

#[test]
fn test_quitting_writes_the_copy_the_window_still_holds() {
    let store = SharedText::default();
    let mut a = window_on(&store);
    square_selected(&mut a, 0.);
    a.trigger("copy");
    a.request_quit();
    assert!(a.quit_requested);
    let text = store.0.borrow().clone().unwrap();
    let mut b = window_on(&store);
    assert!(decode(&text_to_bytes(&text).unwrap()).is_some());
    b.trigger("paste_in_place");
    assert_eq!(xs(&b), [0, 1]);
}

#[test]
fn test_a_new_window_runs_this_program() {
    // test_a_new_window_from_the_repo_runs_main_py has no Rust form (there
    // is no script); a new window starts this very executable.
    assert_eq!(
        MainWindow::new_window_command(),
        std::env::current_exe().ok()
    );
}
