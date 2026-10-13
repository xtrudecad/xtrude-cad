// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Print materials: what a part is printed in, and with it its density
//! and mass.
//!
//! A document has a default print material ([`Doc::print_material`], PLA
//! for a new document). Any group or component may override it
//! ([`Group::print_material`]); a nested group without one takes its
//! top-level group's (the part it belongs to), then the document's. A
//! component's copies share their definition's material, as they share
//! its mesh.
//!
//! Masses are at 100 % infill: how a slicer fills the part is the
//! slicer's business. Densities are in kg/m³, the model being in metres.

use xtrude_core::solids::solid_report;

use crate::group::{iter_placements, Group};
use crate::history::{CmdResult, Command};
use crate::ids::GroupId;
use crate::scene::{parents_placement, Doc, Scene};

/// A print material: a name and a density in kg/m³.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintMaterial {
    pub name: String,
    pub density: f64,
}

/// The built-in print materials, `(name, density in kg/m³)`, in the order
/// the menus list them. Each density is the base resin's typical figure
/// from a manufacturer's datasheet (filaments of one kind differ by a few
/// per cent):
/// - PLA 1.24 g/cm³: NatureWorks Ingeo 4043D, the usual filament grade
///   (specific gravity, ASTM D792);
/// - PETG 1.27: Eastman Eastar 6763 copolyester (ISO 1183);
/// - ABS 1.04: Polymaker PolyLite ABS (Polymaker's "Technical data at a
///   glance" table);
/// - ASA 1.07: INEOS Styrolution Luran S 777K (ISO 1183);
/// - TPU 1.21: Polymaker PolyFlex TPU90 (same table);
/// - PA (nylon) 1.14: unfilled PA66, Zytel 101L NC010, dry (ISO 1183);
/// - PC 1.20: Covestro Makrolon 2405 (ISO 1183).
pub const BUILT_IN: [(&str, f64); 7] = [
    ("PLA", 1240.0),
    ("PETG", 1270.0),
    ("ABS", 1040.0),
    ("ASA", 1070.0),
    ("TPU", 1210.0),
    ("PA (nylon)", 1140.0),
    ("PC", 1200.0),
];

impl PrintMaterial {
    pub fn new(name: &str, density: f64) -> Self {
        PrintMaterial {
            name: name.to_string(),
            density,
        }
    }

    /// The built-in material called `name`, if there is one.
    pub fn built_in(name: &str) -> Option<Self> {
        BUILT_IN
            .iter()
            .find(|(n, _)| *n == name)
            .map(|&(n, d)| PrintMaterial::new(n, d))
    }

    /// Every built-in material, in [`BUILT_IN`] order.
    pub fn built_ins() -> Vec<Self> {
        BUILT_IN
            .iter()
            .map(|&(n, d)| PrintMaterial::new(n, d))
            .collect()
    }

    /// Whether this is one of the built-in materials, density included.
    pub fn is_built_in(&self) -> bool {
        PrintMaterial::built_in(&self.name).is_some_and(|b| b == *self)
    }

    /// Whether a material read or typed is usable: a name and a finite,
    /// positive density.
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && self.density.is_finite() && self.density > 0.0
    }
}

/// A new document's print material: PLA.
pub fn default_print_material() -> PrintMaterial {
    PrintMaterial::new(BUILT_IN[0].0, BUILT_IN[0].1)
}

/// The material `group` is printed in, its top-level group being `link`:
/// its own, else `link`'s, else the document's.
fn material_in<'a>(doc: &'a Doc, link: &'a Group, group: &'a Group) -> &'a PrintMaterial {
    group
        .print_material
        .as_ref()
        .or(link.print_material.as_ref())
        .unwrap_or(&doc.print_material)
}

/// The print material of group `id`: its own, else its top-level group's,
/// else the document's.
pub fn print_material_of(doc: &Doc, id: GroupId) -> &PrintMaterial {
    print_material_from(doc, id).0
}

/// Where a group's print material comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialFrom {
    /// The group's own.
    Own,
    /// Its top-level group's (the part it belongs to).
    Part(GroupId),
    /// The document's.
    Document,
}

/// The print material of group `id` and where it comes from.
pub fn print_material_from(doc: &Doc, id: GroupId) -> (&PrintMaterial, MaterialFrom) {
    let Some(path) = doc.group_path(id) else {
        return (&doc.print_material, MaterialFrom::Document);
    };
    let (link, group) = (path[0], path[path.len() - 1]);
    match (&group.print_material, &link.print_material) {
        (Some(m), _) => (m, MaterialFrom::Own),
        (None, Some(m)) => (m, MaterialFrom::Part(link.id)),
        (None, None) => (&doc.print_material, MaterialFrom::Document),
    }
}

/// The mass of group `id` in kg, at 100 % infill, or `None` when it is not
/// made of closed solids.
///
/// Every placement in the group (the group itself and its nested groups)
/// that holds faces must be a closed solid on its own (the solids' rule,
/// [`solid_report`]); each weighs its volume, scaled by its placement,
/// times its material's density. A group whose faces are all in its
/// nested groups (a part made of several bodies) weighs their sum.
pub fn group_mass(doc: &Doc, id: GroupId) -> Option<f64> {
    let path = doc.group_path(id)?;
    let (link, group) = (path[0], path[path.len() - 1]);
    let base = parents_placement(&path);
    let mut total = 0.0;
    let mut bodies = 0;
    for (g, m) in iter_placements(group, base) {
        let mesh = doc.mesh(g.mesh);
        if mesh.faces.is_empty() {
            continue;
        }
        let (true, Some(volume)) = solid_report(mesh) else {
            return None;
        };
        let scale = m.map_or(1.0, |m| m.determinant().abs());
        total += volume * scale * material_in(doc, link, g).density;
        bodies += 1;
    }
    (bodies > 0).then_some(total)
}

/// Every component copy in `groups`, at every depth, that shares
/// `mesh`, with `f` applied.
fn for_copies(groups: &mut [Group], mesh: crate::ids::MeshId, f: &mut dyn FnMut(&mut Group)) {
    for g in groups {
        if g.is_component() && g.mesh == mesh {
            f(g);
        }
        for_copies(&mut g.children, mesh, f);
    }
}

/// Give groups a print material of their own (`None`: they take their
/// part's, or the document's). A component copy's material is its
/// definition's, so every copy takes it; a group nested in a component
/// reaches the other copies of that component too.
pub struct SetPrintMaterialCommand {
    pub groups: Vec<GroupId>,
    pub material: Option<PrintMaterial>,
}

impl Command for SetPrintMaterialCommand {
    fn name(&self) -> &str {
        "SetPrintMaterialCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let material = self.material.clone().filter(PrintMaterial::is_valid);
        for &id in &self.groups {
            let Some(path) = scene.doc.group_path(id) else {
                continue;
            };
            let target = path[path.len() - 1];
            let copies = target.is_component().then_some(target.mesh);
            // The innermost component holding the target, whose copies
            // hold the target's twins.
            let holder = path[..path.len() - 1]
                .iter()
                .rev()
                .find(|g| g.is_component())
                .map(|g| g.id);
            if let Some(g) = scene.doc.find_group_mut(id) {
                g.print_material = material.clone();
            }
            if let Some(mesh) = copies {
                for_copies(&mut scene.doc.groups, mesh, &mut |g| {
                    g.print_material = material.clone();
                });
            }
            if let Some(holder) = holder {
                let doc = &mut scene.doc;
                crate::definitions::share_children(&mut doc.meshes, &mut doc.groups, holder);
            }
        }
        scene.prune_selection();
        scene.version += 1;
        Ok(())
    }
}

/// Change the document's print material, the one every group without its
/// own takes.
pub struct SetDocumentPrintMaterialCommand {
    pub material: PrintMaterial,
}

impl Command for SetDocumentPrintMaterialCommand {
    fn name(&self) -> &str {
        "SetDocumentPrintMaterialCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if self.material.is_valid() {
            scene.doc.print_material = self.material.clone();
        }
        scene.version += 1;
        Ok(())
    }
}
