// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Section Plane tool ends after one plane, wears the inference colours,
//! and switches the cuts back on; and the name prompt can be switched off.

use crate::common;
use xtrude_core::Axis;

use std::cell::RefCell;
use std::rc::Rc;

use common::{stub, v3, window};
use xtrude_app::tools::section::SectionPlaneTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::{Viewport, ViewportEvent};
use xtrude_app::window::dialogs::SectionName;
use xtrude_app::window::ScriptedDialogs;
use xtrude_core::snap::COLOR_REFERENCE;
use xtrude_model::section::SectionPlane;

fn place(vp: &mut Viewport) {
    let mut tool = SectionPlaneTool::new();
    tool.set_normal(v3(0., 0., 1.));
    tool.on_click(vp, &ToolContext::at(v3(0., 0., 1.)));
}

#[test]
fn test_one_plane_then_back_to_select() {
    let mut vp = stub();
    place(&mut vp);
    assert_eq!(vp.scene.doc.section_planes.len(), 1);
    let id = vp.scene.doc.section_planes[0].id;
    let events: Vec<ViewportEvent> = std::mem::take(&mut vp.outbox.events);
    assert!(events.contains(&ViewportEvent::PromptSectionName(id)));
    let activated: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            ViewportEvent::ActivateTool(k) => Some(k),
            _ => None,
        })
        .collect();
    assert_eq!(activated, vec!["select"]);
}

#[test]
fn test_the_floating_plane_wears_the_inference_colours() {
    let mut tool = SectionPlaneTool::new();
    for (axis, n) in [
        (Axis::X, v3(1., 0., 0.)),
        (Axis::Y, v3(0., -1., 0.)),
        (Axis::Z, v3(0., 0., 1.)),
    ] {
        tool.set_normal(n);
        let c = tool.wireframe_color().unwrap();
        let want = axis.color();
        assert_eq!([c[0], c[1], c[2]], [want[0], want[1], want[2]], "{axis:?}");
    }
    tool.set_normal(v3(1., 1., 0.).normalize()); // a face off the axes
    let c = tool.wireframe_color().unwrap();
    assert_eq!([c[0], c[1], c[2]], COLOR_REFERENCE);
}

#[test]
fn test_placing_a_plane_switches_the_cuts_back_on() {
    let mut vp = stub();
    vp.scene.show_section_cuts = false;
    vp.scene.show_section_planes = false;
    place(&mut vp);
    // (the menu checkmarks read the scene's flags each frame)
    assert!(vp.scene.show_section_cuts && vp.scene.show_section_planes);
}

#[test]
fn test_the_name_prompt_can_be_switched_off() {
    let mut win = window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    dialogs.borrow_mut().section_names.push(Some(SectionName {
        name: "Other".into(),
        symbol: "B".into(),
        dont_ask: false,
    }));
    win.dialogs = Box::new(dialogs.clone());
    xtrude_model::settings::set("section/ask_name", serde_json::Value::Bool(false));
    let plane = SectionPlane::new(v3(0., 0., 0.), v3(0., 0., 1.), "Section 1", "A", true, None);
    let id = plane.id;
    win.viewport.scene.doc.section_planes.push(plane);
    win.prompt_section_name(id); // returns without a dialog
    let p = &win.viewport.scene.doc.section_planes[0];
    assert!(p.name == "Section 1" && p.symbol == "A");
    assert!(dialogs.borrow().asked.is_empty());
}
