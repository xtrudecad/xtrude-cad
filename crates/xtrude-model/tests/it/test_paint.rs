// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Glass paint and the RAL palette.

use crate::common;
use common::*;
use serde_json::Value;
use xtrude_model::commands::{SetFaceColorCommand, SetFaceOpacityCommand};
use xtrude_model::history::{Command, CompoundCommand};

#[test]
fn test_glass_paints_translucent_and_undoes() {
    let (mut s, mut h) = scene_history();
    let f = s
        .mesh_mut()
        .add_face(&[V(0., 0.), V(1., 0.), V(1., 1.), V(0., 1.)], &[]);
    let mid = s.active_mesh_id();
    let paint = |c: [f64; 3], o: Option<f64>| -> CompoundCommand {
        let cmds: Vec<Box<dyn Command>> = vec![
            Box::new(SetFaceColorCommand {
                faces: vec![(mid, f)],
                color: Some(c),
            }),
            Box::new(SetFaceOpacityCommand {
                faces: vec![(mid, f)],
                opacity: o,
            }),
        ];
        CompoundCommand::new(cmds)
    };
    h.execute(&mut s, &mut paint([0.7, 0.85, 0.9], Some(0.45)));
    assert_eq!(s.mesh().face(f).attrs.paint.opacity, Some(0.45));
    h.execute(&mut s, &mut paint([1.0, 0.0, 0.0], None));
    assert_eq!(s.mesh().face(f).attrs.paint.opacity, None);
    assert!(h.undo(&mut s) && s.mesh().face(f).attrs.paint.opacity == Some(0.45));
    assert!(h.undo(&mut s) && s.mesh().face(f).attrs.paint.opacity.is_none());
}

fn ral() -> Value {
    let p =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/colors/ral.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("ral.json")).expect("valid json")
}

fn colours(data: &Value) -> Vec<&Value> {
    data["families"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["colors"].as_array().unwrap())
        .collect()
}

#[test]
fn test_the_ral_palette_is_complete_and_well_formed() {
    let data = ral();
    let cs = colours(&data);
    assert_eq!(cs.len(), 213, "RAL Classic is 213 colours");
    let codes: std::collections::HashSet<&str> =
        cs.iter().map(|c| c["code"].as_str().unwrap()).collect();
    assert_eq!(codes.len(), cs.len(), "a code appears twice");
    for c in cs {
        let code = c["code"].as_str().unwrap();
        assert!(code.starts_with("RAL "));
        let rgb = c["rgb"].as_array().unwrap();
        assert_eq!(rgb.len(), 3);
        assert!(
            rgb.iter()
                .all(|v| (0.0..=1.0).contains(&v.as_f64().unwrap())),
            "{code}"
        );
        let name = c["name"].as_str().unwrap();
        assert!(
            !name.is_empty() && name.chars().next().unwrap().is_uppercase(),
            "{code}"
        );
    }
}

#[test]
fn test_a_few_ral_colours_are_the_ones_the_standard_says() {
    let data = ral();
    let by = |code: &str| {
        colours(&data)
            .into_iter()
            .find(|c| c["code"] == code)
            .unwrap()
            .clone()
    };
    let hex_of = |code: &str| {
        by(code)["rgb"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                format!(
                    "{:02X}",
                    xtrude_core::math::round_even_int(v.as_f64().unwrap() * 255.0)
                )
            })
            .collect::<String>()
    };
    assert_eq!(hex_of("RAL 9010"), "F7F9EF");
    assert_eq!(hex_of("RAL 9005"), "0A0A0D");
    assert_eq!(hex_of("RAL 7035"), "CBD0CC");
    assert_eq!(hex_of("RAL 3020"), "C1121C");
    assert_eq!(by("RAL 7035")["name"], "Light grey");
    assert_eq!(by("RAL 3020")["name"], "Traffic red");
}

#[test]
fn test_every_ral_colour_is_in_exactly_one_family() {
    let data = ral();
    let fams = data["families"].as_array().unwrap();
    let ids: std::collections::HashSet<&str> =
        fams.iter().map(|f| f["id"].as_str().unwrap()).collect();
    assert!(ids.len() == fams.len() && fams.len() == 9);
    let mut seen = std::collections::HashSet::new();
    for fam in fams {
        assert!(!fam["name"].as_str().unwrap().is_empty());
        let mut digit = std::collections::HashSet::new();
        for c in fam["colors"].as_array().unwrap() {
            let code = c["code"].as_str().unwrap().to_string();
            digit.insert(code.chars().nth(4));
            assert!(seen.insert(code));
        }
        assert_eq!(digit.len(), 1, "{}", fam["id"]);
    }
}
