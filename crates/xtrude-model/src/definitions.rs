// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A component's definition: its mesh and its contents.
//!
//! Only components share: two component copies ([`Group::is_component`])
//! with the same [`MeshId`] are copies of one definition, and hold the same
//! contents — the same children, recursively, in the component's local
//! coordinates (same names, matrices, meshes, paint, layers, hidden flags
//! and component flags). Groups never share contents, even when two of
//! them hold the same mesh id.
//!
//! The document file keeps children per copy, so a document can hold copies
//! of one prototype whose contents differ (one edited before the editor
//! shared contents). [`split_divergent_copies`] gives each distinct
//! content its own mesh, so that the invariant holds once a document is
//! in.

use std::collections::HashMap;
use std::sync::Arc;
use xtrude_core::math::{DMat4, MatExt, FUZZY_ZERO};

use xtrude_core::mesh::Mesh;

use crate::group::{copy_group, Group, MeshTable};
use crate::ids::GroupId;
use crate::ids::MeshId;

/// Whether two matrices are equal within [`FUZZY_ZERO`], element by
/// element.
fn same_matrix(a: &Option<DMat4>, b: &Option<DMat4>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.abs_diff_eq(*b, FUZZY_ZERO),
        _ => false,
    }
}

/// Whether two placements inside a definition are the same, with their
/// own contents.
fn same_placement(a: &Group, b: &Group) -> bool {
    a.mesh == b.mesh
        && a.name == b.name
        && same_matrix(&a.xform, &b.xform)
        && a.material == b.material
        && a.layer == b.layer
        && a.hidden == b.hidden
        && a.component == b.component
        && same_contents(a, b)
}

/// Whether two copies hold the same contents: their children, compared
/// recursively in the copies' local coordinates. The copies' own name,
/// matrix, paint and flags are theirs, not their contents, and are not
/// compared.
pub fn same_contents(a: &Group, b: &Group) -> bool {
    same_children(&a.children, &b.children)
}

/// Whether two lists of children are the same, recursively.
pub fn same_children(a: &[Group], b: &[Group]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_placement(x, y))
}

/// Every group of `groups` and below, depth first (each group before its
/// children).
fn walk_all(groups: &[Group]) -> Vec<&Group> {
    let mut out = Vec::new();
    for g in groups {
        g.walk(&mut out);
    }
    out
}

fn assign(groups: &mut [Group], meshes: &[Option<MeshId>], at: &mut usize) {
    for g in groups {
        if let Some(id) = meshes[*at] {
            g.mesh = id;
        }
        *at += 1;
        assign(&mut g.children, meshes, at);
    }
}

/// Split the component copies in `groups` (at every depth) that share a
/// mesh but not their contents: each distinct content gets a copy of the
/// mesh of its own, and the copies holding the first content met (depth
/// first, in creation order) keep the original. Groups are left alone.
/// Returns how many meshes were added.
pub fn split_divergent_copies(meshes: &mut MeshTable, groups: &mut [Group]) -> usize {
    let all = walk_all(groups);
    // Per mesh, one representative copy per distinct content, in the order
    // met; the first keeps the mesh.
    let mut kinds: HashMap<MeshId, Vec<(&Group, Option<MeshId>)>> = HashMap::new();
    let mut new_mesh: Vec<Option<MeshId>> = vec![None; all.len()];
    let mut added = 0;
    for (i, g) in all.iter().enumerate() {
        if !g.is_component() {
            continue;
        }
        let seen = kinds.entry(g.mesh).or_default();
        if let Some((_, id)) = seen.iter().find(|(rep, _)| same_contents(rep, g)) {
            new_mesh[i] = *id;
            continue;
        }
        let id = if seen.is_empty() {
            None
        } else {
            let id = MeshId::fresh();
            let src = meshes.get(&g.mesh).cloned().unwrap_or_default();
            meshes.insert(id, src);
            added += 1;
            Some(id)
        };
        seen.push((g, id));
        new_mesh[i] = id;
    }
    assign(groups, &new_mesh, &mut 0);
    added
}

/// Give every group (not a component copy) that holds `proto`, at every
/// depth, a mesh id of its own with the faces it has now, so that writing
/// into `proto` changes the component's copies only.
pub fn detach_groups(meshes: &mut MeshTable, groups: &mut [Group], proto: MeshId) {
    let Some(faces) = meshes.get(&proto).cloned() else {
        return;
    };
    fn rec(meshes: &mut MeshTable, groups: &mut [Group], proto: MeshId, faces: &Arc<Mesh>) {
        for g in groups {
            if g.mesh == proto && !g.is_component() {
                let id = MeshId::fresh();
                meshes.insert(id, Arc::clone(faces));
                g.mesh = id;
            }
            rec(meshes, &mut g.children, proto, faces);
        }
    }
    rec(meshes, groups, proto, &faces);
}

/// Undo pushing the matrix `x` down into an open container's children:
/// each child back in the container's local coordinates. A child whose
/// placement comes back within [`FUZZY_ZERO`] of the one it had on
/// entering (`entry`, by id) gets that one exactly, so a look changes
/// nothing; one that had no matrix gets its `component` flag back too.
pub fn pull_matrix_up(container: &mut Group, x: &DMat4, entry: &[Group]) {
    let inv = x.try_inverse().unwrap_or(DMat4::IDENTITY);
    for c in container.children.iter_mut() {
        let orig = entry.iter().find(|e| e.id == c.id);
        let local = inv * c.xform.unwrap_or(DMat4::IDENTITY);
        let before = orig.map(|o| o.xform.unwrap_or(DMat4::IDENTITY));
        c.xform = match (orig, before) {
            (Some(o), Some(b)) if b.abs_diff_eq(local, FUZZY_ZERO) => {
                if o.xform.is_none() {
                    c.component = o.component;
                }
                o.xform
            }
            _ => Some(local),
        };
        if let Some(off) = c.explode_offset {
            let local = inv.transform_vector(off);
            c.explode_offset = Some(match orig.and_then(|o| o.explode_offset) {
                Some(b) if b.abs_diff_eq(local, FUZZY_ZERO) => b,
                _ => local,
            });
        }
    }
}

/// Give every other copy of `source`'s definition (the component copies
/// holding its mesh, at every depth, outside `source`) a copy of
/// `source`'s children: fresh ids and uids, shared meshes. Children
/// without a matrix get one first, as `copy_group` gives them, so that
/// every copy holds the same contents. Returns how many copies changed.
pub fn share_children(meshes: &mut MeshTable, groups: &mut [Group], source: GroupId) -> usize {
    fn find(groups: &mut [Group], id: GroupId) -> Option<&mut Group> {
        groups.iter_mut().find_map(|g| g.find_mut(id))
    }
    fn placed_in_definition(g: &mut Group) {
        for c in g.children.iter_mut() {
            if c.xform.is_none() {
                c.xform = Some(DMat4::IDENTITY);
                c.component = false;
            }
            placed_in_definition(c);
        }
    }
    fn rec(meshes: &mut MeshTable, groups: &mut [Group], src: &Group, changed: &mut usize) {
        for g in groups {
            if g.id == src.id {
                continue;
            }
            if g.is_component() && g.mesh == src.mesh {
                g.children = src
                    .children
                    .iter()
                    .map(|c| copy_group(meshes, &mut c.clone(), None, true))
                    .collect();
                *changed += 1;
                continue;
            }
            rec(meshes, &mut g.children, src, changed);
        }
    }
    let Some(g) = find(groups, source) else {
        return 0;
    };
    placed_in_definition(g);
    let src = g.clone();
    let mut changed = 0;
    rec(meshes, groups, &src, &mut changed);
    changed
}
