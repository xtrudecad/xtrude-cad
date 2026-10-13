// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Counts, radii and arrays typed in the value box reach the tool's
//! `on_value` only when its `ValueSpec` takes them.

use crate::common;

use common::{stub, MetreDocument};
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::{Tool, ToolInfo, ToolValue};
use xtrude_app::value_box::{ArrayKind, ArraySpec, PatternEntry, ValueSpec};
use xtrude_app::viewport::Viewport;

/// A tool that only records what the box hands it.
struct Recorder {
    spec: ValueSpec,
    got: Rc<RefCell<Vec<ToolValue>>>,
}

impl Tool for Recorder {
    xtrude_app::tool_any!();

    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo::new("recorder", "Recorder");
        &INFO
    }

    fn value_spec(&self) -> ValueSpec {
        self.spec
    }

    fn on_value(&mut self, _vp: &mut Viewport, value: &ToolValue) -> bool {
        self.got.borrow_mut().push(value.clone());
        true
    }
}

fn return_key() -> KeyEvent {
    KeyEvent::new(Key::Return, "", Modifiers::NONE)
}

/// What a tool with `spec` receives for `typed` + Enter.
fn enter(spec: ValueSpec, typed: &str) -> Vec<ToolValue> {
    let _m = MetreDocument::new();
    let mut vp = stub();
    let got = Rc::new(RefCell::new(Vec::new()));
    vp.set_active_tool(Some(Box::new(Recorder {
        spec,
        got: got.clone(),
    })));
    vp.set_value_buffer(typed);
    assert!(vp.handle_value_key(&return_key()));
    let v = got.borrow().clone();
    v
}

#[test]
fn test_segments_and_radius_reach_only_a_tool_that_takes_them() {
    let none = ValueSpec::default();
    let both = ValueSpec {
        segments: true,
        radius: true,
        ..ValueSpec::default()
    };
    assert_eq!(enter(both, "6s"), vec![ToolValue::Segments(6)]);
    assert_eq!(enter(both, "2r"), vec![ToolValue::Radius(2.0)]);
    assert!(enter(none, "6s").is_empty());
    assert!(enter(none, "2r").is_empty());
}

#[test]
fn test_an_array_step_reaches_only_a_tool_that_takes_one() {
    let count_only = ValueSpec {
        array: Some(ArraySpec { takes_step: false }),
        ..ValueSpec::default()
    };
    let with_step = ValueSpec {
        array: Some(ArraySpec { takes_step: true }),
        ..ValueSpec::default()
    };
    let three = ToolValue::Array {
        count: 3,
        kind: ArrayKind::External,
        step: None,
    };
    assert_eq!(enter(count_only, "3x"), vec![three.clone()]);
    assert_eq!(enter(with_step, "3x"), vec![three]);
    assert!(enter(count_only, "5x10m").is_empty());
    assert_eq!(
        enter(with_step, "5x10m"),
        vec![ToolValue::Array {
            count: 5,
            kind: ArrayKind::External,
            step: Some(10.0),
        }]
    );
    assert!(enter(ValueSpec::default(), "3x").is_empty());
}

fn pattern(count: u32, step: Option<f64>, spread: bool) -> Vec<ToolValue> {
    vec![ToolValue::Pattern(PatternEntry {
        count,
        step,
        spread,
    })]
}

#[test]
fn test_a_pattern_takes_its_count_bare_and_its_spacing_as_a_length() {
    let spec = ValueSpec {
        pattern: true,
        comma_lists: true,
        ..ValueSpec::default()
    };
    assert_eq!(enter(spec, "4"), pattern(4, None, false));
    assert_eq!(enter(spec, "4;15mm"), pattern(4, Some(0.015), false));
    assert_eq!(enter(spec, "4 2"), pattern(4, Some(2.0), false));
    assert_eq!(enter(spec, "4,2"), pattern(4, Some(2.0), false));
    assert_eq!(enter(spec, "4;1,5"), pattern(4, Some(1.5), false));
    assert_eq!(enter(spec, "4/"), pattern(4, None, true));
    assert_eq!(enter(spec, "4/45mm"), pattern(4, Some(0.045), true));
    // the count is no length: a millimetre document leaves it alone
    let _mm = MetreDocument::new();
    xtrude_model::units::set_override(Some(xtrude_model::units::Units::new("mm", 1)));
    let mut vp = stub();
    let got = Rc::new(RefCell::new(Vec::new()));
    vp.set_active_tool(Some(Box::new(Recorder {
        spec,
        got: got.clone(),
    })));
    vp.set_value_buffer("4;15");
    assert!(vp.handle_value_key(&return_key()));
    let got = got.borrow().clone();
    let [ToolValue::Pattern(p)] = got.as_slice() else {
        panic!("{got:?}");
    };
    assert_eq!(p.count, 4);
    assert!((p.step.unwrap() - 0.015).abs() < 1e-12, "{p:?}");
}

#[test]
fn test_a_circular_pattern_takes_its_angle_unitless() {
    let spec = ValueSpec {
        pattern: true,
        unitless: true,
        comma_lists: true,
        ..ValueSpec::default()
    };
    assert_eq!(enter(spec, "6;60"), pattern(6, Some(60.0), false));
    assert_eq!(enter(spec, "5/90"), pattern(5, Some(90.0), true));
}

#[test]
fn test_count_and_spacing_are_two_lengths_outside_a_pattern() {
    let spec = ValueSpec {
        comma_lists: true,
        ..ValueSpec::default()
    };
    assert_eq!(enter(spec, "4;15"), vec![ToolValue::Pair(4.0, 15.0)]);
}

#[test]
fn test_typed_sides_set_the_circle_polygon() {
    let mut vp = stub();
    vp.set_active_tool(Some(Box::new(RadialTool::polygon())));
    vp.set_value_buffer("8s");
    assert!(vp.handle_value_key(&return_key()));
    assert_eq!(vp.tool::<RadialTool>().unwrap().sides, 8);
}
