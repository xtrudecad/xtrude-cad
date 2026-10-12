// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shared geometry collection for the mesh-interchange exporters (STL,
//! OBJ, glTF, DAE): every renderable face in WORLD coordinates,
//! triangulated, and grouped by material.

use indexmap::IndexMap;
use xtrude_core::attrs::{FaceAttrs, Rgb};
use xtrude_core::{DVec3, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::world_mesh;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

pub const DEFAULT_COLOR: Rgb = [0.96, 0.95, 0.925];

/// One face in world space.
#[derive(Clone, Debug)]
pub struct WorldFace {
    pub outer: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    pub normal: DVec3,
    pub tris: Vec<[DVec3; 3]>,
    pub attrs: FaceAttrs,
}

fn push_mesh_faces(
    out: &mut Vec<WorldFace>,
    m: &Mesh,
    only: Option<&dyn Fn(xtrude_core::FaceId) -> bool>,
) {
    for f in m.face_ids() {
        if let Some(keep) = only {
            if !keep(f) {
                continue;
            }
        }
        out.push(WorldFace {
            outer: m.face_positions(f),
            holes: m.face_hole_positions(f),
            normal: m.face_normal(f),
            tris: m.face_triangulate(f, None),
            attrs: m.face(f).attrs.clone(),
        });
    }
}

/// Every renderable face in WORLD space: the visible loose faces, then the
/// visible groups. Component instances share a prototype mesh in local
/// coordinates, so their faces come from a transformed copy.
/// `nested_counts`: the exporters (glTF, DAE, OBJ) also take the baked path
/// for a group with children; STL looked at matrices only.
pub fn world_faces(scene: &Scene, nested_counts: bool) -> Vec<WorldFace> {
    let mut out = Vec::new();
    let loose = scene.loose_mesh();
    let visible = |f: xtrude_core::FaceId| scene.entity_visible(&EntityRef::Face(MeshId::LOOSE, f));
    push_mesh_faces(&mut out, loose, Some(&visible));
    let baked = scene
        .doc
        .groups
        .iter()
        .any(|g| g.xform.is_some() || (nested_counts && !g.children.is_empty()));
    if !baked {
        for (g, _m) in scene.placements() {
            push_mesh_faces(&mut out, scene.doc.mesh(g.mesh), None);
        }
        return out;
    }
    for g in &scene.doc.groups {
        if !scene.entity_visible(&EntityRef::Group(g.id)) {
            continue;
        }
        push_mesh_faces(&mut out, &world_mesh(&scene.doc.meshes, g), None);
    }
    out
}

/// A material key: the colour, plus the opacity when translucent (the
/// opacity rounded to 3 decimals).
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialKey {
    pub color: Rgb,
    pub opacity: Option<f64>,
}

impl MaterialKey {
    fn bits(&self) -> ([u64; 3], Option<u64>) {
        (self.color.map(f64::to_bits), self.opacity.map(f64::to_bits))
    }
}

impl Eq for MaterialKey {}

impl std::hash::Hash for MaterialKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.bits().hash(state);
    }
}

/// What a material key exports as.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialInfo {
    pub color: Rgb,
    pub opacity: Option<f64>,
    /// The registry identity of the first face seen with one.
    pub mat: Option<String>,
}

/// A triangle with its face normal.
pub type Tri = (DVec3, [DVec3; 3]);

/// The material key of a face's attrs.
pub fn material_key(attrs: &FaceAttrs) -> MaterialKey {
    let op = attrs
        .paint
        .opacity
        .filter(|&o| o < 0.999)
        .map(|o| xtrude_core::stable_json::round_exact(o, 3));
    MaterialKey {
        color: attrs.paint.color.unwrap_or(DEFAULT_COLOR),
        opacity: op,
    }
}

/// Group the scene's triangles by material, in first-seen order.
pub fn collect_geometry(
    scene: &Scene,
) -> (
    IndexMap<MaterialKey, MaterialInfo>,
    IndexMap<MaterialKey, Vec<Tri>>,
) {
    let mut materials: IndexMap<MaterialKey, MaterialInfo> = IndexMap::new();
    let mut prims: IndexMap<MaterialKey, Vec<Tri>> = IndexMap::new();
    for face in world_faces(scene, true) {
        let key = material_key(&face.attrs);
        let info = materials
            .entry(key.clone())
            .or_insert_with(|| MaterialInfo {
                color: key.color,
                opacity: key.opacity,
                mat: None,
            });
        if info.mat.is_none() {
            if let Some(m) = face.attrs.paint.mat.as_ref().filter(|m| !m.is_empty()) {
                info.mat = Some(m.clone());
            }
        }
        for t in face.tris {
            prims.entry(key.clone()).or_default().push((face.normal, t));
        }
    }
    (materials, prims)
}

/// `key` → a unique, export-safe material name: the registry identity
/// sanitized (whitespace → `_`, anything but alnum/`._-` dropped, a
/// leading digit prefixed), else `matN`; collisions get `_2` suffixes.
pub fn export_names(
    materials: &IndexMap<MaterialKey, MaterialInfo>,
) -> IndexMap<MaterialKey, String> {
    let mut names = IndexMap::new();
    let mut used = std::collections::HashSet::new();
    for (i, (key, info)) in materials.iter().enumerate() {
        let raw = info.mat.as_deref().unwrap_or("").trim();
        let base = if raw.is_empty() {
            format!("mat{i}")
        } else {
            let cleaned: String = raw
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || "._-".contains(c) {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let trimmed = cleaned.trim_matches(|c| "._-".contains(c));
            let mut b = if trimmed.is_empty() {
                format!("mat{i}")
            } else {
                trimmed.to_string()
            };
            if b.chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c.is_numeric())
            {
                b = format!("m_{b}");
            }
            b
        };
        let mut name = base.clone();
        let mut n = 2;
        while used.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        used.insert(name.clone());
        names.insert(key.clone(), name);
    }
    names
}
