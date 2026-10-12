// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Command parameters: the manifest's declarations, the values' types in
//! Lua, their bounds, and how the settings keep them.

use serde_json::json;
use xtrude_ext::{ErrorKind, Extension, ParamKind, ParamSpec, ParamValue};
use xtrude_model::units::Units;
use xtrude_model::{History, Scene};

use crate::common::TmpDir;

const GEAR: &str = r#"
return { id = "gear", name = "Gear", api = 1, commands = {
  { id = "spur", label = "Spur gear…",
    params = {
      { key = "teeth", label = "Teeth", type = "int", default = 20, min = 6, max = 200 },
      { key = "ratio", label = "Ratio", type = "number", default = 0.5 },
      { key = "width", label = "Width", type = "length", default = 5.0, min = 0.1 },
      { key = "hub", label = "Hub", type = "bool", default = true },
      { key = "shape", label = "Shape", type = "choice", choices = { "round", "square" }, default = "square" },
    },
    run = function(doc, p)
      assert(math.type(p.teeth) == "integer", "teeth: " .. math.type(p.teeth))
      assert(math.type(p.ratio) == "float", "ratio")
      assert(type(p.width) == "number", "width")
      assert(type(p.hub) == "boolean", "hub")
      assert(type(p.shape) == "string", "shape")
      local w = p.width
      doc:add_face({ xtrude.vec(0, 0), xtrude.vec(w, 0), xtrude.vec(w, w), xtrude.vec(0, w) })
      local g = doc:make_group(doc:faces(), p.shape .. " " .. p.teeth .. " " .. tostring(p.hub))
    end },
} }
"#;

fn gear(dir: &TmpDir) -> Extension {
    dir.write("gear/init.lua", GEAR);
    Extension::load(&dir.0.join("gear")).expect("loads")
}

/// Runs `spur` with `values` in a document in `unit`; the scene, if it ran.
fn run(ext: &Extension, unit: &str, values: &[ParamValue], log: &TmpDir) -> Option<Scene> {
    xtrude_model::settings::use_memory_store();
    let mut scene = Scene::new();
    scene.units = Units::new(unit, 2);
    let mut history = History::new();
    history.error_log = Some(log.0.join("errors.log"));
    let mut cmd = ext.script_command("spur", values)?.ok()?;
    history.execute(&mut scene, &mut cmd).then_some(scene)
}

#[test]
fn the_manifest_declares_typed_params() {
    let dir = TmpDir::new();
    let ext = gear(&dir);
    let cmd = ext.command("spur").expect("the command");
    assert_eq!(
        cmd.params[0],
        ParamSpec {
            key: "teeth".into(),
            label: "Teeth".into(),
            kind: ParamKind::Int {
                min: Some(6),
                max: Some(200)
            },
            default: ParamValue::Int(20),
        }
    );
    assert_eq!(
        cmd.params[4].kind,
        ParamKind::Choice(vec!["round".into(), "square".into()])
    );
    assert_eq!(
        cmd.defaults(),
        [
            ParamValue::Int(20),
            ParamValue::Number(0.5),
            ParamValue::Length(5.0),
            ParamValue::Bool(true),
            ParamValue::Choice(1),
        ]
    );
}

#[test]
fn values_reach_lua_with_their_types_and_units() {
    let dir = TmpDir::new();
    let ext = gear(&dir);
    let values = [
        ParamValue::Int(12),
        ParamValue::Number(1.0),
        ParamValue::Length(20.0),
        ParamValue::Bool(false),
        ParamValue::Choice(0),
    ];
    let mm = run(&ext, "mm", &values, &dir).expect("runs in mm");
    let cm = run(&ext, "cm", &values, &dir).expect("runs in cm");
    let size = |s: &Scene| {
        let (lo, hi) = s.compute_bounds().expect("geometry");
        (hi - lo).x
    };
    assert!((size(&mm) - 0.02).abs() < 1e-9);
    assert!((size(&cm) - 0.2).abs() < 1e-9);
    let names: Vec<String> = mm.doc.groups.iter().map(|g| g.name.clone()).collect();
    assert_eq!(names, ["round 12 false"]);
}

#[test]
fn values_out_of_bounds_or_of_another_type_are_refused() {
    let dir = TmpDir::new();
    let ext = gear(&dir);
    let mut values = ext.command("spur").expect("the command").defaults();
    values[0] = ParamValue::Int(3);
    let err = ext
        .script_command("spur", &values)
        .expect("the command")
        .err()
        .expect("refused");
    assert!(
        err.message.contains("Teeth must be between 6 and 200"),
        "{err:?}"
    );
    values[0] = ParamValue::Number(20.0);
    assert!(ext.script_command("spur", &values).expect("cmd").is_err());
    values[0] = ParamValue::Int(20);
    values[4] = ParamValue::Choice(2);
    assert!(ext.script_command("spur", &values).expect("cmd").is_err());
    assert!(ext
        .script_command("spur", &values[..2])
        .expect("cmd")
        .is_err());
    values[4] = ParamValue::Choice(0);
    values[2] = ParamValue::Length(f64::NAN);
    assert!(ext.script_command("spur", &values).expect("cmd").is_err());
}

#[test]
fn bad_param_declarations_are_manifest_errors() {
    for (params, reason) in [
        ("{ key = 'n', type = 'colour' }", "unknown type"),
        (
            "{ key = 'n', type = 'int', default = 2, min = 5 }",
            "default is not valid",
        ),
        ("{ key = 'n', type = 'int', default = 2.5 }", "whole number"),
        (
            "{ key = 'c', type = 'choice', choices = { 'a' }, default = 'b' }",
            "not a choice",
        ),
        ("{ key = 'c', type = 'choice' }", "'choices'"),
        ("{ type = 'int' }", "'key'"),
        (
            "{ key = 'n', type = 'int' }, { key = 'n', type = 'bool' }",
            "two parameters",
        ),
    ] {
        let dir = TmpDir::new();
        dir.write(
            "e/init.lua",
            &format!(
                "return {{ id = 'e', name = 'E', api = 1, commands = {{ \
                 {{ id = 'c', label = 'C', params = {{ {params} }}, run = function() end }} }} }}"
            ),
        );
        let err = Extension::load(&dir.0.join("e")).err().expect(params);
        assert_eq!(err.kind, ErrorKind::Manifest);
        assert!(err.message.contains(reason), "{params}: {err:?}");
    }
}

#[test]
fn stored_values_keep_lengths_in_metres_and_choices_by_text() {
    let dir = TmpDir::new();
    let ext = gear(&dir);
    let specs = &ext.command("spur").expect("the command").params;
    let mm = 0.001;
    assert_eq!(specs[2].to_json(&ParamValue::Length(5.0), mm), json!(0.005));
    assert_eq!(specs[4].to_json(&ParamValue::Choice(0), mm), json!("round"));
    assert_eq!(
        specs[2].from_json(&json!(0.005), 0.01),
        Some(ParamValue::Length(0.5))
    );
    assert_eq!(
        specs[4].from_json(&json!("square"), mm),
        Some(ParamValue::Choice(1))
    );
    // no longer valid: out of bounds, of another type, an unknown choice
    assert_eq!(specs[0].from_json(&json!(500), mm), None);
    assert_eq!(specs[0].from_json(&json!("20"), mm), None);
    assert_eq!(specs[4].from_json(&json!("hexagon"), mm), None);
    assert_eq!(
        specs[3].from_json(&json!(false), mm),
        Some(ParamValue::Bool(false))
    );
}
