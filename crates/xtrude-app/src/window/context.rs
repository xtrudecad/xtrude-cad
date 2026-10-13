// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport's right-click menu and its submenus, tailored to what is
//! selected. Built afresh on every right-click; what Repeat remembers is
//! the command, not the entry.

use xtrude_core::Axis;
use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::settings::{self, keys};
use xtrude_model::solids::is_solid;
use xtrude_model::text3d::text_is_pristine;

use super::commands::{is_hidden, split_selection};
use super::MainWindow;

/// One entry of the right-click menu.
#[derive(Clone, Debug, PartialEq)]
pub enum CtxItem {
    Entry {
        id: String,
        text: String,
        enabled: bool,
        checked: Option<bool>,
    },
    Menu(String, Vec<CtxItem>),
    Separator,
}

fn entry(id: &str, text: &str) -> CtxItem {
    CtxItem::Entry {
        id: id.to_string(),
        text: tr(text, &[]),
        enabled: true,
        checked: None,
    }
}

impl CtxItem {
    pub fn text(&self) -> &str {
        match self {
            CtxItem::Entry { text, .. } | CtxItem::Menu(text, _) => text,
            CtxItem::Separator => "",
        }
    }
}

impl MainWindow {
    /// The right-click menu for the current selection.
    pub fn context_menu(&self) -> Vec<CtxItem> {
        let scene = &self.viewport.scene;
        let sel = &scene.selection;
        let s = split_selection(scene);
        let has_group = sel.iter().any(|e| e.is_group());
        let has_mesh = sel.iter().any(|e| e.is_edge() || e.is_face());
        let has_face = sel.iter().any(|e| e.is_face());
        let lone_group = sel.len() == 1 && has_group;
        let planes = self.selected_section_planes();
        let mut menu = Vec::new();

        // AutoCAD's first right-click entry.
        if let Some((label, _)) = &self.last_command {
            menu.push(CtxItem::Entry {
                id: "repeat_last_command".into(),
                text: tr("Repeat {name}", &[("name", &tr(label, &[]))]),
                enabled: true,
                checked: None,
            });
            menu.push(CtxItem::Separator);
        }
        if let Some(&first) = planes.first() {
            let active = scene
                .doc
                .section_planes
                .iter()
                .any(|p| p.id == first && p.active);
            menu.push(entry("section:reverse", "Reverse"));
            menu.push(CtxItem::Entry {
                id: "section:active".into(),
                text: tr("Active Cut", &[]),
                enabled: true,
                checked: Some(active),
            });
            menu.push(entry("section:align", "Align View"));
            menu.push(entry(&format!("section:name:{first}"), "Name and symbol…"));
            menu.push(CtxItem::Separator);
        }
        if (has_mesh || has_group) && !lone_group {
            menu.push(entry("make_group", "Make Group"));
        }
        if has_mesh {
            menu.push(entry("make_component", "Make Component…"));
        }
        if has_face {
            menu.push(entry("reverse_faces", "Reverse Faces"));
            menu.push(entry("orient_faces", "Orient Faces"));
        }
        if has_mesh || has_group {
            menu.push(CtxItem::Menu(
                tr("Flip Along", &[]),
                vec![
                    entry("flip_along:x", "Red axis"),
                    entry("flip_along:y", "Green axis"),
                    entry("flip_along:z", "Blue axis"),
                ],
            ));
        }
        let loose_edges: Vec<&EntityRef> = sel.iter().filter(|e| e.is_edge()).collect();
        let mid = scene.active_mesh_id();
        if !loose_edges.is_empty()
            && loose_edges.iter().all(
                |e| matches!(e, EntityRef::Edge(m, x) if *m == mid && scene.mesh().has_edge(*x)),
            )
        {
            menu.push(entry("divide", "Divide…"));
        }
        if !self.hideable().is_empty() {
            menu.push(entry("hide", "Hide"));
        }
        if sel
            .iter()
            .any(|e| scene.entity_hidden(e) || is_hidden(scene, e))
        {
            menu.push(entry("unhide_selected", "Unhide"));
        }
        if let Some(layer) = self.layer_submenu() {
            menu.push(layer);
        }
        if let Some(material) = self.print_material_submenu() {
            menu.push(material);
        }
        if let Some(select) = self.select_submenu() {
            menu.push(select);
        }
        if has_group {
            let groups = &s.groups;
            if groups.len() == 1 {
                menu.push(entry(&format!("edit_group:{}", groups[0]), "Edit Group"));
            }
            if let Some(text) = self.selected_text3d() {
                let pristine = scene
                    .doc
                    .find_group(text)
                    .is_some_and(|g| text_is_pristine(&scene.doc.meshes, g));
                if pristine {
                    menu.push(entry(&format!("edit_text3d:{text}"), "Edit 3D Text…"));
                } else {
                    menu.push(CtxItem::Entry {
                        id: "edit_text3d:off".into(),
                        text: tr("Edit 3D Text… (letters edited by hand)", &[]),
                        enabled: false,
                        checked: None,
                    });
                }
            }
            let any_plain = groups
                .iter()
                .any(|g| scene.doc.find_group(*g).is_some_and(|g| !g.is_component()));
            if any_plain || groups.len() >= 2 {
                menu.push(entry("make_component", "Make Component…"));
            }
            menu.push(entry("explode_group", "Explode Group"));
            if groups.len() == 1 {
                menu.push(entry("split_into_pieces", "Split into Pieces"));
            }
            if groups.len() >= 2 {
                menu.push(entry("merge_groups", "Merge Groups"));
            }
            if groups
                .iter()
                .any(|g| scene.doc.find_group(*g).is_some_and(|g| g.is_component()))
            {
                menu.push(entry("make_independent", "Make Independent"));
            }
            if groups.len() == 1 && sel.len() == 1 {
                menu.push(entry(&format!("change_axes:{}", groups[0]), "Change Axes"));
            }
            let solid: Vec<_> = groups
                .iter()
                .filter(|g| {
                    scene
                        .doc
                        .find_group(**g)
                        .is_some_and(|g| is_solid(&scene.doc.meshes, g))
                })
                .collect();
            if solid.len() >= 2 && solid.len() == groups.len() {
                menu.push(entry("merge_solids", "Merge Solids"));
                let mut keys = vec!["solid_intersect", "solid_union"];
                if solid.len() == 2 {
                    keys.push("solid_split");
                }
                menu.push(CtxItem::Menu(
                    tr("Solids", &[]),
                    keys.iter()
                        .map(|k| {
                            let name = super::make_tool(k).map(|t| t.info().name).unwrap_or(k);
                            entry(k, name)
                        })
                        .collect(),
                ));
            }
        }
        if has_face || has_group {
            menu.push(CtxItem::Menu(
                tr("Add Crossing Edges", &[]),
                vec![
                    entry("intersect:model", "With Everything"),
                    entry("intersect:selection", "Within Selection"),
                    entry("intersect:context", "Within Open Group"),
                ],
            ));
        }
        if has_mesh || has_group {
            menu.push(entry("cut", "Cut"));
            menu.push(entry("copy", "Copy"));
        }
        if !sel.is_empty() {
            menu.push(entry("zoom_selection", "Zoom Selection"));
            menu.push(entry("delete_selection", "Delete"));
            menu.push(entry("clear_selection", "Clear selection"));
            menu.push(CtxItem::Separator);
        }
        if self.viewport.clipboard.is_some()
            || super::clipboard::available(self.system_clipboard.as_ref())
        {
            menu.push(entry("paste", "Paste"));
            menu.push(entry("paste_in_place", "Paste in Place"));
        }
        menu.push(entry("zoom_extents", "Zoom Extents"));
        menu.push(CtxItem::Separator);
        menu.push(CtxItem::Entry {
            id: "undo".into(),
            text: tr("Undo", &[]),
            enabled: self.viewport.history.can_undo(),
            checked: None,
        });
        menu.push(CtxItem::Entry {
            id: "redo".into(),
            text: tr("Redo", &[]),
            enabled: self.viewport.history.can_redo(),
            checked: None,
        });
        menu
    }

    /// Right-click ▸ Select: grow the selection.
    fn select_submenu(&self) -> Option<CtxItem> {
        let scene = &self.viewport.scene;
        let sel = &scene.selection;
        let has_mesh = sel.iter().any(|e| e.is_edge() || e.is_face());
        let has_face = sel.iter().any(|e| e.is_face());
        let has_group = sel.iter().any(|e| e.is_group());
        if !(has_mesh || has_group) {
            return None;
        }
        let mut sub = Vec::new();
        if has_mesh {
            sub.push(entry("select:all_connected", "All Connected"));
        }
        if has_face {
            sub.push(entry("select:bounding_edges", "Bounding Edges"));
        }
        let painted_group = sel.iter().any(|e| match e {
            EntityRef::Group(g) => scene
                .doc
                .find_group(*g)
                .is_some_and(|g| g.material.is_some()),
            _ => false,
        });
        if has_face || painted_group {
            sub.push(entry("select:same_color", "All with Same Color"));
        }
        sub.push(entry("select:same_layer", "All on Same Layer"));
        Some(CtxItem::Menu(tr("Select", &[]), sub))
    }

    /// Right-click ▸ Layer ▸ the document's layers, the selection's own
    /// ticked, plus «New layer…».
    fn layer_submenu(&self) -> Option<CtxItem> {
        let scene = &self.viewport.scene;
        let targets: Vec<&EntityRef> = scene
            .selection
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EntityRef::Face(..)
                        | EntityRef::Edge(..)
                        | EntityRef::Group(_)
                        | EntityRef::Dimension(_)
                        | EntityRef::TextLabel(_)
                )
            })
            .collect();
        if targets.is_empty() {
            return None;
        }
        let current: std::collections::BTreeSet<String> = targets
            .iter()
            .map(|e| xtrude_model::commands::layer_of(scene, e))
            .collect();
        let mut sub: Vec<CtxItem> = scene
            .doc
            .layers
            .iter()
            .map(|ly| CtxItem::Entry {
                id: format!("layer:assign:{}", ly.name),
                text: ly.name.clone(),
                enabled: true,
                checked: Some(current.len() == 1 && current.contains(&ly.name)),
            })
            .collect();
        sub.push(CtxItem::Separator);
        sub.push(entry("layer:new", "New layer…"));
        Some(CtxItem::Menu(tr("Layer", &[]), sub))
    }

    /// The context-menu-only actions (parametrised ids).
    pub(super) fn dispatch_context(&mut self, id: &str) -> bool {
        if let Some(a) = id.strip_prefix("flip_along:") {
            if let Some(c) = a.chars().next() {
                // Any other letter has always meant blue.
                self.on_flip_along(Axis::from_char(c).unwrap_or(Axis::Z));
            }
            return true;
        }
        if let Some(k) = id.strip_prefix("select:") {
            self.grow_selection(k);
            return true;
        }
        if let Some(name) = id.strip_prefix("layer:assign:") {
            let name = name.to_string();
            self.assign_layer(&name);
            return true;
        }
        if id == "layer:new" {
            self.assign_new_layer();
            return true;
        }
        if let Some(g) = id.strip_prefix("edit_group:").and_then(|g| g.parse().ok()) {
            self.viewport.begin_group_edit(g);
            return true;
        }
        if let Some(g) = id.strip_prefix("edit_text3d:") {
            self.on_edit_3d_text(g.parse().ok());
            return true;
        }
        if let Some(g) = id.strip_prefix("change_axes:").and_then(|g| g.parse().ok()) {
            self.on_change_axes(g);
            return true;
        }
        if let Some(p) = id
            .strip_prefix("section:name:")
            .and_then(|p| p.parse().ok())
        {
            // asked for again, whatever «Don't ask again» said
            let ask = settings::read(&keys::ASK_SECTION_NAME);
            settings::write(&keys::ASK_SECTION_NAME, &true);
            self.prompt_section_name(p);
            if !ask {
                settings::write(&keys::ASK_SECTION_NAME, &false);
            }
            return true;
        }
        match id {
            "section:reverse" => self.on_reverse_section(),
            "section:active" => self.on_toggle_active_section(),
            "section:align" => self.on_align_view_to_section(),
            "clear_selection" => {
                self.viewport.scene.clear_selection();
                self.viewport.update();
            }
            _ => return false,
        }
        true
    }
}
