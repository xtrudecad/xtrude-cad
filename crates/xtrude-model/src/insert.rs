// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Insert another Xtrude CAD document into the open one, as ONE component.
//!
//! What the file holds becomes the component's tree, nothing gets baked:
//! its loose geometry is the container's own mesh, its groups (with their
//! own children and matrices) its children. A file that is exactly one
//! group inserts as that group.
//!
//! Materials and layers cross over by name: a material whose name is taken
//! by a DIFFERENT recipe registers under "name (2)" and the faces that wear
//! it are restamped (see [`crate::materials::register`]); layers the file
//! uses and the document lacks are created, visible.

use indexmap::IndexMap;
use xtrude_core::attrs::Back;

use crate::group::{iter_placements, Group, MeshTable};
use crate::ids::MeshId;
use crate::layers::{layer_name, Layer, DEFAULT_LAYER};
use crate::materials::{register, Material};
use crate::scene::Doc;

/// Whether `name` is one a group gives itself when nobody named it
/// (`Group N`).
fn is_auto_name(name: &str) -> bool {
    name.strip_prefix("Group ")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The component a loaded document becomes, or `None` when the file holds
/// no geometry at all. `src` is consumed: its meshes move into `meshes`
/// (no copies); the loose mesh gets a fresh id there.
pub fn component_from_doc(src: Doc, meshes: &mut MeshTable, name: &str) -> Option<Group> {
    let Doc {
        meshes: mut src_meshes,
        groups,
        ..
    } = src;
    let loose_mesh = src_meshes.remove(&MeshId::LOOSE);
    let loose = loose_mesh
        .as_ref()
        .is_some_and(|m| !m.faces.is_empty() || !m.edges.is_empty());
    if !loose && groups.is_empty() {
        return None;
    }
    meshes.extend(src_meshes);
    if !loose && groups.len() == 1 {
        let mut comp = groups.into_iter().next().expect("one group");
        if is_auto_name(&comp.name) {
            comp.name = name.to_string();
        }
        return Some(comp);
    }
    let own = MeshId::fresh();
    meshes.insert(own, loose_mesh.unwrap_or_default());
    let mut comp = Group::new(own, Some(name));
    if !groups.is_empty() {
        comp.adopt(groups); // a container is always an instance
    }
    Some(comp)
}

fn mesh_ids(comp: &Group) -> Vec<MeshId> {
    let mut out: Vec<MeshId> = Vec::new();
    for (g, _) in iter_placements(comp, None) {
        if !out.contains(&g.mesh) {
            out.push(g.mesh);
        }
    }
    out
}

/// Register the file's materials in the document, restamping the
/// component's faces where a name had to change. Returns the rename map.
pub fn merge_materials(
    src_materials: &IndexMap<String, Material>,
    dst_materials: &mut IndexMap<String, Material>,
    comp: &Group,
    meshes: &mut MeshTable,
) -> IndexMap<String, String> {
    let mut renamed = IndexMap::new();
    for (old, mat) in src_materials {
        let fin = register(dst_materials, mat.clone());
        if &fin != old {
            renamed.insert(old.clone(), fin);
        }
    }
    if renamed.is_empty() {
        return renamed;
    }
    for id in mesh_ids(comp) {
        let Some(arc) = meshes.get_mut(&id) else {
            continue;
        };
        let mesh = std::sync::Arc::make_mut(arc);
        for f in mesh.face_ids() {
            let a = &mut mesh.face_mut(f).attrs;
            if let Some(new) = a.paint.mat.as_ref().and_then(|m| renamed.get(m)) {
                a.paint.mat = Some(new.clone());
            }
            if let Some(Back::Own(p)) = a.back.as_mut() {
                if let Some(new) = p.mat.as_ref().and_then(|m| renamed.get(m)) {
                    p.mat = Some(new.clone());
                }
            }
        }
    }
    renamed
}

/// Create, visible, every layer the component's entities carry that the
/// document does not have yet. Returns the names added.
pub fn ensure_layers(dst: &mut Doc, comp: &Group, meshes: &MeshTable) -> Vec<String> {
    let mut needed: Vec<String> = Vec::new();
    {
        let have: Vec<&str> = dst.layers.iter().map(|l| l.name.as_str()).collect();
        let mut want = |label: Option<&str>| {
            let n = layer_name(label);
            if n != DEFAULT_LAYER && !have.contains(&n) && !needed.iter().any(|x| x == n) {
                needed.push(n.to_string());
            }
        };
        for (g, _) in iter_placements(comp, None) {
            want(g.layer.as_deref());
            if let Some(m) = meshes.get(&g.mesh) {
                for (_, f) in m.faces.iter() {
                    want(f.attrs.layer.as_deref());
                }
                for (_, e) in m.edges.iter() {
                    want(e.layer.as_deref());
                }
            }
        }
    }
    for n in &needed {
        dst.layers.push(Layer::new(n));
    }
    needed
}

/// The whole of `src` as one component ready for placement in `dst`:
/// meshes moved in, materials merged, layers ensured. `None` = empty file.
pub fn import_document_as_component(dst: &mut Doc, src: Doc, name: &str) -> Option<Group> {
    let materials = src.materials.clone();
    let comp = component_from_doc(src, &mut dst.meshes, name)?;
    merge_materials(&materials, &mut dst.materials, &comp, &mut dst.meshes);
    let meshes = dst.meshes.clone();
    ensure_layers(dst, &comp, &meshes);
    Some(comp)
}
