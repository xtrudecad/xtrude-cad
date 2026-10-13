// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Print materials in the window: the group context menu's Print material
//! submenu (the built-ins, Custom…, Use default) and Model ▸ Document
//! print material…, which sets the material every group without its own
//! takes.

use xtrude_model::i18n::tr;
use xtrude_model::print_material::{
    print_material_of, PrintMaterial, SetDocumentPrintMaterialCommand, SetPrintMaterialCommand,
    BUILT_IN,
};
use xtrude_model::units::{density_scale, density_unit};

use super::commands::split_selection;
use super::context::CtxItem;
use super::MainWindow;

/// The action of Model ▸ Document print material….
pub const DOCUMENT_ACTION: &str = "document_print_material";
/// The context menu entries' ids: `print_material:<index in BUILT_IN>`,
/// `print_material:custom` and `print_material:default`.
const PREFIX: &str = "print_material:";

/// The densities a custom material may have, in kg/m³: lighter than any
/// foam filament and heavier than any metal-filled one are typing errors.
const DENSITY_RANGE: (f64, f64) = (10.0, 25_000.0);

impl MainWindow {
    /// Right-click ▸ Print material, for selected groups: the built-ins,
    /// the selection's own ticked when they all have the same, Custom…
    /// and Use default.
    pub(crate) fn print_material_submenu(&self) -> Option<CtxItem> {
        let scene = &self.viewport.scene;
        let groups = split_selection(scene).groups;
        if groups.is_empty() {
            return None;
        }
        let own: Vec<Option<&PrintMaterial>> = groups
            .iter()
            .map(|&g| {
                scene
                    .doc
                    .find_group(g)
                    .and_then(|g| g.print_material.as_ref())
            })
            .collect();
        let common = own.windows(2).all(|p| p[0] == p[1]).then(|| own[0]);
        let mut sub: Vec<CtxItem> = BUILT_IN
            .iter()
            .enumerate()
            .map(|(i, &(name, density))| CtxItem::Entry {
                id: format!("{PREFIX}{i}"),
                text: name.to_string(),
                enabled: true,
                checked: Some(common.flatten() == Some(&PrintMaterial::new(name, density))),
            })
            .collect();
        let custom = common.flatten().filter(|m| !m.is_built_in());
        sub.push(CtxItem::Entry {
            id: format!("{PREFIX}custom"),
            text: match custom {
                Some(m) => tr("Custom: {name}…", &[("name", &m.name)]),
                None => tr("Custom…", &[]),
            },
            enabled: true,
            checked: Some(custom.is_some()),
        });
        sub.push(CtxItem::Separator);
        sub.push(CtxItem::Entry {
            id: format!("{PREFIX}default"),
            text: tr(
                "Use default ({name})",
                &[("name", &scene.doc.print_material.name)],
            ),
            enabled: true,
            checked: Some(common == Some(None)),
        });
        Some(CtxItem::Menu(tr("Print material", &[]), sub))
    }

    /// A Print material entry picked: `true` when `id` was one.
    pub(super) fn dispatch_print_material(&mut self, id: &str) -> bool {
        let Some(what) = id.strip_prefix(PREFIX) else {
            return false;
        };
        let groups = split_selection(&self.viewport.scene).groups;
        if groups.is_empty() {
            return true;
        }
        let material = match what {
            "default" => None,
            "custom" => {
                let current = print_material_of(&self.viewport.scene.doc, groups[0]).clone();
                let Some(m) = self.ask_custom_material(&current) else {
                    return true;
                };
                Some(m)
            }
            i => {
                let Some(&(name, density)) = i.parse::<usize>().ok().and_then(|i| BUILT_IN.get(i))
                else {
                    return true;
                };
                Some(PrintMaterial::new(name, density))
            }
        };
        let name = material
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| self.viewport.scene.doc.print_material.name.clone());
        self.viewport
            .execute(&mut SetPrintMaterialCommand { groups, material });
        self.viewport.update();
        self.show_message(&tr("Print material: {name}", &[("name", &name)]), 2500);
        true
    }

    /// Model ▸ Document print material…: a built-in, or a custom one.
    pub(super) fn on_document_print_material(&mut self) {
        let current = self.viewport.scene.doc.print_material.clone();
        let mut items: Vec<String> = BUILT_IN.iter().map(|(n, _)| n.to_string()).collect();
        items.push(tr("Custom…", &[]));
        let at = BUILT_IN
            .iter()
            .position(|&(n, d)| PrintMaterial::new(n, d) == current)
            .unwrap_or(BUILT_IN.len());
        let Some(pick) = self.dialogs.choose(
            &tr("Document print material", &[]),
            &tr(
                "Groups without a print material of their own are printed in:",
                &[],
            ),
            &items,
            at,
        ) else {
            return;
        };
        let material = match BUILT_IN.get(pick) {
            Some(&(name, density)) => PrintMaterial::new(name, density),
            None => match self.ask_custom_material(&current) {
                Some(m) => m,
                None => return,
            },
        };
        if material == current {
            return;
        }
        let name = material.name.clone();
        self.viewport
            .execute(&mut SetDocumentPrintMaterialCommand { material });
        self.viewport.update();
        self.show_message(
            &tr("Document print material: {name}", &[("name", &name)]),
            2500,
        );
    }

    /// A custom material: its name, then its density in the document's
    /// density unit. `None` when cancelled, or while an answer is still to
    /// come.
    fn ask_custom_material(&mut self, current: &PrintMaterial) -> Option<PrintMaterial> {
        let title = tr("Custom print material", &[]);
        let suggested = if current.is_built_in() {
            String::new()
        } else {
            current.name.clone()
        };
        let name = self
            .dialogs
            .get_text(&title, &tr("Name:", &[]), &suggested, false)?;
        let scale = density_scale();
        let decimals = if density_unit() == "g/cm³" { 3 } else { 4 };
        let density = self.dialogs.get_number(
            &title,
            &tr("Density ({unit}):", &[("unit", &density_unit())]),
            current.density / scale,
            DENSITY_RANGE.0 / scale,
            DENSITY_RANGE.1 / scale,
            decimals,
        );
        if self.dialogs.waiting() {
            // the density comes later: the material is set then, with it
            return None;
        }
        let m = PrintMaterial::new(name.trim(), density? * scale);
        m.is_valid().then_some(m)
    }
}
