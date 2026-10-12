// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The group tree behind the model tree panel: who owns which group (found by a
//! walk from the root — groups keep no back-pointer).

use std::collections::HashMap;

use xtrude_core::math::DMat4;

use crate::group::Group;
use crate::ids::GroupId;
use crate::scene::Scene;

/// `group → parent` (`None` for the top level) over the whole tree.
pub fn parent_map(scene: &Scene) -> HashMap<GroupId, Option<GroupId>> {
    let mut out = HashMap::new();
    fn walk(nodes: &[Group], parent: Option<GroupId>, out: &mut HashMap<GroupId, Option<GroupId>>) {
        for g in nodes {
            out.insert(g.id, parent);
            walk(&g.children, Some(g.id), out);
        }
    }
    walk(&scene.doc.groups, None, &mut out);
    out
}

/// The groups `group` hangs from, outermost first.
pub fn ancestors(parents: &HashMap<GroupId, Option<GroupId>>, group: GroupId) -> Vec<GroupId> {
    let mut chain = Vec::new();
    let mut p = parents.get(&group).copied().flatten();
    while let Some(id) = p {
        chain.push(id);
        p = parents.get(&id).copied().flatten();
    }
    chain.reverse();
    chain
}

/// The world matrix of the frame at the end of `chain` (outermost first);
/// `None` when none of them carries one.
pub fn chain_matrix(scene: &Scene, chain: &[GroupId]) -> Option<DMat4> {
    let mut m: Option<DMat4> = None;
    for id in chain {
        if let Some(x) = scene.doc.find_group(*id).and_then(|g| g.xform) {
            m = Some(match m {
                None => x,
                Some(mm) => mm * x,
            });
        }
    }
    m
}

/// Whether a frame sits inside a component instance.
pub fn inside_component(scene: &Scene, chain: &[GroupId]) -> bool {
    chain
        .iter()
        .any(|id| scene.doc.find_group(*id).is_some_and(|g| g.is_component()))
}

/// Whether `groups` may move under `parent` (`None` = top level).
pub fn can_reparent(
    scene: &Scene,
    groups: &[GroupId],
    parent: Option<GroupId>,
    ignore_edit: bool,
) -> bool {
    if groups.is_empty() || (scene.edit_group().is_some() && !ignore_edit) {
        return false;
    }
    let parents = parent_map(scene);
    if let Some(p) = parent {
        if !parents.contains_key(&p) {
            return false;
        }
        let mut target = ancestors(&parents, p);
        target.push(p);
        if inside_component(scene, &target) {
            return false;
        }
        if groups.iter().any(|g| target.contains(g)) {
            return false;
        }
    }
    for g in groups {
        if !parents.contains_key(g) {
            return false;
        }
        if inside_component(scene, &ancestors(&parents, *g)) {
            return false;
        }
    }
    true
}
