// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Right-click ▸ Select: grow a selection by what it touches or by what it
//! shares. Answers from the CURRENT editing context.

use std::collections::HashSet;

use indexmap::IndexSet;
use xtrude_core::mesh::{EdgeId, FaceId, Mesh, VertexId};

use crate::entity::EntityRef;
use crate::ids::{GroupId, MeshId};
use crate::layers::layer_name;
use crate::scene::Scene;

/// Everything physically connected to `entities` — the whole solid — walked
/// through shared vertices (the classic triple click).
pub fn all_connected(mesh: &Mesh, mid: MeshId, entities: &[EntityRef]) -> Vec<EntityRef> {
    let mut seeds: Vec<VertexId> = Vec::new();
    for e in entities {
        match *e {
            EntityRef::Face(m, f) if m == mid && mesh.has_face(f) => {
                for lp in mesh.face(f).loops() {
                    seeds.extend(lp.iter().copied());
                }
            }
            EntityRef::Edge(m, ed) if m == mid && mesh.has_edge(ed) => {
                seeds.push(mesh.edge(ed).v0);
                seeds.push(mesh.edge(ed).v1);
            }
            _ => {}
        }
    }
    let mut seen_v: HashSet<VertexId> = seeds.iter().copied().collect();
    let mut edges: IndexSet<EdgeId> = IndexSet::new();
    let mut faces: IndexSet<FaceId> = IndexSet::new();
    let mut stack: Vec<VertexId> = seen_v.iter().copied().collect();
    while let Some(v) = stack.pop() {
        for &e in &mesh.verts[v].edges {
            if !edges.insert(e) {
                continue;
            }
            for &f in &mesh.edge(e).faces {
                if !faces.insert(f) {
                    continue;
                }
                for lp in mesh.face(f).loops() {
                    for &w in lp {
                        if seen_v.insert(w) {
                            stack.push(w);
                        }
                    }
                }
            }
            let w = mesh.edge(e).other(v);
            if seen_v.insert(w) {
                stack.push(w);
            }
        }
    }
    edges
        .into_iter()
        .map(|e| EntityRef::Edge(mid, e))
        .chain(faces.into_iter().map(|f| EntityRef::Face(mid, f)))
        .collect()
}

/// The edges that outline the selected faces: exactly ONE selected face on
/// them.
pub fn bounding_edges(mesh: &Mesh, mid: MeshId, entities: &[EntityRef]) -> Vec<EntityRef> {
    let faces: HashSet<FaceId> = entities
        .iter()
        .filter_map(|e| {
            e.as_face()
                .filter(|(m, f)| *m == mid && mesh.has_face(*f))
                .map(|(_, f)| f)
        })
        .collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut flist: Vec<FaceId> = faces.iter().copied().collect();
    flist.sort();
    for f in flist {
        for lp in mesh.face(f).loops() {
            let n = lp.len();
            for i in 0..n {
                let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % n]) else {
                    continue;
                };
                if !mesh.edge(e).faces.contains(&f) || !seen.insert(e) {
                    continue;
                }
                if mesh
                    .edge(e)
                    .faces
                    .iter()
                    .filter(|g| faces.contains(g))
                    .count()
                    == 1
                {
                    out.push(EntityRef::Edge(mid, e));
                }
            }
        }
    }
    out
}

/// What «the same material» means for a face: its named material, else its
/// colour; `None` = unpainted.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MaterialKey {
    Mat(String),
    Paint([i64; 3]),
}

pub fn material_key(attrs: &xtrude_core::attrs::FaceAttrs) -> Option<MaterialKey> {
    if let Some(m) = attrs.paint.mat.as_ref().filter(|m| !m.is_empty()) {
        return Some(MaterialKey::Mat(m.clone()));
    }
    attrs
        .paint
        .color
        .map(|c| MaterialKey::Paint(c.map(|x| xtrude_core::math::round_even_int(x * 1e4))))
}

fn context_groups(scene: &Scene) -> Vec<GroupId> {
    match scene.edit_group() {
        None => scene.doc.groups.iter().map(|g| g.id).collect(),
        Some(c) => scene
            .doc
            .find_group(c)
            .map(|g| g.children.iter().map(|k| k.id).collect())
            .unwrap_or_default(),
    }
}

/// Every face in the current context painted like one of the selected
/// faces; groups matched by their own material.
pub fn same_material(scene: &Scene, entities: &[EntityRef]) -> Vec<EntityRef> {
    let mid = scene.active_mesh_id();
    let mesh = scene.mesh();
    let keys: HashSet<Option<MaterialKey>> = entities
        .iter()
        .filter_map(|e| e.as_face())
        .filter(|(m, f)| scene.doc.mesh(*m).has_face(*f))
        .map(|(m, f)| material_key(&scene.doc.mesh(m).face(f).attrs))
        .collect();
    let mut out: Vec<EntityRef> = if keys.is_empty() {
        Vec::new()
    } else {
        mesh.face_ids()
            .into_iter()
            .filter(|&f| keys.contains(&material_key(&mesh.face(f).attrs)))
            .map(|f| EntityRef::Face(mid, f))
            .collect()
    };
    let group_mats: Vec<_> = entities
        .iter()
        .filter_map(|e| e.as_group())
        .filter_map(|g| scene.doc.find_group(g).and_then(|g| g.material.clone()))
        .collect();
    if !group_mats.is_empty() {
        for g in context_groups(scene) {
            if let Some(m) = scene.doc.find_group(g).and_then(|g| g.material.clone()) {
                if group_mats.contains(&m) {
                    out.push(EntityRef::Group(g));
                }
            }
        }
    }
    out
}

/// Every edge, face and group in the current context on one of the
/// selected entities' layers.
pub fn same_layer(scene: &Scene, entities: &[EntityRef]) -> Vec<EntityRef> {
    let layers: HashSet<String> = entities
        .iter()
        .filter(|e| {
            matches!(
                e,
                EntityRef::Face(..) | EntityRef::Edge(..) | EntityRef::Group(..)
            )
        })
        .map(|e| layer_name(scene.entity_layer(e).as_deref()).to_string())
        .collect();
    if layers.is_empty() {
        return Vec::new();
    }
    let mid = scene.active_mesh_id();
    let mesh = scene.mesh();
    let mut pool: Vec<EntityRef> = mesh
        .edge_ids()
        .into_iter()
        .map(|e| EntityRef::Edge(mid, e))
        .collect();
    pool.extend(mesh.face_ids().into_iter().map(|f| EntityRef::Face(mid, f)));
    pool.extend(context_groups(scene).into_iter().map(EntityRef::Group));
    pool.into_iter()
        .filter(|e| layers.contains(layer_name(scene.entity_layer(e).as_deref())))
        .collect()
}
