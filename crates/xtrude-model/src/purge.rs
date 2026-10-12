// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Purge unused — for layers and materials. The traversal descends into
//! nested placements: a name used only inside one is IN USE.

use std::collections::HashSet;

use crate::ids::MeshId;
use crate::layers::{layer_name, DEFAULT_LAYER};
use crate::scene::Scene;

/// The loose mesh plus every group's mesh, each distinct mesh once.
pub fn iter_meshes(scene: &Scene) -> Vec<MeshId> {
    let mut out = vec![MeshId::LOOSE];
    for g in scene.doc.all_groups() {
        if !out.contains(&g.mesh) {
            out.push(g.mesh);
        }
    }
    out
}

/// Every layer name something in the document carries.
pub fn used_layers(scene: &Scene) -> HashSet<String> {
    let mut used: HashSet<String> = HashSet::new();
    used.insert(DEFAULT_LAYER.to_string());
    for mid in iter_meshes(scene) {
        let m = scene.doc.mesh(mid);
        for (_, f) in m.faces.iter() {
            used.insert(layer_name(f.attrs.layer.as_deref()).to_string());
        }
        for (_, e) in m.edges.iter() {
            used.insert(layer_name(e.layer.as_deref()).to_string());
        }
    }
    for g in scene.doc.all_groups() {
        if let Some(l) = &g.layer {
            used.insert(l.clone());
        }
    }
    for d in &scene.doc.dimensions {
        used.insert(layer_name(d.layer.as_deref()).to_string());
    }
    for t in &scene.doc.text_labels {
        used.insert(layer_name(t.layer.as_deref()).to_string());
    }
    used
}

/// Layer names a saved view switches off.
pub fn layers_held_by_scenes(scene: &Scene) -> HashSet<String> {
    scene
        .saved_views
        .iter()
        .flat_map(|v| v.hidden_layers.iter().cloned())
        .collect()
}

/// The layer names a purge would remove.
pub fn unused_layers(scene: &Scene) -> Vec<String> {
    let mut keep = used_layers(scene);
    keep.extend(layers_held_by_scenes(scene));
    scene
        .doc
        .layers
        .iter()
        .filter(|l| l.name != DEFAULT_LAYER && !keep.contains(&l.name))
        .map(|l| l.name.clone())
        .collect()
}

/// Every registry material name a face (or a container) wears.
pub fn used_materials(scene: &Scene) -> HashSet<String> {
    let mut used = HashSet::new();
    for mid in iter_meshes(scene) {
        for (_, f) in scene.doc.mesh(mid).faces.iter() {
            if let Some(n) = &f.attrs.paint.mat {
                if !n.is_empty() {
                    used.insert(n.clone());
                }
            }
        }
    }
    for g in scene.doc.all_groups() {
        if let Some(n) = g.material.as_ref().and_then(|p| p.mat.as_ref()) {
            if !n.is_empty() {
                used.insert(n.clone());
            }
        }
    }
    used
}

/// Registry material names no face wears, in registry order.
pub fn unused_materials(scene: &Scene) -> Vec<String> {
    let used = used_materials(scene);
    scene
        .doc
        .materials
        .keys()
        .filter(|n| !used.contains(*n))
        .cloned()
        .collect()
}
