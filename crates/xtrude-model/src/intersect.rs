// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Add Crossing Edges (Model ▸ Add Crossing Edges ▸ With Everything / Within
//! Selection / Within Open Group): which faces cross which. The crossing geometry itself is
//! [`xtrude_core::intersect`].

use xtrude_core::intersect::{intersect, world_poly, Poly};
use xtrude_core::DVec3;

use crate::entity::EntityRef;
use crate::group::iter_placements;
use crate::scene::Scene;

/// What the selection is intersected with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntersectMode {
    /// The selection against everything it touches (the whole context and
    /// every group in it).
    Model,
    /// Only the selected things against each other.
    Selection,
    /// The selection against the rest of the context being edited.
    Context,
}

/// World polygons for a mix of loose faces and groups/components.
pub fn polys_of(scene: &Scene, entities: &[EntityRef]) -> Vec<Poly> {
    let mut out = Vec::new();
    for e in entities {
        match *e {
            EntityRef::Face(mid, f) => {
                let m = scene.doc.mesh(mid);
                if m.has_face(f) {
                    out.extend(world_poly(m, f, None));
                }
            }
            EntityRef::Group(gid) => {
                if let Some(g) = scene.doc.find_group(gid) {
                    for (pg, xf) in iter_placements(g, None) {
                        let m = scene.doc.mesh(pg.mesh);
                        for f in m.face_ids() {
                            out.extend(world_poly(m, f, xf.as_ref()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The crossing segments Add Crossing Edges would add, for `mode`.
pub fn segments_for(scene: &Scene, mode: IntersectMode) -> Vec<(DVec3, DVec3)> {
    let sel: Vec<EntityRef> = scene
        .selection
        .iter()
        .filter(|e| matches!(e, EntityRef::Face(..) | EntityRef::Group(_)))
        .copied()
        .collect();
    if sel.is_empty() {
        return Vec::new();
    }
    let mine = polys_of(scene, &sel);
    if mode == IntersectMode::Selection {
        return intersect(&mine, &mine, true);
    }
    let mid = scene.active_mesh_id();
    let mut rest: Vec<EntityRef> = scene
        .mesh()
        .face_ids()
        .into_iter()
        .map(|f| EntityRef::Face(mid, f))
        .filter(|e| !sel.contains(e))
        .collect();
    if mode == IntersectMode::Model {
        let kids: Vec<EntityRef> = match scene.edit_group().and_then(|g| scene.doc.find_group(g)) {
            Some(owner) => owner
                .children
                .iter()
                .map(|k| EntityRef::Group(k.id))
                .collect(),
            None => scene
                .doc
                .groups
                .iter()
                .map(|g| EntityRef::Group(g.id))
                .collect(),
        };
        rest.extend(kids.into_iter().filter(|e| !sel.contains(e)));
    }
    let others = polys_of(scene, &rest);
    intersect(&mine, &others, false)
}
