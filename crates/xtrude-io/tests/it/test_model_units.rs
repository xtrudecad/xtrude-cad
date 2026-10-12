// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Model units through .igz.

use crate::common;
use common::*;
use xtrude_io::igz;
use xtrude_model::scene::Scene;
use xtrude_model::units::Units;

#[test]
fn test_units_travel_in_the_igz_and_old_files_read_as_metres() {
    let mut s = Scene::new();
    s.units = Units::new("cm", 1);
    let back = roundtrip(&s);
    assert_eq!(back.units, Units::new("cm", 1));
    let tmp = TmpDir::new();
    let plain = Scene::new();
    igz::save_scene(&plain, &tmp.join("mm.igz")).unwrap();
    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.join("mm.igz")).unwrap()).unwrap();
    assert_eq!(
        doc["scene"]["units"],
        serde_json::json!({"length": "mm", "precision": 2})
    );
    doc["scene"].as_object_mut().unwrap().remove("units");
    std::fs::write(tmp.join("old.igz"), doc.to_string()).unwrap();
    let mut again = Scene::new();
    igz::load_into(&mut again, &tmp.join("old.igz"), None).unwrap();
    assert_eq!(again.units, Units::new("m", 2));
    s.clear();
    assert_eq!(s.units, Units::new("mm", 2));
}
