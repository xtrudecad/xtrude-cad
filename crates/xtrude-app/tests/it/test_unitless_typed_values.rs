// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A tool whose value is not a length says so (`ValueSpec::unitless`) and the
//! number reaches it as typed; lengths keep the document's unit.

use crate::common;

use common::stub;
use glam::DVec3;
use std::cell::RefCell;
use std::rc::Rc;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::scale::ScaleTool;
use xtrude_app::tools::{Tool, ToolInfo, ToolValue};
use xtrude_app::value_box::ValueSpec;
use xtrude_app::viewport::Viewport;
use xtrude_model::units::{bare_number_scale, Units, UnitsOverride};

/// A tool that only records what the value box hands it.
struct Recorder {
    unitless: bool,
    got: Rc<RefCell<Vec<ToolValue>>>,
}

impl Tool for Recorder {
    xtrude_app::tool_any!();

    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo::new("recorder", "Recorder");
        &INFO
    }

    fn value_spec(&self) -> ValueSpec {
        ValueSpec {
            unitless: self.unitless,
            ..ValueSpec::default()
        }
    }

    fn on_value(&mut self, _vp: &mut Viewport, value: &ToolValue) -> bool {
        self.got.borrow_mut().push(value.clone());
        true
    }
}

fn return_key() -> KeyEvent {
    KeyEvent::new(Key::Return, "", Modifiers::NONE)
}

fn enter(unitless: bool, typed: &str) -> ToolValue {
    let mut vp = stub();
    let got = Rc::new(RefCell::new(Vec::new()));
    vp.set_active_tool(Some(Box::new(Recorder {
        unitless,
        got: got.clone(),
    })));
    vp.set_value_buffer(typed);
    assert!(vp.handle_value_key(&return_key()));
    let v = got.borrow().last().cloned().expect("a value");
    v
}

fn num(v: ToolValue) -> f64 {
    match v {
        ToolValue::Num(x) => x,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn test_an_angle_or_a_count_arrives_as_typed_in_every_unit() {
    for unit in ["m", "mm", "cm", "in", "ft-in"] {
        let _u = UnitsOverride::with(Units::new(unit, 2));
        assert!((num(enter(true, "45")) - 45.0).abs() < 1e-12, "{unit}");
        match enter(true, "2;3") {
            ToolValue::Pair(a, b) => assert!((a - 2.0).abs() < 1e-9 && (b - 3.0).abs() < 1e-9),
            other => panic!("{unit}: {other:?}"),
        }
        assert!((num(enter(true, "-90")) + 90.0).abs() < 1e-9, "{unit}");
    }
}

#[test]
fn test_a_length_still_takes_the_documents_unit() {
    {
        let _u = UnitsOverride::with(Units::new("mm", 0));
        assert!((num(enter(false, "45")) - 0.045).abs() < 1e-12);
    }
    let _u = UnitsOverride::with(Units::new("in", 0));
    assert!((num(enter(false, "2")) - 2.0 * 0.0254).abs() < 1e-12);
}

#[test]
fn test_a_unit_typed_on_purpose_still_converts() {
    // Scale's «2m» is the new absolute size, not a factor.
    let _u = UnitsOverride::with(Units::new("mm", 0));
    assert!((num(enter(true, "2m")) - 2.0).abs() < 1e-12);
}

#[test]
fn test_the_unitless_reading_ends_with_the_entry() {
    let _u = UnitsOverride::with(Units::new("mm", 0));
    enter(true, "45");
    assert!((bare_number_scale() - 0.001).abs() < 1e-12);
}

#[test]
fn test_which_tools_take_a_number_that_is_not_a_length() {
    assert!(RotateTool::new().value_spec().unitless); // the angle
    assert!(ProtractorTool::new().value_spec().unitless); // the angle
    assert!(ScaleTool::new().value_spec().unitless); // the factor
    for mut t in [RadialTool::circle(), RadialTool::polygon()] {
        assert!(t.value_spec().unitless); // sides, before the centre
        t.start_point = Some(DVec3::ZERO);
        assert!(!t.value_spec().unitless); // the radius after it
    }
    assert!(!MoveTool::new().value_spec().unitless); // a distance
}

#[test]
fn test_polygon_sides_survive_a_millimetre_or_inch_model() {
    for unit in ["mm", "in"] {
        let _u = UnitsOverride::with(Units::new(unit, 0));
        let mut vp = stub();
        vp.set_active_tool(Some(Box::new(RadialTool::polygon())));
        vp.set_value_buffer("8");
        vp.handle_value_key(&return_key());
        assert_eq!(vp.tool::<RadialTool>().unwrap().sides, 8, "{unit}");
    }
}
