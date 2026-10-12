// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A registry identity exports under its own (sanitized, unique) name in OBJ,
//! DAE and glTF.

use crate::common;
use common::*;
use indexmap::IndexMap;
use xtrude_io::meshexport::{export_names, MaterialInfo, MaterialKey};
use xtrude_io::{dae, gltf, obj};
use xtrude_model::scene::Scene;

fn scene_two_materials() -> Scene {
    let mut s = Scene::new();
    let m = s.mesh_mut();
    let f1 = quad(m, 0., 2., 0., 2., 0.);
    m.face_mut(f1).attrs.paint.color = Some([0.62, 0.60, 0.58]);
    m.face_mut(f1).attrs.paint.mat = Some("Matte grey".into());
    let f2 = quad(m, 3., 5., 0., 2., 0.);
    m.face_mut(f2).attrs.paint.color = Some([0.8, 0.1, 0.1]);
    s
}

#[test]
fn test_names_sanitized_unique_and_fallback() {
    let mut mats = IndexMap::new();
    for (c, mat) in [
        (0.1, Some("Matte grey")),
        (0.2, Some("Matte grey")),
        (0.3, None),
        (0.4, Some("1º coat / fine")),
    ] {
        mats.insert(
            MaterialKey {
                color: [c, c, c],
                opacity: None,
            },
            MaterialInfo {
                color: [c, c, c],
                opacity: None,
                mat: mat.map(str::to_string),
            },
        );
    }
    let names: Vec<String> = export_names(&mats).into_values().collect();
    assert_eq!(
        names,
        ["Matte_grey", "Matte_grey_2", "mat2", "m_1º_coat___fine"]
    );
}

#[test]
fn test_obj_mtl_uses_real_names() {
    let tmp = TmpDir::new();
    let path = tmp.join("m.obj");
    obj::save_obj(&scene_two_materials(), &path).unwrap();
    let mtl = std::fs::read_to_string(tmp.join("m.mtl")).unwrap();
    let o = std::fs::read_to_string(&path).unwrap();
    assert!(mtl.contains("newmtl Matte_grey"));
    assert!(o.contains("usemtl Matte_grey"));
    assert!(mtl.contains("mat1"));
    assert!(!mtl.contains("mat0"));
}

#[test]
fn test_dae_material_names() {
    let tmp = TmpDir::new();
    let path = tmp.join("m.dae");
    dae::save_dae(&scene_two_materials(), &path, "Xtrude CAD model").unwrap();
    let xml = std::fs::read_to_string(&path).unwrap();
    assert!(xml.contains(r#"name="Matte_grey""#));
    assert!(xml.contains(r#"id="mat0""#));
}

#[test]
fn test_gltf_material_names() {
    let tmp = TmpDir::new();
    let path = tmp.join("m.glb");
    gltf::save_glb(&scene_two_materials(), &path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let json_len = u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&raw[20..20 + json_len]).unwrap();
    let names: Vec<&str> = doc["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Matte_grey"));
    assert!(names.iter().any(|n| n.starts_with("mat")));
}
