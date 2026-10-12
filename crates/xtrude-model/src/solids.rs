// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Solid booleans on groups and components — the Solids tools (the
//! arithmetic is `xtrude_core::solids`).
//!
//! A **solid** is a group or component instance with a closed volume and no
//! nested groups. The result is always a **group**; every face keeps its
//! material, a cut takes the cutter's, a container's paint is baked onto
//! the faces that wore it. The result goes on the layer of the solid that
//! receives the operation.

use xtrude_core::solids::{run as run_core, solid_report, SolidInput};
pub use xtrude_core::solids::{SolidError, SolidOp};

use crate::entity::EntityRef;
use crate::group::{add_mesh, world_mesh, Group, MeshTable};
use crate::history::{CmdError, CmdResult, Command};
use crate::ids::GroupId;
use crate::scene::Scene;

/// The group's volume when it is a solid, else `None`. Nested groups
/// disqualify it.
pub fn solid_volume(meshes: &MeshTable, group: &Group) -> Option<f64> {
    if !group.children.is_empty() {
        return None;
    }
    let (ok, vol) = solid_report(&world_mesh(meshes, group));
    if ok {
        vol
    } else {
        None
    }
}

pub fn is_solid(meshes: &MeshTable, group: &Group) -> bool {
    solid_volume(meshes, group).is_some()
}

/// Run `op` on `groups` (pick order: for Subtract and Trim the first cuts
/// the second). Returns `(removed ids, created groups)`; the created groups'
/// meshes are added to `meshes`.
pub fn run(
    op: SolidOp,
    meshes: &mut MeshTable,
    groups: &[&Group],
) -> Result<(Vec<GroupId>, Vec<Group>), SolidError> {
    if groups.len() < 2 {
        return Err(SolidError("two solids".into()));
    }
    for g in groups {
        if !is_solid(meshes, g) {
            return Err(SolidError("not a solid".into()));
        }
    }
    let worlds: Vec<_> = groups.iter().map(|g| world_mesh(meshes, g)).collect();
    let inputs: Vec<SolidInput> = worlds
        .iter()
        .zip(groups.iter())
        .map(|(m, g)| SolidInput {
            mesh: m,
            paint: g.material.as_ref(),
        })
        .collect();
    let res = run_core(op, &inputs)?;
    let removed = res.removed.iter().map(|&i| groups[i].id).collect();
    let created = res
        .meshes
        .into_iter()
        .map(|(m, layer_of)| {
            let id = add_mesh(meshes, m);
            let mut g = Group::new(id, Some(op.result_name()));
            g.layer = groups[layer_of].layer.clone();
            g
        })
        .collect();
    Ok((removed, created))
}

/// One solid operation as one undo step: the inputs leave the list they
/// live in, the results take the place of the first input that left, and
/// they end up selected.
pub struct SolidOperationCommand {
    pub op: SolidOp,
    pub groups: Vec<GroupId>,
    pub created: Vec<GroupId>,
}

impl SolidOperationCommand {
    pub fn new(op: SolidOp, groups: Vec<GroupId>) -> Self {
        SolidOperationCommand {
            op,
            groups,
            created: Vec::new(),
        }
    }

    /// Run the boolean on a copy to surface a [`SolidError`] before the
    /// command executes.
    pub fn prepare(&self, scene: &Scene) -> Result<(), SolidError> {
        let mut meshes = scene.doc.meshes.clone();
        let gs: Vec<&Group> = self
            .groups
            .iter()
            .filter_map(|&g| scene.doc.find_group(g))
            .collect();
        if gs.len() != self.groups.len() {
            return Err(SolidError("not a solid".into()));
        }
        run(self.op, &mut meshes, &gs).map(|_| ())
    }
}

impl Command for SolidOperationCommand {
    fn name(&self) -> &str {
        "SolidOperationCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mut meshes = std::mem::take(&mut scene.doc.meshes);
        let result = {
            let gs: Vec<&Group> = self
                .groups
                .iter()
                .filter_map(|&g| scene.doc.find_group(g))
                .collect();
            if gs.len() != self.groups.len() {
                Err(SolidError("not a solid".into()))
            } else {
                run(self.op, &mut meshes, &gs)
            }
        };
        scene.doc.meshes = meshes;
        let (removed, created) = result.map_err(|e| CmdError::msg(e.0))?;
        let home_ref = if matches!(self.op, SolidOp::Subtract | SolidOp::Trim) {
            self.groups[self.groups.len() - 1]
        } else {
            self.groups[0]
        };
        let home = scene.doc.parent_of(home_ref).unwrap_or(None);
        let mut slots: Vec<(Option<GroupId>, usize)> = Vec::new();
        for gid in &removed {
            if let Some((parent, idx, _)) = scene.doc.take_group(*gid) {
                slots.push((parent, idx));
            }
            scene.discard(&EntityRef::Group(*gid));
        }
        let at = slots
            .iter()
            .filter(|(p, _)| *p == home)
            .map(|(_, i)| *i)
            .min();
        let list = scene
            .doc
            .siblings_mut(home)
            .ok_or_else(|| CmdError::msg("container is gone"))?;
        let at = at.unwrap_or(list.len()).min(list.len());
        self.created = created.iter().map(|g| g.id).collect();
        for (k, g) in created.into_iter().enumerate() {
            list.insert(at + k, g);
        }
        scene.selection.clear();
        scene
            .selection
            .extend(self.created.iter().map(|&g| EntityRef::Group(g)));
        scene.version += 1;
        Ok(())
    }
}
