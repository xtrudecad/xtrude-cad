// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Model units through the document.

use crate::common;
use common::*;
use xtrude_io::{document, igz};
use xtrude_model::scene::Scene;
use xtrude_model::units::Units;

#[test]
fn test_units_travel_in_the_document_and_old_files_read_as_metres() {
    let mut s = Scene::new();
    s.units = Units::new("cm", 1);
    let back = roundtrip(&s);
    assert_eq!(back.units, Units::new("cm", 1));
    let tmp = TmpDir::new();
    let plain = Scene::new();
    document::save_scene(&plain, &tmp.join("mm.xtrude")).unwrap();
    let doc = document_json(&tmp.join("mm.xtrude"));
    assert_eq!(
        doc["scene"]["units"],
        serde_json::json!({"length": "mm", "precision": 2})
    );
    // an IngeTrazo document from before documents carried their units
    let mut old = igz_json(&plain);
    old["scene"].as_object_mut().unwrap().remove("units");
    std::fs::write(tmp.join("old.igz"), old.to_string()).unwrap();
    let mut again = Scene::new();
    igz::load_into(&mut again, &tmp.join("old.igz"), None).unwrap();
    assert_eq!(again.units, Units::new("m", 2));
    s.clear();
    assert_eq!(s.units, Units::new("mm", 2));
}
