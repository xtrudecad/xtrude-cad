// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Circle and Polygon read and take millimetres in a millimetre document; the
//! value box shows the unit a bare number will be read in; no tool writes
//! metres by hand.

use crate::common;

use common::stub;
use glam::DVec3;
use xtrude_app::tools::circle::RadialTool;
use xtrude_app::tools::{Tool, ToolContext, ToolValue};
use xtrude_app::value_box::{parse_value_buffer, Typed};
use xtrude_model::i18n::tr;
use xtrude_model::units::{bind, default_model_units, typed_value_text, Units};

/// A millimetre document, 0 decimals, bound.
struct MmScene;

impl MmScene {
    fn new() -> MmScene {
        bind(&Units::new("mm", 0));
        MmScene
    }
}

impl Drop for MmScene {
    fn drop(&mut self) {
        bind(&default_model_units());
    }
}

#[test]
fn test_circle_and_polygon_read_and_take_millimetres() {
    let _mm = MmScene::new();
    for (mut tool, sides) in [(RadialTool::circle(), 24), (RadialTool::polygon(), 6)] {
        let mut vp = stub();
        vp.scene.units = Units::new("mm", 0);
        vp.scene.bind_units();
        tool.on_click(&mut vp, &ToolContext::at(DVec3::ZERO)); // centre
        tool.on_hover(&mut vp, &ToolContext::at(DVec3::new(0.3, 0., 0.))); // 300 mm away
        let (text, _) = tool.value_label(&vp).unwrap();
        let n = sides.to_string();
        assert_eq!(
            text,
            format!("R 300 mm  ({})", tr("{n} sides", &[("n", &n)]))
        );
        // typing «500» + Enter: read in the document's unit
        let Some(Typed::Value(v)) = parse_value_buffer("500", false) else {
            panic!("a value");
        };
        assert!((v - 0.5).abs() < 1e-12);
        assert!(tool.on_value(&mut vp, &ToolValue::Num(v)));
        let m = vp.scene.mesh();
        let f = m.face_ids()[0];
        let ps = m.face_positions(f);
        assert_eq!(ps.len(), sides);
        assert!(ps.iter().all(|p| (p.length() - 0.5).abs() < 1e-6)); // 500 mm radius
    }
}

#[test]
fn test_what_is_typed_shows_the_unit_it_will_be_read_in() {
    let _mm = MmScene::new();
    assert_eq!(typed_value_text("500"), "500 mm");
    assert_eq!(typed_value_text("300,200"), "300,200 mm");
    assert_eq!(typed_value_text("2m"), "2m"); // its own unit wins
    assert_eq!(typed_value_text("24s"), "24s");
    bind(&Units::new("m", 2));
    assert_eq!(typed_value_text("2"), "2 m");
    bind(&Units::new("ft-in", 0));
    assert_eq!(typed_value_text("6"), "6\""); // inches, as parsed
}

/// A format placeholder followed by a literal metre sign: `{} m`,
/// `{:.2} m` — not `m²`, not a word.
fn writes_metres(line: &str) -> bool {
    let b = line.as_bytes();
    let mut i = 0;
    while let Some(open) = line[i..].find('{').map(|k| k + i) {
        let Some(close) = line[open..].find('}').map(|k| k + open) else {
            break;
        };
        let mut j = close + 1;
        if b.get(j) == Some(&b' ') {
            j += 1;
        }
        if b.get(j) == Some(&b'm') {
            let next = line[j + 1..].chars().next();
            let word = next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '²');
            if !word && next.is_some() {
                return true;
            }
        }
        i = close + 1;
    }
    false
}

#[test]
fn test_no_tool_writes_metres_by_hand() {
    assert!(writes_metres(r#"format!("{:.2} m", d)"#)); // the pattern bites
    assert!(!writes_metres(r#"format!("{:.2} m²", a)"#));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
    let mut offenders = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for path in files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
    {
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if writes_metres(code) {
                offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}
