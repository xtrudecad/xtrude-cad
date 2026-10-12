// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The `.skp` seam (container detection, parse → apply, the `NeedsConverter`
//! fallback) and the OpenSKP adapter on hand-built models.

use crate::common;

use std::io::Write;
use std::path::{Path, PathBuf};

use common::skp::*;
use common::TmpDir;
use openskp::{Dimension as SkpDimension, Layer as SkpLayer, Page, SkpModel, Texture};
use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::math::{DMat4, MatExt};
use xtrude_core::DVec3;
use xtrude_io::skp::{
    self, adapter, AdaptOptions, AttrEntry, DimRecord, LayerRecord, PayloadFace, PayloadGroup,
    Proto, SceneRecord, SkpBackend, SkpPayload,
};
use xtrude_model::dimension::Dimension;
use xtrude_model::history::{FnCommand, History};
use xtrude_model::layers::Layer;
use xtrude_model::scene::Scene;
use xtrude_model::units;

/// The UTF-16LE "SketchUp Model" marker a real `.skp` starts with.
fn skp_header_bytes() -> Vec<u8> {
    let mut b = b"\xff\xfe".to_vec();
    b.extend(
        "SketchUp Model"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes()),
    );
    b
}

fn adapt(model: &SkpModel, name: &str) -> Option<SkpPayload> {
    adapter::adapt(model, name, None, &AdaptOptions::default()).unwrap()
}

/// The large-model path (flatten and share) on a fake model of a few
/// placements.
fn flattened() -> AdaptOptions {
    AdaptOptions {
        tree_max_placements: 0,
        ..Default::default()
    }
}

fn adapt_with(model: &SkpModel, name: &str, options: AdaptOptions) -> SkpPayload {
    adapter::adapt(model, name, None, &options)
        .unwrap()
        .unwrap()
}

fn tri() -> Vec<DVec3> {
    vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(1.0, 1.0, 0.0),
    ]
}

fn face(outer: Vec<DVec3>, attrs: Option<usize>) -> PayloadFace {
    PayloadFace {
        outer,
        holes: Vec::new(),
        attrs,
    }
}

fn group(name: &str, faces: Vec<PayloadFace>) -> PayloadGroup {
    PayloadGroup {
        name: Some(name.into()),
        faces,
        soft_edges: Some(Vec::new()),
        ..Default::default()
    }
}

fn paint(color: [f64; 3], mat: &str) -> Paint {
    Paint {
        color: Some(color),
        mat: Some(mat.into()),
        opacity: None,
    }
}

fn attrs_of(payload: &SkpPayload, f: &PayloadFace) -> Option<FaceAttrs> {
    payload.face_attrs(f.attrs)
}

#[test]
fn test_detect_format_recognises_a_skp_file() {
    let tmp = TmpDir::new();
    let p = tmp.join("m.skp");
    let mut b = skp_header_bytes();
    b.extend([0u8; 40]);
    std::fs::write(&p, b).unwrap();
    assert_eq!(skp::detect_format(&p), "skp");
}

#[test]
fn test_detect_format_unknown_for_non_skp() {
    let tmp = TmpDir::new();
    let p = tmp.join("x.skp");
    std::fs::write(&p, b"not a .skp file at all").unwrap();
    assert_eq!(skp::detect_format(&p), "unknown");
    assert_eq!(skp::detect_format(&tmp.join("missing.skp")), "unknown");
}

#[test]
fn test_unrecognised_file_needs_converter() {
    let tmp = TmpDir::new();
    let p = tmp.join("x.skp");
    std::fs::write(&p, b"garbage").unwrap();
    assert!(!skp::can_handle(&p));
    let err = skp::parse_skp(&p, None).unwrap_err();
    assert_eq!(err.format, "unknown");
}

#[test]
fn test_backends_status_lists_openskp() {
    assert!(skp::backends_status().iter().any(|(n, _)| n == "openskp"));
}

/// A backend answering with a fixed payload (the tests' fakes).
struct FakeBackend {
    name: &'static str,
    payload: Option<SkpPayload>,
    report: bool,
}

impl SkpBackend for FakeBackend {
    fn name(&self) -> &str {
        self.name
    }

    fn available(&self) -> bool {
        true
    }

    fn supports(&self, fmt: &str) -> bool {
        fmt == "skp"
    }

    fn parse(
        &self,
        _path: &Path,
        progress: Option<xtrude_io::dae::Progress<'_>>,
    ) -> Result<Option<SkpPayload>, String> {
        if self.report {
            if let Some(p) = progress {
                p(1.0, "done");
            }
        }
        Ok(self.payload.clone())
    }
}

#[test]
fn test_cascade_parses_with_available_backend_and_applies() {
    let mut payload = SkpPayload::new("fake");
    let a = payload.push_attrs(AttrEntry {
        paint: Paint {
            color: Some([0.2, 0.4, 0.6]),
            ..Default::default()
        },
        ..Default::default()
    });
    payload.groups.push(PayloadGroup {
        name: Some("g".into()),
        faces: vec![face(tri(), Some(a))],
        ..Default::default()
    });
    let backend = FakeBackend {
        name: "fake",
        payload: Some(payload),
        report: true,
    };
    let tmp = TmpDir::new();
    let p = tmp.join("y.skp");
    std::fs::write(&p, skp_header_bytes()).unwrap();
    assert!(skp::can_handle_with(&p, &[&backend]));

    let mut scene = Scene::new();
    let mut calls: Vec<String> = Vec::new();
    let mut cb = |_f: f64, t: &str| calls.push(t.to_string());
    let used = skp::load_skp_with(&mut scene, &p, Some(&mut cb), &[&backend]).unwrap();
    assert_eq!(used, "fake");
    assert_eq!(scene.doc.groups.len(), 1);
    assert_eq!(scene.doc.groups[0].name, "g");
    assert_eq!(calls, ["done"]);
}

#[test]
fn test_empty_parse_falls_back_to_converter() {
    let backend = FakeBackend {
        name: "empty",
        payload: None,
        report: false,
    };
    let tmp = TmpDir::new();
    let p = tmp.join("z.skp");
    std::fs::write(&p, skp_header_bytes()).unwrap();
    assert!(skp::parse_skp_with(&p, None, &[&backend]).is_err());
}

// ---- OpenSKP adapter (fake model) ----

fn xs(f: &PayloadFace) -> Vec<f64> {
    let mut v: Vec<f64> = f.outer.iter().map(|p| (p.x * 1e4).round() / 1e4).collect();
    v.sort_by(f64::total_cmp);
    v
}

#[test]
fn test_openskp_adapter_resolves_a_face_ring_in_metres() {
    let root = defn(
        0,
        "ROOT_MODEL",
        &[
            (1, [0.0, 0.0, 0.0]),
            (2, [100.0, 0.0, 0.0]),
            (3, [100.0, 100.0, 0.0]),
        ],
        &[(10, (1, 2)), (11, (2, 3)), (12, (3, 1))],
        &[(20, vec![vec![(10, 1), (11, 1), (12, 1)]])],
        vec![],
    );
    let payload = adapt(&model(root, vec![]), "tri").unwrap();
    assert_eq!(payload.backend, "openskp");
    let faces = &payload.groups[0].faces;
    assert_eq!(faces.len(), 1);
    assert_eq!(xs(&faces[0]), [0.0, 2.54, 2.54]); // 100 in = 2.54 m
    assert!(faces[0].holes.is_empty());
}

#[test]
fn test_face_ring_reads_the_same_square_under_either_coedge_contract() {
    // Edge 12 is stored 4→3 while the loop walks 3→4: the reversed one.
    let d = defn(
        0,
        "ROOT_MODEL",
        &[
            (1, [0.0, 0.0, 0.0]),
            (2, [100.0, 0.0, 0.0]),
            (3, [100.0, 100.0, 0.0]),
            (4, [0.0, 100.0, 0.0]),
        ],
        &[(10, (1, 2)), (11, (2, 3)), (12, (4, 3)), (13, (4, 1))],
        &[],
        vec![],
    );
    for (senses, era) in [
        ([0, 0, 1, 0], "raw storage bit: 0 forward, 1 reversed"),
        (
            [1, 1, -1, 1],
            "documented contract: +1 forward, -1 reversed",
        ),
    ] {
        let lp: Vec<(i64, i64)> = [10, 11, 12, 13].into_iter().zip(senses).collect();
        let ring = adapter::ring_raw(&d, &lp).unwrap();
        let corners: Vec<(f64, f64)> = ring.iter().map(|p| (p.x, p.y)).collect();
        let square = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let rotations: Vec<Vec<(f64, f64)>> = (0..4)
            .map(|k| square[k..].iter().chain(&square[..k]).copied().collect())
            .collect();
        assert!(rotations.contains(&corners), "{era}");
    }
}

/// The single placed definition's entry (a prototype when
/// there is one).
fn placed(payload: &SkpPayload) -> (&[PayloadFace], Option<&Proto>) {
    if !payload.protos.is_empty() {
        assert_eq!(payload.protos.len(), 1);
        return (&payload.protos[0].faces, Some(&payload.protos[0]));
    }
    (&payload.groups[0].faces, None)
}

#[test]
fn test_openskp_adapter_places_instances_with_transform() {
    let child = defn(
        5,
        "Child",
        &[
            (1, [0.0, 0.0, 0.0]),
            (2, [10.0, 0.0, 0.0]),
            (3, [10.0, 10.0, 0.0]),
        ],
        &[(10, (1, 2)), (11, (2, 3)), (12, (3, 1))],
        &[(20, vec![vec![(10, 1), (11, 1), (12, 1)]])],
        vec![],
    );
    let root = empty_defn(0, "ROOT_MODEL", vec![ins(5, 100.0)]);
    let payload = adapt_with(&model(root, vec![child]), "inst", flattened());
    // The definition keeps its own coordinates; the placement rides on the
    // instance matrix.
    let proto = &payload.protos[0];
    let x = xs(&proto.faces[0]);
    assert!(x[0].abs() <= 1e-4); // local: 0..10 in
    assert!((x[2] - 0.254).abs() <= 1e-4);
    assert_eq!(proto.instances.len(), 1);
    let at = proto.instances[0].project_point(DVec3::ZERO);
    assert!((at.x - 2.54).abs() <= 1e-4); // +100 in on X
}

#[test]
fn test_openskp_adapter_returns_none_without_geometry() {
    let root = empty_defn(0, "ROOT_MODEL", vec![]);
    assert!(adapt(&model(root, vec![]), "empty").is_none());
}

/// A 2021+ style `.skp`: the marker, then the ZIP that carries the
/// original program's render of the model (`meta/model_thumbnail.png`),
/// white with `black` pixels.
fn vff_skp(tmp: &TmpDir, black: &[(u32, u32)]) -> PathBuf {
    let mut img = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 255, 255, 255]));
    for &(x, y) in black {
        img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut zbuf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut zbuf);
        zw.start_file(
            "meta/model_thumbnail.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zw.write_all(&png).unwrap();
        zw.finish().unwrap();
    }
    let p = tmp.join("m.skp");
    let mut data = skp_header_bytes();
    data.extend(b"VFF");
    data.extend(zbuf.into_inner());
    std::fs::write(&p, data).unwrap();
    p
}

fn empty_vff_model() -> SkpModel {
    SkpModel {
        root: empty_defn(0, "ROOT_MODEL", vec![]),
        version: "{22.0.354}".into(),
        ..Default::default()
    }
}

#[test]
fn test_an_empty_skp_file_opens_empty_instead_of_asking_for_the_converter() {
    let tmp = TmpDir::new();
    let skp_path = vff_skp(&tmp, &[]);
    let payload = adapter::adapt(
        &empty_vff_model(),
        "template",
        Some(&skp_path),
        &AdaptOptions::default(),
    )
    .unwrap()
    .unwrap();
    assert!(payload.empty);
    assert!(payload.groups.is_empty() && payload.protos.is_empty());
}

#[test]
fn test_a_file_whose_thumbnail_shows_geometry_still_falls_back() {
    let tmp = TmpDir::new();
    let skp_path = vff_skp(&tmp, &[(3, 3)]);
    let r = adapter::adapt(
        &empty_vff_model(),
        "m",
        Some(&skp_path),
        &AdaptOptions::default(),
    );
    assert!(r.unwrap().is_none());
}

#[test]
fn test_an_empty_legacy_file_still_falls_back() {
    let tmp = TmpDir::new();
    let skp_path = vff_skp(&tmp, &[]);
    let mut m = empty_vff_model();
    m.version = "{17.2.2555}".into();
    let r = adapter::adapt(&m, "m", Some(&skp_path), &AdaptOptions::default());
    assert!(r.unwrap().is_none());
}

#[test]
fn test_parse_skp_accepts_an_empty_payload() {
    let tmp = TmpDir::new();
    let p = tmp.join("m.skp");
    let mut b = skp_header_bytes();
    b.extend([0u8; 40]);
    std::fs::write(&p, b).unwrap();
    let backend = FakeBackend {
        name: "openskp",
        payload: Some(SkpPayload {
            empty: true,
            ..SkpPayload::new("openskp")
        }),
        report: false,
    };
    let payload = skp::parse_skp_with(&p, None, &[&backend]).unwrap();
    let mut scene = Scene::new();
    assert_eq!(skp::apply_payload(&mut scene, &payload).unwrap(), "openskp");
    assert!(scene.doc.groups.is_empty());
}

fn tri_root_with_material(mid: Option<i64>) -> openskp::Definition {
    let mut root = defn(
        0,
        "ROOT_MODEL",
        &[
            (1, [0.0, 0.0, 0.0]),
            (2, [10.0, 0.0, 0.0]),
            (3, [10.0, 10.0, 0.0]),
        ],
        &[(10, (1, 2)), (11, (2, 3)), (12, (3, 1))],
        &[(20, vec![vec![(10, 1), (11, 1), (12, 1)]])],
        vec![],
    );
    root.faces[&20].material_id = mid;
    root
}

#[test]
fn test_openskp_adapter_resolves_face_colours_via_materials_by_id() {
    let with_join = with_materials(
        model(tri_root_with_material(Some(29491)), vec![]),
        vec![(29491, material("Wood", [255, 0, 51], 1.0))],
    );
    let payload = adapt(&with_join, "m").unwrap();
    let a = attrs_of(&payload, &payload.groups[0].faces[0]).unwrap();
    assert_eq!(
        a,
        FaceAttrs {
            paint: paint([1.0, 0.0, 0.2], "Wood"),
            ..Default::default()
        }
    );
    // A model without the join imports uncoloured.
    let without_join = model(tri_root_with_material(Some(29491)), vec![]);
    let payload = adapt(&without_join, "m").unwrap();
    assert_eq!(attrs_of(&payload, &payload.groups[0].faces[0]), None);
}

#[test]
fn test_openskp_adapter_imports_a_textured_material_as_its_colour() {
    let mut glass = material("Glass", [8, 201, 241], 0.5);
    glass.texture = Some(Texture {
        filename: "glass.jpg".into(),
        width: 24.0,
        height: 12.0,
        data: Some(b"\xff\xd8fakejpeg".to_vec()),
    });
    let m = with_materials(
        model(tri_root_with_material(Some(5)), vec![]),
        vec![(5, glass)],
    );
    let tmp = TmpDir::new();
    let src = tmp.join("docs");
    std::fs::create_dir_all(&src).unwrap();
    let skp_path = src.join("part.skp");
    std::fs::write(&skp_path, b"").unwrap();
    let payload = adapter::adapt(&m, "part", Some(&skp_path), &AdaptOptions::default())
        .unwrap()
        .unwrap();
    let a = attrs_of(&payload, &payload.groups[0].faces[0]).unwrap();
    let c = a.paint.color.unwrap();
    for (got, want) in c.iter().zip([8.0 / 255.0, 201.0 / 255.0, 241.0 / 255.0]) {
        assert!((got - want).abs() < 1e-12);
    }
    assert_eq!(a.paint.opacity, Some(0.5));
    // Nothing is written anywhere: the folder is left as it was.
    let listing: Vec<PathBuf> = std::fs::read_dir(&src)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(listing, [skp_path]);
}

#[test]
fn test_openskp_adapter_groups_per_top_level_instance() {
    let child = tri_def(5, "Clip", vec![]);
    let root = tri_def(0, "ROOT_MODEL", vec![ins(5, 100.0)]);
    let payload = adapt_with(&model(root, vec![child]), "project", flattened());
    // The root's loose faces stay a group; the placed definition is a proto.
    let names: Vec<&str> = payload
        .groups
        .iter()
        .filter_map(|g| g.name.as_deref())
        .collect();
    assert_eq!(names, ["project"]);
    let protos: Vec<&str> = payload
        .protos
        .iter()
        .filter_map(|p| p.name.as_deref())
        .collect();
    assert_eq!(protos, ["Clip"]);
    assert_eq!(payload.protos[0].instances.len(), 1);
}

fn shared_options() -> AdaptOptions {
    AdaptOptions {
        tree_max_placements: 0,
        inst_min_polys: 1,
        inst_min_saved: 1,
    }
}

#[test]
fn test_openskp_adapter_shares_repeated_components() {
    let child = tri_def(5, "Knob", vec![]);
    let root = empty_defn(0, "ROOT_MODEL", vec![ins(5, 0.0), ins(5, 100.0)]);
    let payload = adapt_with(&model(root, vec![child]), "project", shared_options());
    assert!(payload.groups.is_empty());
    assert_eq!(payload.protos.len(), 1);
    let proto = &payload.protos[0];
    assert_eq!(proto.name.as_deref(), Some("Knob"));
    assert_eq!(proto.instances.len(), 2);
    // Prototype geometry is LOCAL (the translation lives in the matrices).
    let max = xs(&proto.faces[0]).into_iter().fold(f64::MIN, f64::max);
    assert!((max - 10.0 * 0.0254).abs() < 1e-9);

    // apply: two instance Groups SHARING one prototype mesh.
    let mut scene = Scene::new();
    skp::apply_payload(&mut scene, &payload).unwrap();
    let inst: Vec<_> = scene
        .doc
        .groups
        .iter()
        .filter(|g| g.xform.is_some())
        .collect();
    assert_eq!(inst.len(), 2);
    assert_eq!(inst[0].mesh, inst[1].mesh);
}

#[test]
fn test_openskp_adapter_inherits_instance_material() {
    let child = tri_def(5, "Spacer", vec![]); // faces carry no material
    let mut i = ins(5, 0.0);
    i.material_id = Some(77);
    let root = empty_defn(0, "ROOT_MODEL", vec![i]);
    let m = with_materials(
        model(root, vec![child]),
        vec![(77, material("Wood", [255, 0, 0], 1.0))],
    );
    let payload = adapt(&m, "project").unwrap();
    let (faces, _) = placed(&payload);
    // Painting the component paints BOTH sides of its default faces.
    assert_eq!(
        attrs_of(&payload, &faces[0]).unwrap(),
        FaceAttrs {
            paint: paint([1.0, 0.0, 0.0], "Wood"),
            back: Some(Back::Mirror),
            ..Default::default()
        }
    );
}

#[test]
fn test_openskp_adapter_face_material_beats_inherited() {
    let mut child = tri_def(5, "Spacer", vec![]);
    child.faces[&20].material_id = Some(88); // the face's own material wins
    let mut i = ins(5, 0.0);
    i.material_id = Some(77);
    let root = empty_defn(0, "ROOT_MODEL", vec![i]);
    let m = with_materials(
        model(root, vec![child]),
        vec![
            (77, material("W", [255, 0, 0], 1.0)),
            (88, material("B", [0, 0, 255], 1.0)),
        ],
    );
    let payload = adapt(&m, "project").unwrap();
    let (faces, _) = placed(&payload);
    // The unpainted back shows the instance's inherited paint.
    assert_eq!(
        attrs_of(&payload, &faces[0]).unwrap(),
        FaceAttrs {
            paint: paint([0.0, 0.0, 1.0], "B"),
            back: Some(Back::Own(paint([1.0, 0.0, 0.0], "W"))),
            ..Default::default()
        }
    );
}

#[test]
fn test_openskp_adapter_image_entities_become_their_own_groups() {
    let image_def = |id: i64, name: &str, mid: i64| {
        let mut d = tri_def(id, name, vec![]);
        d.faces[&20].material_id = Some(mid);
        d.is_image = true;
        d
    };
    let decal = image_def(5, "image#1", 51);
    let label = image_def(6, "image#2", 52);
    let root = empty_defn(0, "ROOT_MODEL", vec![ins(5, 0.0), ins(6, 100.0)]);
    let tex = |file: &str| Texture {
        filename: file.into(),
        width: 48.0,
        height: 48.0,
        data: Some(b"\x89PNGimage".to_vec()),
    };
    let mut t = material("T", [1, 1, 1], 1.0);
    t.texture = Some(tex("decal.png"));
    let mut mm = material("M", [1, 1, 1], 1.0);
    mm.texture = Some(tex("label.png"));
    let m = with_materials(model(root, vec![decal, label]), vec![(51, t), (52, mm)]);
    let tmp = TmpDir::new();
    let skp_path = tmp.join("m.skp");
    std::fs::write(&skp_path, b"").unwrap();
    let payload = adapter::adapt(&m, "m", Some(&skp_path), &AdaptOptions::default())
        .unwrap()
        .unwrap();
    let by_name = |n: &str| {
        payload
            .groups
            .iter()
            .find(|g| g.name.as_deref() == Some(n))
            .unwrap()
    };
    assert!(!by_name("image#1").faces.is_empty());
    assert!(!by_name("image#2").faces.is_empty());

    let mut scene = Scene::new();
    skp::apply_payload(&mut scene, &payload).unwrap();
    let names: Vec<&str> = scene.doc.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"image#1") && names.contains(&"image#2"));
}

#[test]
fn test_openskp_adapter_own_back_material_beats_instance_paint() {
    // The bullring: group painted blue, faces carrying grey on their backs.
    let mut child = tri_def(5, "Ring", vec![]);
    child.faces[&20].back_material_id = Some(30);
    let mut i = ins(5, 0.0);
    i.material_id = Some(40);
    let root = empty_defn(0, "ROOT_MODEL", vec![i]);
    let m = with_materials(
        model(root, vec![child]),
        vec![
            (30, material("Grey", [128, 128, 128], 1.0)),
            (40, material("Blue", [65, 105, 225], 1.0)),
        ],
    );
    let payload = adapt(&m, "m").unwrap();
    let (faces, _) = placed(&payload);
    let g = 128.0 / 255.0;
    assert_eq!(
        attrs_of(&payload, &faces[0]).unwrap(),
        FaceAttrs {
            paint: paint([g, g, g], "Grey"),
            ..Default::default()
        }
    );
}

#[test]
fn test_openskp_adapter_back_painted_face_flips_and_paints() {
    let mut root = tri_def(0, "ROOT_MODEL", vec![]);
    root.faces[&20].back_material_id = Some(7);
    let m = with_materials(
        model(root, vec![]),
        vec![(7, material("Grass", [0, 200, 0], 1.0))],
    );
    let payload = adapt(&m, "m").unwrap();
    let f = &payload.groups[0].faces[0];
    assert_eq!(
        attrs_of(&payload, f).unwrap(),
        FaceAttrs {
            paint: paint([0.0, 200.0 / 255.0, 0.0], "Grass"),
            ..Default::default()
        }
    );
    // Reversed: the first output corner is the original last one.
    let p = f.outer[0];
    assert_eq!(
        ((p.x * 1e3).round() / 1e3, (p.y * 1e3).round() / 1e3),
        (0.254, 0.254)
    );
}

#[test]
fn test_openskp_adapter_face_camera_component_imports_as_plain_geometry() {
    let mut susan = tri_def(5, "Susan", vec![]);
    susan.always_faces_camera = true;
    susan.faces[&20].material_id = Some(9);
    let root = empty_defn(0, "ROOT_MODEL", vec![ins(5, 0.0)]);
    let m = with_materials(
        model(root, vec![susan]),
        vec![(9, material("Shirt", [0, 128, 0], 1.0))],
    );
    let payload = adapt(&m, "m").unwrap();
    let names: Vec<&str> = payload
        .protos
        .iter()
        .filter_map(|p| p.name.as_deref())
        .collect();
    assert_eq!(names, ["Susan"]);
    let mut scene = Scene::new();
    skp::apply_payload(&mut scene, &payload).unwrap();
    assert!(scene.doc.groups.iter().any(|g| g.name == "Susan"));
}

#[test]
fn test_openskp_adapter_splits_prototypes_by_inherited_material() {
    let child = tri_def(5, "Peg", vec![]);
    let painted = |mid: i64, x: f64| {
        let mut i = ins(5, x);
        i.material_id = Some(mid);
        i
    };
    let root = empty_defn(
        0,
        "ROOT_MODEL",
        vec![painted(1, 0.0), painted(1, 50.0), painted(2, 100.0)],
    );
    let m = with_materials(
        model(root, vec![child]),
        vec![
            (1, material("R", [255, 0, 0], 1.0)),
            (2, material("G", [0, 255, 0], 1.0)),
        ],
    );
    let payload = adapt_with(&m, "project", shared_options());
    assert_eq!(payload.protos.len(), 2); // one per inherited material
    let mut counts: Vec<usize> = payload.protos.iter().map(|p| p.instances.len()).collect();
    counts.sort();
    assert_eq!(counts, [1, 2]);
    let mut colors: Vec<[f64; 3]> = payload
        .protos
        .iter()
        .map(|p| {
            attrs_of(&payload, &p.faces[0])
                .unwrap()
                .paint
                .color
                .unwrap()
        })
        .collect();
    colors.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(colors, [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
}

#[test]
fn test_openskp_adapter_translucent_material_carries_opacity() {
    let mut glass = material("Glass", [100, 149, 237], 0.27);
    glass.texture = Some(Texture {
        filename: "glass.png".into(),
        width: 48.0,
        height: 48.0,
        data: Some(b"\x89PNGglass".to_vec()),
    });
    let m = with_materials(
        model(tri_root_with_material(Some(9)), vec![]),
        vec![(9, glass)],
    );
    let tmp = TmpDir::new();
    let skp_path = tmp.join("m.skp");
    std::fs::write(&skp_path, b"").unwrap();
    let payload = adapter::adapt(&m, "m", Some(&skp_path), &AdaptOptions::default())
        .unwrap()
        .unwrap();
    let a = attrs_of(&payload, &payload.groups[0].faces[0]).unwrap();
    assert_eq!(a.paint.opacity, Some(0.27));
}

// ---- Layers (.skp tags) ----

#[test]
fn test_openskp_adapter_carries_file_layers_with_visibility() {
    // The model's DEFAULT layer never travels.
    let layer = |name: &str, visible: bool| SkpLayer {
        hidden: !visible,
        ..SkpLayer::new(name)
    };
    let mut m = model(tri_def(0, "ROOT_MODEL", vec![]), vec![]);
    m.layers = vec![
        layer("Layer0", true),
        layer("Shells", true),
        layer("Curves", false),
    ];
    let payload = adapt(&m, "m").unwrap();
    assert_eq!(
        payload.layers,
        [
            LayerRecord {
                name: "Shells".into(),
                visible: true
            },
            LayerRecord {
                name: "Curves".into(),
                visible: false
            },
        ]
    );
}

#[test]
fn test_openskp_adapter_top_level_instance_layer_on_group() {
    let child = tri_def(5, "Knob", vec![]);
    let mut i = ins(5, 0.0);
    i.layer = "Fasteners".into();
    let root = empty_defn(0, "ROOT_MODEL", vec![i]);
    let payload = adapt_with(&model(root, vec![child]), "m", flattened());
    // The tag rides per PLACEMENT.
    let (_, proto) = placed(&payload);
    assert_eq!(
        proto.unwrap().instance_layers,
        Some(vec![Some("Fasteners".to_string())])
    );
}

/// A tagged leaf inside an untagged container, placed at the top.
fn housing() -> SkpModel {
    let leaf = tri_def(7, "Screw", vec![]);
    let mut tagged = ins(7, 0.0);
    tagged.layer = "screws".into();
    let container = empty_defn(5, "Housing", vec![tagged]);
    let root = empty_defn(0, "ROOT_MODEL", vec![ins(5, 0.0)]);
    model(root, vec![container, leaf])
}

#[test]
fn test_openskp_adapter_extracts_nested_tagged_instance_as_layer_group() {
    let payload = adapt_with(&housing(), "m", flattened());
    let g = payload
        .groups
        .iter()
        .find(|g| g.layer.as_deref() == Some("screws"))
        .unwrap();
    assert_eq!(g.faces.len(), 1);
}

/// OpenSKP gives a face's own layer as a raw id; the adapter resolves it
/// through the model's layer table.
#[test]
fn test_openskp_adapter_face_layer_lands_in_attrs() {
    let mut root = tri_def(0, "ROOT_MODEL", vec![]);
    root.faces[&20].layer = Some(42);
    let mut m = model(root, vec![]);
    m.layer_names.insert(42, "Labels".into());
    let payload = adapt(&m, "m").unwrap();
    let a = attrs_of(&payload, &payload.groups[0].faces[0]).unwrap();
    assert_eq!(a.layer.as_deref(), Some("Labels"));
}

#[test]
fn test_apply_payload_registers_layers_and_assigns_groups() {
    let mut scene = Scene::new();
    let mut shells = Layer::new("Shells");
    shells.visible = false; // the user's own state
    scene.doc.layers.push(shells);
    let mut payload = SkpPayload::new("openskp");
    payload.layers = vec![
        LayerRecord {
            name: "Shells".into(),
            visible: true,
        },
        LayerRecord {
            name: "Curves".into(),
            visible: false,
        },
    ];
    payload.groups.push(PayloadGroup {
        layer: Some("Curves".into()),
        ..group("g", vec![face(tri(), None)])
    });
    payload.protos.push(Proto {
        name: Some("p".into()),
        faces: vec![face(tri(), None)],
        soft_edges: Some(Vec::new()),
        instances: vec![DMat4::IDENTITY, DMat4::IDENTITY],
        instance_layers: Some(vec![Some("Shells".into()), None]),
        ..Default::default()
    });
    skp::apply_payload(&mut scene, &payload).unwrap();
    let layer = |n: &str| scene.doc.layers.iter().find(|l| l.name == n).unwrap();
    assert!(!layer("Curves").visible); // the file's state honoured
    assert!(!layer("Shells").visible); // the user's state untouched
    assert_eq!(scene.doc.groups[0].layer.as_deref(), Some("Curves"));
    let proto_layers: Vec<Option<&str>> = scene.doc.groups[1..]
        .iter()
        .map(|g| g.layer.as_deref())
        .collect();
    assert_eq!(proto_layers, [Some("Shells"), None]);
}

/// Apply `payload` as one undo step.
fn import(scene: &mut Scene, history: &mut History, payload: &SkpPayload) {
    let mut cmd = FnCommand::new("SnapshotImport", |s: &mut Scene| {
        skp::apply_payload(s, payload)
            .map(|_| ())
            .map_err(xtrude_model::history::CmdError::msg)
    });
    assert!(history.execute(scene, &mut cmd));
}

fn one_group_payload() -> SkpPayload {
    let mut payload = SkpPayload::new("openskp");
    payload.groups.push(group("g", vec![face(tri(), None)]));
    payload
}

#[test]
fn test_snapshot_import_undo_reverts_added_layers() {
    let mut scene = Scene::new();
    let mut payload = one_group_payload();
    payload.layers = vec![LayerRecord {
        name: "Curves".into(),
        visible: false,
    }];
    let mut history = History::new();
    import(&mut scene, &mut history, &payload);
    let has = |s: &Scene| s.doc.layers.iter().any(|l| l.name == "Curves");
    assert!(has(&scene));
    history.undo(&mut scene);
    assert!(!has(&scene));
    history.redo(&mut scene);
    assert!(has(&scene));
}

// ---- Scenes (.skp pages) ----

#[test]
fn test_openskp_adapter_carries_scenes_in_metres() {
    let mut m = model(tri_def(0, "ROOT_MODEL", vec![]), vec![]);
    m.pages = vec![Page {
        name: "View1".into(),
        eye: Some([100.0, 0.0, 0.0]),
        target: Some([0.0, 0.0, 0.0]),
        up: Some([0.0, 0.0, 1.0]),
        fov: 35.0,
        parallel: true,
        ortho_height: 100.0,
        hidden_layers: vec!["Curves".into()],
    }];
    let payload = adapt(&m, "m").unwrap();
    let sc = &payload.scenes[0];
    assert_eq!(sc.name, "View1");
    assert!((sc.eye[0] - 2.54).abs() < 1e-12 && sc.eye[1] == 0.0 && sc.eye[2] == 0.0);
    assert!(sc.parallel);
    assert!((sc.ortho_height.unwrap() - 2.54).abs() < 1e-12);
    assert_eq!(sc.hidden_layers, ["Curves"]);
}

fn view_record(fov: Option<f64>, hidden: Vec<String>) -> SceneRecord {
    SceneRecord {
        name: "View1".into(),
        eye: [10.0, 0.0, 0.0],
        target: [0.0, 0.0, 0.0],
        up: Some([0.0, 0.0, 1.0]),
        fov,
        parallel: false,
        ortho_height: None,
        hidden_layers: hidden,
    }
}

#[test]
fn test_apply_payload_creates_saved_views_once() {
    let mut scene = Scene::new();
    let mut payload = one_group_payload();
    payload.scenes = vec![view_record(Some(35.0), vec!["Curves".into()])];
    skp::apply_payload(&mut scene, &payload).unwrap();
    let names: Vec<&str> = scene.saved_views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["View1"]);
    let v = &scene.saved_views[0];
    assert!((v.pose.distance - 10.0).abs() < 1e-9);
    assert_eq!(v.pose.fov_deg, 35.0);
    assert_eq!(v.hidden_layers, ["Curves"]);
    // Re-import: same name → not duplicated.
    skp::apply_payload(&mut scene, &payload).unwrap();
    assert_eq!(scene.saved_views.len(), 1);
}

#[test]
fn test_snapshot_import_undo_reverts_added_views() {
    let mut scene = Scene::new();
    let mut payload = one_group_payload();
    payload.scenes = vec![view_record(None, Vec::new())];
    let mut history = History::new();
    import(&mut scene, &mut history, &payload);
    let names =
        |s: &Scene| -> Vec<String> { s.saved_views.iter().map(|v| v.name.clone()).collect() };
    assert_eq!(names(&scene), ["View1"]);
    history.undo(&mut scene);
    assert!(scene.saved_views.is_empty());
    history.redo(&mut scene);
    assert_eq!(names(&scene), ["View1"]);
}

// ---- Dimensions (.skp linear dimensions) ----

#[test]
fn test_openskp_adapter_carries_dimensions_in_metres() {
    let mut m = model(tri_def(0, "ROOT_MODEL", vec![]), vec![]);
    m.dimensions = vec![SkpDimension {
        a: Some([0.0, 0.0, 0.0]),
        b: Some([0.0, 0.0, 100.0]),
        offset: 20.0,
        normal: Some([-1.0, 0.0, 0.0]),
        plane_x: Some([0.0, 1.0, 0.0]),
        ..Default::default()
    }];
    let payload = adapt(&m, "m").unwrap();
    let dm = &payload.dimensions[0];
    assert_eq!(dm.a, [0.0, 0.0, 0.0]);
    assert!((dm.b[2] - 2.54).abs() < 1e-12); // 100 in = 2.54 m
    assert!((dm.offset - 0.508).abs() < 1e-12); // 20 in
    assert_eq!(dm.normal, Some([-1.0, 0.0, 0.0]));
}

/// Readouts in metres while the guard lives.
struct MetreDocument;

impl MetreDocument {
    fn new() -> MetreDocument {
        units::set_override(Some(units::legacy_document_units()));
        MetreDocument
    }
}

impl Drop for MetreDocument {
    fn drop(&mut self) {
        units::set_override(None);
    }
}

fn dim_payload(a: [f64; 3], b: [f64; 3], offset: f64, normal: [f64; 3]) -> SkpPayload {
    let mut payload = one_group_payload();
    payload.dimensions = vec![DimRecord {
        a,
        b,
        offset,
        normal: Some(normal),
        text: String::new(),
    }];
    payload
}

#[test]
fn test_apply_payload_creates_dimensions_with_perpendicular_offset() {
    let _metres = MetreDocument::new();
    let mut scene = Scene::new();
    // A 2 m vertical segment, offset 0.5 m, plane normal along -X: the
    // dimension line sits along the in-plane perpendicular (±Y).
    let payload = dim_payload([0.0, 0.0, 0.0], [0.0, 0.0, 2.0], 0.5, [-1.0, 0.0, 0.0]);
    skp::apply_payload(&mut scene, &payload).unwrap();
    assert_eq!(scene.doc.dimensions.len(), 1);
    let d: &Dimension = &scene.doc.dimensions[0];
    let view = scene.doc_view();
    assert!((d.value(&view) - 2.0).abs() < 1e-9);
    assert_eq!(d.label(&view), "2.00 m");
    assert!(d.offset.z.abs() < 1e-12);
    assert!((d.offset.length() - 0.5).abs() < 1e-12);
}

#[test]
fn test_apply_payload_skips_degenerate_dimension() {
    let mut scene = Scene::new();
    let payload = dim_payload([1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 0.5, [0.0, 0.0, 1.0]);
    skp::apply_payload(&mut scene, &payload).unwrap();
    assert!(scene.doc.dimensions.is_empty()); // zero length: skipped
}

#[test]
fn test_snapshot_import_undo_reverts_added_dimensions() {
    let mut scene = Scene::new();
    let payload = dim_payload([0.0, 0.0, 0.0], [3.0, 0.0, 0.0], 0.5, [0.0, 0.0, 1.0]);
    let mut history = History::new();
    import(&mut scene, &mut history, &payload);
    assert_eq!(scene.doc.dimensions.len(), 1);
    history.undo(&mut scene);
    assert!(scene.doc.dimensions.is_empty());
    history.redo(&mut scene);
    assert_eq!(scene.doc.dimensions.len(), 1);
}

#[test]
fn test_openskp_adapter_keeps_a_component_placed_once_a_component() {
    // A definition placed ONCE is still a definition.
    let child = tri_def(5, "Grille", vec![]);
    let root = empty_defn(0, "ROOT_MODEL", vec![ins(5, 100.0)]);
    let payload = adapt(&model(root, vec![child]), "project").unwrap();
    let names: Vec<&str> = payload
        .protos
        .iter()
        .filter_map(|p| p.name.as_deref())
        .collect();
    assert_eq!(names, ["Grille"]);
    assert!(payload.groups.is_empty()); // nothing flattened
}

#[test]
fn test_openskp_adapter_flattens_a_tagged_subtree_placed_once() {
    let payload = adapt_with(&housing(), "project", flattened());
    assert!(!payload
        .protos
        .iter()
        .any(|p| p.name.as_deref() == Some("Housing")));
    assert!(payload
        .groups
        .iter()
        .any(|g| g.layer.as_deref() == Some("screws")));
}
