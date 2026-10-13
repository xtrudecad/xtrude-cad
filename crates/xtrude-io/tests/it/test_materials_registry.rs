// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The materials registry through the document and .skp.

use crate::common;
use common::*;
use xtrude_model::materials::Material;
use xtrude_model::scene::Scene;

#[test]
fn test_document_roundtrip_plain() {
    let mut s = Scene::new();
    let f = quad(s.mesh_mut(), 0., 2., 0., 2., 0.);
    let mat = Material::new("Brick red", Some([0.7, 0.3, 0.2]), None);
    let paint = mat.face_paint();
    s.mesh_mut().face_mut(f).attrs.paint = paint;
    s.doc.materials.insert("Brick red".into(), mat);
    let loaded = roundtrip(&s);
    assert_eq!(
        loaded.doc.materials.keys().collect::<Vec<_>>(),
        ["Brick red"]
    );
    assert_eq!(
        loaded.doc.materials["Brick red"].color,
        Some([0.7, 0.3, 0.2])
    );
    let m = loaded.mesh();
    let a = &m.face(m.face_ids()[0]).attrs;
    assert_eq!(a.paint.mat.as_deref(), Some("Brick red"));
    assert_eq!(a.paint.color, Some([0.7, 0.3, 0.2]));
}

// ---- .skp import ----

mod skp_import {
    use super::common::skp::*;
    use openskp::SkpModel;
    use xtrude_io::skp::{self, adapter, AdaptOptions};
    use xtrude_model::materials::Material;
    use xtrude_model::scene::Scene;

    /// A triangle painted with the first of `materials`.
    fn fake_model(materials: Vec<(i64, openskp::Material)>) -> SkpModel {
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
        root.faces[&20].material_id = materials.first().map(|(id, _)| *id);
        with_materials(model(root, vec![]), materials)
    }

    fn payload(m: &SkpModel, name: &str) -> skp::SkpPayload {
        adapter::adapt(m, name, None, &AdaptOptions::default())
            .unwrap()
            .unwrap()
    }

    fn face_mat(scene: &Scene) -> Option<String> {
        let mesh = scene.doc.mesh(scene.doc.groups[0].mesh);
        let f = mesh.face_ids()[0];
        mesh.face(f).attrs.paint.mat.clone()
    }

    #[test]
    fn test_skp_import_registers_named_material() {
        let m = fake_model(vec![(7, material("Matte grey", [158, 153, 148], 1.0))]);
        let p = payload(&m, "project");
        assert_eq!(
            p.materials,
            [Material::new(
                "Matte grey",
                Some([158.0 / 255.0, 153.0 / 255.0, 148.0 / 255.0]),
                None
            )]
        );
        let attrs = p.face_attrs(p.groups[0].faces[0].attrs).unwrap();
        assert_eq!(attrs.paint.mat.as_deref(), Some("Matte grey"));

        let mut scene = Scene::new();
        skp::apply_payload(&mut scene, &p).unwrap();
        assert!(scene.doc.materials.contains_key("Matte grey"));
        assert_eq!(face_mat(&scene).as_deref(), Some("Matte grey"));
    }

    #[test]
    fn test_skp_reimport_does_not_duplicate() {
        let m = fake_model(vec![(7, material("Wood", [255, 0, 0], 1.0))]);
        let mut scene = Scene::new();
        skp::apply_payload(&mut scene, &payload(&m, "a")).unwrap();
        skp::apply_payload(&mut scene, &payload(&m, "a")).unwrap();
        assert_eq!(scene.doc.materials.keys().collect::<Vec<_>>(), ["Wood"]);
    }

    /// The document already has a DIFFERENT "Wood": the import's lands as
    /// "Wood (2)" and the imported faces follow the rename.
    #[test]
    fn test_skp_import_remaps_on_conflict_with_document() {
        let mut scene = Scene::new();
        scene.doc.materials.insert(
            "Wood".into(),
            Material::new("Wood", Some([0.1, 0.1, 0.1]), None),
        );
        let m = fake_model(vec![(7, material("Wood", [255, 0, 0], 1.0))]);
        skp::apply_payload(&mut scene, &payload(&m, "a")).unwrap();
        let mut names: Vec<&String> = scene.doc.materials.keys().collect();
        names.sort();
        assert_eq!(names, ["Wood", "Wood (2)"]);
        assert_eq!(face_mat(&scene).as_deref(), Some("Wood (2)"));
        assert_eq!(scene.doc.materials["Wood"].color, Some([0.1, 0.1, 0.1]));
    }
}
