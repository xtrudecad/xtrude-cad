// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The side tray's panels as toolkit-neutral models: what each panel
//! shows, computed from the window, and what its controls do, applied to
//! the window. The egui shell draws them; tests drive them directly.

pub mod colors;
pub mod components;
pub mod dimstyle;
pub mod layers;
pub mod model_tree;
pub mod parts;
pub mod properties;
pub mod saved_views;
pub mod styles;

use crate::window::{Asker, MainWindow};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::GroupId;
use xtrude_model::model_tree::{ancestors, parent_map};
use xtrude_model::settings::{self, keys};

/// The Properties tray (`Tray`): its sections, top to bottom.
#[derive(Default)]
pub struct Tray {
    pub properties: properties::Properties,
    pub model_tree: model_tree::ModelTree,
    pub layers: layers::Layers,
    pub saved_views: saved_views::SavedViews,
    pub colors: colors::Colors,
    pub components: components::Components,
    pub parts: parts::Parts,
    /// The on-demand panels (toolbar drop-downs).
    pub styles: styles::Styles,
    pub dimstyle: dimstyle::DimensionStyle,
}

/// A panel control that asks a question: run through [`Tray::run`], so
/// that the answer comes back to it ([`Asker::Tray`]).
#[derive(Clone, Debug, PartialEq)]
pub enum TrayAction {
    /// Layers › Purge.
    PurgeLayers,
    /// Colors › + Color….
    AddColor,
    /// Colors › right-click › Edit color… on a named colour.
    EditNamedColor { name: String, current: [f64; 3] },
    /// Colors › Purge.
    PurgeColors,
    /// Styles › a colour swatch: `attr` is the style's colour, `title` the
    /// picker's.
    StyleColor { title: String, attr: String },
    /// Styles › Save style….
    SaveStyle,
    /// Styles › Delete.
    DeleteStyle,
    /// Dimension style › Color.
    DimensionColor,
}

/// `(key, title)` of the tray's sections, in order.
pub const SECTIONS: [(&str, &str); 7] = [
    ("properties", "Properties"),
    ("model_tree", "Model tree"),
    ("layers", "Layers"),
    ("saved_views", "Saved views"),
    ("colors", "Colors"),
    ("components", "Components"),
    ("parts", "Parts"),
];

impl Tray {
    /// Bring every panel up to date.
    pub fn refresh(&mut self, w: &MainWindow) {
        self.properties.refresh(w);
        self.model_tree.refresh(w);
        self.layers.refresh(w);
        self.saved_views.refresh(w);
        self.colors.refresh_in_model(w);
        self.components.refresh_in_model(w);
        self.parts.refresh(w);
        self.styles.refresh(w);
    }

    /// Run a control that asks a question, as the asker of that question.
    pub fn run(&mut self, w: &mut MainWindow, action: TrayAction) {
        w.ask_as(Asker::Tray(action.clone()), |w| match action {
            TrayAction::PurgeLayers => self.layers.purge(w),
            TrayAction::AddColor => self.colors.add_color(w),
            TrayAction::EditNamedColor { name, current } => {
                self.colors.edit_named_color(w, &name, current)
            }
            TrayAction::PurgeColors => self.colors.purge(w),
            TrayAction::StyleColor { title, attr } => self.styles.pick_color(w, &title, &attr),
            TrayAction::SaveStyle => self.styles.save(w),
            TrayAction::DeleteStyle => self.styles.delete(w),
            TrayAction::DimensionColor => dimstyle::DimensionStyle::pick_color(w),
        });
    }

    /// Whether a section is open: as the user left it last time
    /// (`tray/collapsed/<panel class>`), open on
    /// a first start. Keyed by the panel, not its title, which changes with
    /// the language.
    pub fn section_open(key: &str) -> bool {
        !settings::read(&keys::tray_collapsed(panel_class(key)))
    }

    pub fn set_section_open(key: &str, open: bool) {
        settings::write(&keys::tray_collapsed(panel_class(key)), &!open);
    }

    /// Whether a section is drawn: every one is, until its close button or
    /// its Window ▸ Panels entry hides it (`ui/hidden_tray_sections`). A
    /// hidden section keeps its fold and its place in the order.
    pub fn section_shown(key: &str) -> bool {
        !stored_sections(&keys::HIDDEN_TRAY_SECTIONS).contains(&key)
    }

    pub fn set_section_shown(key: &str, shown: bool) {
        let mut hidden = stored_sections(&keys::HIDDEN_TRAY_SECTIONS);
        hidden.retain(|k| *k != key);
        if !shown {
            hidden.extend(section_key(key));
        }
        store_sections(&keys::HIDDEN_TRAY_SECTIONS, &hidden);
    }

    /// The section keys top to bottom, hidden ones included: the saved
    /// order (`ui/tray_section_order`) with unknown classes dropped and
    /// the missing sections after it, in their [`SECTIONS`] order.
    pub fn section_order() -> Vec<&'static str> {
        let mut order = stored_sections(&keys::TRAY_SECTION_ORDER);
        for (key, _) in SECTIONS {
            if !order.contains(&key) {
                order.push(key);
            }
        }
        order
    }

    /// Move the section `key` just above `before`, or to the bottom for
    /// `None`, and save the order. The other sections, hidden ones
    /// included, keep their order.
    pub fn move_section(key: &str, before: Option<&str>) {
        let Some(moved) = section_key(key) else {
            return;
        };
        if before == Some(moved) {
            return;
        }
        let mut order = Self::section_order();
        order.retain(|k| *k != moved);
        let at = before
            .and_then(|b| order.iter().position(|k| *k == b))
            .unwrap_or(order.len());
        order.insert(at, moved);
        store_sections(&keys::TRAY_SECTION_ORDER, &order);
    }

    /// The factory layout: every section shown, in the [`SECTIONS`] order.
    /// The folds (`tray/collapsed/<panel class>`) stay as they are.
    pub fn reset_layout() {
        settings::clear(&keys::TRAY_SECTION_ORDER);
        settings::clear(&keys::HIDDEN_TRAY_SECTIONS);
    }
}

/// Where a section dragged by its grip lands among the shown sections,
/// given each one's top and bottom: the index of the first section whose
/// middle is below `y`, or the end.
pub fn drop_index(spans: &[(f32, f32)], y: f32) -> usize {
    spans
        .iter()
        .position(|(top, bottom)| y < (top + bottom) / 2.0)
        .unwrap_or(spans.len())
}

/// The section key in [`SECTIONS`] named `key`.
fn section_key(key: &str) -> Option<&'static str> {
    SECTIONS.iter().map(|(k, _)| *k).find(|k| *k == key)
}

/// A list of panel classes in the settings as section keys, each once:
/// anything but a list of strings reads as empty, unknown classes are
/// dropped.
fn stored_sections(key: &settings::Key<serde_json::Value>) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    if let serde_json::Value::Array(a) = settings::read(key) {
        for class in a.iter().filter_map(|v| v.as_str()) {
            let found = SECTIONS
                .iter()
                .map(|(k, _)| *k)
                .find(|k| panel_class(k) == class);
            if let Some(k) = found.filter(|k| !out.contains(k)) {
                out.push(k);
            }
        }
    }
    out
}

fn store_sections(key: &settings::Key<serde_json::Value>, sections: &[&str]) {
    let classes: Vec<&str> = sections.iter().map(|k| panel_class(k)).collect();
    settings::write(key, &serde_json::json!(classes));
}

/// The panel class name a section's collapsed state is remembered by, as
/// the settings already store it (`keys::tray_collapsed`).
fn panel_class(key: &str) -> &str {
    match key {
        // named after the section's former title, as stored
        "properties" => "EntityInfoPanel",
        // named after the section's former title, as stored
        "model_tree" => "OutlinerPanel",
        "layers" => "LayersPanel",
        // titled Scenes when the settings key was chosen
        "saved_views" => "ScenesPanel",
        // titled Materials when the settings key was chosen
        "colors" => "MaterialsPanel",
        "components" => "ComponentsPanel",
        "parts" => "PartsPanel",
        other => other,
    }
}

/// Make `chain` (outermost first) the open contexts.
fn open_chain(w: &mut MainWindow, chain: &[GroupId]) {
    let vp = &mut w.viewport;
    let stack = |vp: &crate::viewport::Viewport| -> Vec<GroupId> {
        vp.scene.edit_stack.iter().map(|l| l.group).collect()
    };
    let mut s = stack(vp);
    // `while stack and stack != chain[:len(stack)]`: leave the levels
    // that are not part of the chain.
    while !s.is_empty() && (s.len() > chain.len() || s[..] != chain[..s.len()]) {
        vp.end_one_group_edit();
        let next = stack(vp);
        if next.len() >= s.len() {
            break;
        }
        s = next;
    }
    for &g in &chain[s.len()..] {
        vp.begin_group_edit(g);
    }
}

/// Open `group` for editing, its parents first.
fn edit_group(w: &mut MainWindow, group: GroupId) {
    let mut chain = ancestors(&parent_map(&w.viewport.scene), group);
    chain.push(group);
    open_chain(w, &chain);
}

/// Select `groups` in the model, opening the first one's parents (a group
/// is selectable only from inside its parent).
fn select_groups(w: &mut MainWindow, groups: &[GroupId]) {
    let chain = groups
        .first()
        .map(|g| ancestors(&parent_map(&w.viewport.scene), *g))
        .unwrap_or_default();
    open_chain(w, &chain);
    let scene = &mut w.viewport.scene;
    scene.selection.clear();
    for g in groups {
        scene.selection.insert(EntityRef::Group(*g));
    }
    scene.bump_view();
    w.viewport.update();
    w.pump_events();
}

/// Natural sort key: «leg-2» before «leg-10».
pub fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                out.push((1, 0, std::mem::take(&mut text).to_lowercase()));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
                digits.clear();
            }
            text.push(c);
        }
    }
    if !digits.is_empty() {
        out.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !text.is_empty() {
        out.push((1, 0, text.to_lowercase()));
    }
    out
}
