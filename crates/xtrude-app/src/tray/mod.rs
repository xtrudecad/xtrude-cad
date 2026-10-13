// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The side panels as toolkit-neutral models: what each panel shows,
//! computed from the window, and what its controls do, applied to the
//! window. The egui shell draws them; tests drive them directly.
//!
//! The left dock holds the [`DOCK_PANELS`]: the model tree on top, and
//! under a splitter Properties and, when the user docks it, Layers. The
//! others are drop-down panels on the bars (`window::TOOLBAR_PANELS`).

pub mod colors;
pub mod components;
pub mod dimstyle;
pub mod exploded;
pub mod layers;
pub mod model_tree;
pub mod properties;
pub mod saved_views;
pub mod styles;

use crate::window::{Asker, MainWindow};
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::GroupId;
use xtrude_model::model_tree::{ancestors, parent_map};
use xtrude_model::settings::{self, keys};

/// The side panels' models.
#[derive(Default)]
pub struct Tray {
    /// The left dock's panels.
    pub model_tree: model_tree::ModelTree,
    pub properties: properties::Properties,
    pub layers: layers::Layers,
    /// The drop-down panels on the bars (Layers is one too).
    pub saved_views: saved_views::SavedViews,
    pub colors: colors::Colors,
    pub exploded: exploded::ExplodedView,
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

/// `(key, title)` of the left dock's panels, top to bottom: the model
/// tree, then under the splitter Properties and Layers.
pub const DOCK_PANELS: [(&str, &str); 3] = [
    ("model", "Model"),
    ("properties", "Properties"),
    ("layers", "Layers"),
];

/// The dock panels hidden on a first start, and after Reset panel layout.
const FACTORY_HIDDEN: [&str; 1] = ["layers"];

impl Tray {
    /// Bring every panel up to date.
    pub fn refresh(&mut self, w: &MainWindow) {
        self.properties.refresh(w);
        self.model_tree.refresh(w);
        self.layers.refresh(w);
        self.saved_views.refresh(w);
        self.colors.refresh_in_model(w);
        self.exploded.refresh(w);
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

    /// Whether a dock panel is unfolded: as the user left it last time
    /// (`dock/collapsed/<panel class>`), open on a first start. Keyed by
    /// the panel, not its title.
    pub fn panel_open(key: &str) -> bool {
        !settings::read(&keys::dock_collapsed(panel_class(key)))
    }

    pub fn set_panel_open(key: &str, open: bool) {
        settings::write(&keys::dock_collapsed(panel_class(key)), &!open);
    }

    /// Whether a dock panel is drawn: Model and Properties are, Layers is
    /// not, until a close button or a Window ▸ Panels entry changes it
    /// (`ui/hidden_dock_panels`). A hidden panel keeps its fold.
    pub fn panel_shown(key: &str) -> bool {
        !hidden_panels().contains(&key)
    }

    pub fn set_panel_shown(key: &str, shown: bool) {
        let mut hidden = hidden_panels();
        hidden.retain(|k| *k != key);
        if !shown {
            hidden.extend(panel_key(key));
        }
        let classes: Vec<&str> = hidden.iter().map(|k| panel_class(k)).collect();
        settings::write(&keys::HIDDEN_DOCK_PANELS, &serde_json::json!(classes));
    }

    /// The dock panels drawn, top to bottom.
    pub fn shown_panels() -> Vec<&'static str> {
        DOCK_PANELS
            .iter()
            .map(|(k, _)| *k)
            .filter(|k| Self::panel_shown(k))
            .collect()
    }

    /// The factory layout: Model and Properties shown, Layers hidden. The
    /// folds (`dock/collapsed/<panel class>`) stay as they are.
    pub fn reset_layout() {
        settings::clear(&keys::HIDDEN_DOCK_PANELS);
    }
}

/// The dock panel key in [`DOCK_PANELS`] named `key`.
fn panel_key(key: &str) -> Option<&'static str> {
    DOCK_PANELS.iter().map(|(k, _)| *k).find(|k| *k == key)
}

/// The dock panels the settings hide, each once: the factory ones when
/// nothing is stored; anything but a list of strings reads as none hidden,
/// unknown classes are dropped.
fn hidden_panels() -> Vec<&'static str> {
    let stored = settings::read(&keys::HIDDEN_DOCK_PANELS);
    if stored.is_null() {
        return FACTORY_HIDDEN.to_vec();
    }
    let mut out: Vec<&'static str> = Vec::new();
    if let serde_json::Value::Array(a) = stored {
        for class in a.iter().filter_map(|v| v.as_str()) {
            let found = DOCK_PANELS
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

/// The panel class name a dock panel's settings are stored by.
fn panel_class(key: &str) -> &str {
    match key {
        "model" => "ModelPanel",
        "properties" => "PropertiesPanel",
        "layers" => "LayersPanel",
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
