// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Stacked toolbar buttons: one button that holds several related actions.
//! It shows one of them (its current member), a click runs that one, and
//! its arrow lists them all. A bar names a stack as `stack:<id>` among its
//! action ids. No egui types: the shell draws the button and the list.
//!
//! The rules:
//! - the current member is the last one run by any route (the button's
//!   list, a shortcut, a menu, Repeat), or the first member;
//! - so a stack of tools always shows the tool in use;
//! - the current members are saved (`ui/toolbar_stacks`) on every change;
//!   an unknown stack or action in the setting is ignored.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use xtrude_model::settings::{self, keys};

/// The prefix of a stack in a bar's action ids.
pub const STACK_PREFIX: &str = "stack:";

/// The stacks: `(id, title, member action ids)`, the first member current
/// by default. An action is a member of one stack at most.
pub const TOOLBAR_STACKS: [(&str, &str, &[&str]); 10] = [
    ("paint", "Paint", &["paint", "eyedropper"]),
    ("lines", "Lines", &["line", "freehand"]),
    (
        "rectangles",
        "Rectangles",
        &["rectangle", "rectangle_center", "rotated_rect"],
    ),
    (
        "circles",
        "Circles, Polygons and Slots",
        &["circle", "polygon", "slot"],
    ),
    ("arcs", "Arcs", &["arc", "arc3", "center_arc", "sector"]),
    ("edges", "Fillet and Chamfer", &["fillet", "chamfer"]),
    (
        "patterns",
        "Patterns",
        &["pattern_linear", "pattern_circular"],
    ),
    (
        "booleans",
        "Solid Booleans",
        &[
            "solid_union",
            "solid_subtract",
            "solid_trim",
            "solid_intersect",
        ],
    ),
    (
        "nav",
        "Navigation",
        &["nav:orbit", "nav:pan", "nav:zoom", "nav:zoom_window"],
    ),
    (
        "views",
        "Standard Views",
        &[
            "view:iso",
            "view:top",
            "view:front",
            "view:right",
            "view:left",
            "view:back",
            "view:bottom",
        ],
    ),
];

/// The members of stack `id`.
pub fn members(id: &str) -> Option<&'static [&'static str]> {
    TOOLBAR_STACKS
        .iter()
        .find(|(s, _, _)| *s == id)
        .map(|(_, _, m)| *m)
}

/// The title of stack `id`.
pub fn title(id: &str) -> Option<&'static str> {
    TOOLBAR_STACKS
        .iter()
        .find(|(s, _, _)| *s == id)
        .map(|(_, t, _)| *t)
}

/// The stack `action` is a member of.
pub fn stack_of(action: &str) -> Option<&'static str> {
    TOOLBAR_STACKS
        .iter()
        .find(|(_, _, m)| m.contains(&action))
        .map(|(s, _, _)| *s)
}

/// A bar's action ids with its stack members folded: the first member of
/// a stack becomes `stack:<id>`, the others drop out.
pub fn fold(ids: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids {
        let entry = match stack_of(&id) {
            Some(s) => format!("{STACK_PREFIX}{s}"),
            None => id,
        };
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// The current member of each stack that has left its first one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolbarStacks {
    current: BTreeMap<String, String>,
}

impl ToolbarStacks {
    /// The saved current members, without the unknown ones.
    pub fn load() -> ToolbarStacks {
        let mut current = BTreeMap::new();
        if let Some(Value::Object(m)) = settings::stored(&keys::TOOLBAR_STACKS) {
            for (id, action) in m {
                let Value::String(action) = action else {
                    continue;
                };
                if members(&id).is_some_and(|ms| ms.contains(&action.as_str())) {
                    current.insert(id, action);
                }
            }
        }
        ToolbarStacks { current }
    }

    fn save(&self) {
        let m: Map<String, Value> = self
            .current
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        settings::write(&keys::TOOLBAR_STACKS, &Value::Object(m));
        settings::sync();
    }

    /// The member stack `id` shows.
    pub fn current(&self, id: &str) -> Option<&str> {
        let ms = members(id)?;
        Some(
            self.current
                .get(id)
                .map(String::as_str)
                .unwrap_or(ms.first().copied().unwrap_or_default()),
        )
    }

    /// `action` was run: its stack, if any, shows it from now on.
    pub fn follow(&mut self, action: &str) {
        let Some(id) = stack_of(action) else {
            return;
        };
        if self.current(id) != Some(action) {
            self.current.insert(id.to_string(), action.to_string());
            self.save();
        }
    }
}
