// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Saved views (`SavedViews`): named camera + layer-visibility snapshots —
//! double-click recalls one; add, update from the view, delete.

use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::ids::ObjId;
use xtrude_model::saved_views::SavedView;

use crate::window::MainWindow;

#[derive(Default)]
pub struct SavedViews {
    /// `(id, name)` per saved view.
    pub rows: Vec<(ObjId, String)>,
    pub current: Option<ObjId>,
}

impl SavedViews {
    pub fn refresh(&mut self, w: &MainWindow) {
        self.rows = w
            .viewport
            .scene
            .saved_views
            .iter()
            .map(|v| (v.id, v.name.clone()))
            .collect();
        if self
            .current
            .is_some_and(|c| !self.rows.iter().any(|r| r.0 == c))
        {
            self.current = None;
        }
    }

    fn touch(w: &mut MainWindow) {
        w.viewport.scene.version += 1;
        w.viewport.update();
        w.pump_events();
    }

    /// Double-click: recall the view.
    pub fn activate(&mut self, w: &mut MainWindow, id: ObjId) {
        let Some(view) = w
            .viewport
            .scene
            .saved_views
            .iter()
            .find(|v| v.id == id)
            .cloned()
        else {
            return;
        };
        let vp = &mut w.viewport;
        view.apply(&mut vp.scene, &mut vp.camera);
        // the style and the projection both come with the view
        w.sync_style_menu();
        let scene = &mut w.viewport.scene;
        let dead: Vec<EntityRef> = scene
            .selection
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EntityRef::Face(..) | EntityRef::Edge(..) | EntityRef::Group(_)
                )
            })
            .filter(|e| !scene.entity_selectable(e))
            .copied()
            .collect();
        for e in dead {
            scene.discard(&e);
        }
        Self::touch(w);
        w.show_message(&tr("View '{name}'", &[("name", &view.name)]), 2000);
    }

    pub fn rename(&mut self, w: &mut MainWindow, id: ObjId, text: &str) {
        let name = text.trim();
        if let Some(v) = w.viewport.scene.saved_views.iter_mut().find(|v| v.id == id) {
            if !name.is_empty() {
                v.name = name.to_string();
            }
        }
        self.refresh(w);
        Self::touch(w);
    }

    /// «+ View»: the current view and layer visibility, «View N».
    pub fn add(&mut self, w: &mut MainWindow) {
        let base = tr("View", &[]);
        let taken: Vec<String> = w
            .viewport
            .scene
            .saved_views
            .iter()
            .map(|v| v.name.clone())
            .collect();
        let mut n = 1;
        while taken.contains(&format!("{base} {n}")) {
            n += 1;
        }
        let v = SavedView::capture(
            &format!("{base} {n}"),
            &w.viewport.scene,
            &w.viewport.camera,
        );
        w.viewport.scene.saved_views.push(v);
        self.refresh(w);
        Self::touch(w);
    }

    /// «Update»: the selected saved view from the current view.
    pub fn update(&mut self, w: &mut MainWindow) {
        let Some(id) = self.current else { return };
        let vp = &mut w.viewport;
        let Some(v) = vp.scene.saved_views.iter().position(|v| v.id == id) else {
            return;
        };
        let cam = vp.camera.clone();
        let name = vp.scene.saved_views[v].name.clone();
        let fresh = SavedView::capture(&name, &vp.scene, &cam);
        vp.scene.saved_views[v] = SavedView { id, ..fresh };
        Self::touch(w);
        w.show_message(&tr("View '{name}' updated", &[("name", &name)]), 2000);
    }

    pub fn delete(&mut self, w: &mut MainWindow) {
        let Some(id) = self.current else { return };
        w.viewport.scene.saved_views.retain(|v| v.id != id);
        self.current = None;
        self.refresh(w);
        Self::touch(w);
    }
}
