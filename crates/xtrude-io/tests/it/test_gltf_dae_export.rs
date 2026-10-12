// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! GLB and COLLADA exports.

use crate::common;
use common::*;
use std::path::Path;
use xtrude_io::{dae, gltf};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

const NS: &str = "http://www.collada.org/2005/11/COLLADASchema";

fn el<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name((NS, name)))
}

fn read_glb(path: &Path) -> (serde_json::Value, Vec<u8>) {
    let data = std::fs::read(path).unwrap();
    let u = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    assert_eq!(u(0), 0x4654_6C67, "bad glTF magic");
    assert_eq!(u(4), 2);
    assert_eq!(u(8) as usize, data.len());
    let json_len = u(12) as usize;
    assert_eq!(u(16), 0x4E4F_534A, "first chunk must be JSON");
    let gltf: serde_json::Value = serde_json::from_slice(&data[20..20 + json_len]).unwrap();
    let mut off = 20 + json_len;
    let mut bin = Vec::new();
    if off < data.len() {
        let bin_len = u(off) as usize;
        assert_eq!(u(off + 4), 0x004E_4942, "second chunk must be BIN");
        off += 8;
        bin = data[off..off + bin_len].to_vec();
    }
    (gltf, bin)
}

fn cube() -> Scene {
    let mut s = Scene::new();
    let mut h = History::new();
    drawn_cube(&mut s, &mut h);
    s
}

#[test]
fn test_glb_is_valid_and_has_geometry() {
    let s = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.glb");
    gltf::save_glb(&s, &path).unwrap();
    let (g, bin) = read_glb(&path);
    assert_eq!(g["asset"]["version"], "2.0");
    let prim = &g["meshes"][0]["primitives"][0];
    assert!(prim["attributes"].get("POSITION").is_some());
    assert!(prim["attributes"].get("NORMAL").is_some());
    assert!(prim.get("indices").is_some());
    let pos_acc = &g["accessors"][prim["attributes"]["POSITION"].as_u64().unwrap() as usize];
    assert!(pos_acc["type"] == "VEC3" && pos_acc["min"].as_array().unwrap().len() == 3);
    assert_eq!(
        g["buffers"][0]["byteLength"].as_u64().unwrap() as usize,
        bin.len()
    );
}

#[test]
fn test_glb_is_y_up() {
    let s = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.glb");
    gltf::save_glb(&s, &path).unwrap();
    let (g, _) = read_glb(&path);
    let ys: Vec<f64> = g["meshes"][0]["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            g["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize]["max"][1]
                .as_f64()
                .unwrap()
        })
        .collect();
    assert!(
        ys.iter().cloned().fold(f64::MIN, f64::max) > 2.9,
        "height should map to +Y in glTF"
    );
}

#[test]
fn test_dae_is_wellformed_zup_with_geometry() {
    let s = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("cube.dae");
    dae::save_dae(&s, &path, "Xtrude CAD model").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let root = doc.root_element();
    let up = el(root, "asset").and_then(|a| el(a, "up_axis")).unwrap();
    assert_eq!(up.text().unwrap().trim(), "Z_UP");
    let total: usize = root
        .descendants()
        .filter(|n| n.has_tag_name((NS, "triangles")))
        .map(|t| t.attribute("count").unwrap().parse::<usize>().unwrap())
        .sum();
    assert_eq!(total, 12);
    let node = el(root, "library_visual_scenes")
        .and_then(|l| el(l, "visual_scene"))
        .and_then(|v| el(v, "node"))
        .unwrap();
    assert!(el(node, "instance_geometry").is_some());
}

#[test]
fn test_dae_roundtrip_geometry() {
    let s = cube();
    let tmp = TmpDir::new();
    let path = tmp.join("rt.dae");
    dae::save_dae(&s, &path, "Xtrude CAD model").unwrap();
    let mut s2 = Scene::new();
    dae::load_dae(&mut s2, &path, None).unwrap();
    assert!(!s2.mesh().faces.is_empty() || !s2.doc.groups.is_empty());
}
