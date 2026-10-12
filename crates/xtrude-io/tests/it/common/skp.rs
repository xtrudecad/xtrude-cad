// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Hand-built OpenSKP models for the `.skp` adapter tests. The implicit
//! root is `model.root`, as
//! `SkpFile::parse` gives it.

use openskp::{Definition, Edge, Face, Instance, Material, SkpModel, Vertex};

/// A face `(id, loops)`, each loop a list of `(edge id, orientation)`.
pub type FaceSpec = (i64, Vec<Vec<(i64, i64)>>);

/// Vertices `(id, (x, y, z))` in inches, edges
/// `(id, (v1, v2))`, faces `(id, loops)`.
pub fn defn(
    id: i64,
    name: &str,
    verts: &[(i64, [f64; 3])],
    edges: &[(i64, (i64, i64))],
    faces: &[FaceSpec],
    instances: Vec<Instance>,
) -> Definition {
    let mut d = Definition {
        id,
        name: name.to_string(),
        instances,
        ..Default::default()
    };
    for &(vid, [x, y, z]) in verts {
        d.vertices.insert(vid, Vertex { id: vid, x, y, z });
    }
    for &(eid, (a, b)) in edges {
        d.edges.insert(
            eid,
            Edge {
                id: eid,
                v1_id: a,
                v2_id: b,
                ..Default::default()
            },
        );
    }
    for (fid, loops) in faces {
        d.faces.insert(
            *fid,
            Face {
                id: *fid,
                loops: loops.clone(),
                normal: Some([0.0, 0.0, 1.0]),
                ..Default::default()
            },
        );
    }
    d
}

/// An empty definition (no geometry).
pub fn empty_defn(id: i64, name: &str, instances: Vec<Instance>) -> Definition {
    defn(id, name, &[], &[], &[], instances)
}

/// A 10-inch right triangle whose loop starts with edge 12
/// (3→1), so the ring starts at vertex 1.
pub fn tri_def(id: i64, name: &str, instances: Vec<Instance>) -> Definition {
    defn(
        id,
        name,
        &[
            (1, [0.0, 0.0, 0.0]),
            (2, [10.0, 0.0, 0.0]),
            (3, [10.0, 10.0, 0.0]),
        ],
        &[(10, (1, 2)), (11, (2, 3)), (12, (3, 1))],
        &[(20, vec![vec![(12, 1), (10, 1), (11, 1)]])],
        instances,
    )
}

/// A placement of definition `ref_idx` translated `x` inches along X.
pub fn ins(ref_idx: i64, x: f64) -> Instance {
    Instance {
        name: String::new(),
        ref_idx: Some(ref_idx),
        guid: Some(String::new()),
        matrix: vec![
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, x, 0.0, 0.0, 1.0,
        ],
        ..Default::default()
    }
}

/// A model of `root` and `defs`.
pub fn model(root: Definition, defs: Vec<Definition>) -> SkpModel {
    let mut m = SkpModel {
        root,
        ..Default::default()
    };
    for d in defs {
        m.definitions.insert(Some(d.id), d);
    }
    m
}

/// A material of colour `rgb` (0–255) and opacity `transparency`.
pub fn material(name: &str, rgb: [i64; 3], transparency: f64) -> Material {
    Material {
        color: [rgb[0], rgb[1], rgb[2], 255],
        transparency,
        ..Material::new(name)
    }
}

/// Join `materials` into `model` by id (`materials_by_id`).
pub fn with_materials(mut model: SkpModel, materials: Vec<(i64, Material)>) -> SkpModel {
    for (id, mut m) in materials {
        m.id = Some(id);
        model.materials.push(m);
        model.materials_by_id.insert(id, model.materials.len() - 1);
    }
    model
}
