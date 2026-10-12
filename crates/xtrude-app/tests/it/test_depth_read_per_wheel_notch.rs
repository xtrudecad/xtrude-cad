// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The shell reads the frame's depth back from the GPU only before an
//! input that looks at it: a press, or a wheel notch that picks its zoom
//! focus. The later notches of a run whose cursor stays put reuse the
//! first notch's focus and read nothing, and zoom exactly as before.

use crate::common;

use common::{add_box, sized_viewport, v3, Gpu};
use xtrude_app::input::{Button, Input, MouseEvent};
use xtrude_app::viewport::Viewport;

fn boxed() -> Viewport {
    let mut vp = sized_viewport(800.0, 600.0);
    vp.scene.guides.clear();
    add_box(&mut vp, v3(-1., -1., 0.), v3(1., 1., 2.));
    vp.camera.set_view("iso");
    vp.camera.fit_to(v3(-3., -3., -1.), v3(3., 3., 3.), 1.3);
    vp
}

fn notch(x: f64, y: f64) -> Input {
    Input::Wheel { x, y, delta: 120.0 }
}

/// A run of notches as the shell handles it, a frame drawn after each:
/// how many of them read the depth.
fn depth_reads(vp: &mut Viewport, at: &[(f64, f64)]) -> usize {
    let mut reads = 0;
    for &(x, y) in at {
        vp.frame_data([800, 600], false);
        let ev = notch(x, y);
        if vp.wants_depth_for(&ev) {
            reads += 1;
        }
        vp.wheel(x, y, 120.0);
    }
    reads
}

#[test]
fn test_a_run_of_notches_with_the_cursor_still_reads_the_depth_once() {
    let mut vp = boxed();
    assert_eq!(depth_reads(&mut vp, &[(400., 300.); 10]), 1);
}

#[test]
fn test_a_cursor_that_moves_away_reads_it_again() {
    let mut vp = boxed();
    let at = [(400., 300.), (410., 305.), (500., 300.), (500., 300.)];
    assert_eq!(depth_reads(&mut vp, &at), 2);
}

#[test]
fn test_an_edit_between_notches_reads_it_again() {
    let mut vp = boxed();
    assert_eq!(depth_reads(&mut vp, &[(400., 300.); 2]), 1);
    vp.scene.version += 1;
    assert_eq!(depth_reads(&mut vp, &[(400., 300.)]), 1);
}

#[test]
fn test_a_press_still_reads_it_and_other_inputs_do_not() {
    let mut vp = boxed();
    depth_reads(&mut vp, &[(400., 300.)]);
    vp.frame_data([800, 600], false);
    assert!(!vp.wants_depth_for(&notch(400., 300.)));
    assert!(vp.wants_depth_for(&Input::Press(MouseEvent::button(
        400.,
        300.,
        Button::Middle,
        true
    ))));
    assert!(!vp.wants_depth_for(&Input::Move(MouseEvent::at(400., 300.))));
}

#[test]
fn test_the_zoom_lands_where_reading_on_every_notch_did() {
    let Some(mut gpu) = Gpu::new() else {
        eprintln!("no wgpu adapter: skipped");
        return;
    };
    let at = [(330., 280.); 6]
        .into_iter()
        .chain([(450., 340.); 6])
        .collect::<Vec<_>>();
    let run = |gpu: &mut Gpu, every_notch: bool| {
        let mut vp = boxed();
        let mut reads = 0;
        for &(x, y) in &at {
            gpu.grab(&mut vp);
            if every_notch {
                if vp.wants_depth() {
                    gpu.depth_for_input(&mut vp);
                    reads += 1;
                }
            } else if gpu.depth_before(&mut vp, &notch(x, y)) {
                reads += 1;
            }
            vp.wheel(x, y, 120.0);
        }
        let c = &vp.camera;
        (reads, (c.yaw, c.pitch, c.distance, c.target))
    };
    let (every, before) = run(&mut gpu, true);
    let (lazy, after) = run(&mut gpu, false);
    assert_eq!(every, at.len());
    assert_eq!(lazy, 2);
    assert_eq!(after, before);
}
