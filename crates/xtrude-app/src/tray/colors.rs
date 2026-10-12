// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Colors section (`MaterialsPanel`): the active paint, the colours
//! (named or not) in this model, the RAL Classic library, «+ Color…»
//! and Purge. It holds colours only, hence its title.

use std::sync::OnceLock;

use serde_json::Value;
use xtrude_model::commands::{PurgeUnusedCommand, RestampMaterialCommand};
use xtrude_model::group::iter_placements;
use xtrude_model::i18n::tr;
use xtrude_model::materials::Material;

use crate::tools::paint::{paint_state, set_paint_state};
use crate::window::MainWindow;

/// One swatch: its colour and the name it paints with (if named).
#[derive(Clone, Debug, PartialEq)]
pub struct Swatch {
    pub rgb: [f64; 3],
    pub name: Option<String>,
    pub tip: String,
}

/// The RAL Classic library (resources/colors/ral.json), in code order.
pub fn ral_colors() -> &'static [Swatch] {
    static RAL: OnceLock<Vec<Swatch>> = OnceLock::new();
    RAL.get_or_init(|| {
        let raw: Value =
            serde_json::from_str(include_str!("../../../../resources/colors/ral.json"))
                .unwrap_or(Value::Null);
        let mut out = Vec::new();
        for fam in raw
            .get("families")
            .and_then(|f| f.as_array())
            .into_iter()
            .flatten()
        {
            for c in fam
                .get("colors")
                .and_then(|c| c.as_array())
                .into_iter()
                .flatten()
            {
                let code = c.get("code").and_then(|v| v.as_str()).unwrap_or_default();
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                let rgb: Vec<f64> = c
                    .get("rgb")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
                    .unwrap_or_default();
                if rgb.len() == 3 {
                    out.push(Swatch {
                        rgb: [rgb[0], rgb[1], rgb[2]],
                        name: Some(format!("{code} {name}")),
                        tip: format!("{code} · {name}"),
                    });
                }
            }
        }
        out
    })
}

#[derive(Default)]
pub struct Colors {
    /// The «In model» swatches.
    pub in_model: Vec<Swatch>,
    /// The library section is open.
    pub library_open: bool,
    /// The library's swatches: built the first time the section is
    /// OPENED, so a closed panel costs nothing.
    pub library: Vec<Swatch>,
}

impl Colors {
    /// The library's one section, «RAL Classic  (213)»: the palette in
    /// code order, without a heading per family. Named after the palette,
    /// not «Colors», which is the tray section's own title.
    pub fn library_title(&self) -> String {
        format!("{}  ({})", tr("RAL Classic", &[]), ral_colors().len())
    }

    /// The library section opened or closed (its swatches built on the
    /// first open).
    pub fn set_library_open(&mut self, open: bool) {
        if open && self.library.is_empty() {
            self.library = ral_colors().to_vec();
        }
        self.library_open = open;
    }

    /// `refresh_in_model`: a swatch per colour in use (faces and group
    /// paints), named when the colour has a name.
    pub fn refresh_in_model(&mut self, w: &MainWindow) {
        let scene = &w.viewport.scene;
        let mut seen: Vec<([i64; 3], Swatch)> = Vec::new();
        let mut take = |color: Option<[f64; 3]>, mat: Option<&str>| {
            let Some(c) = color else { return };
            let key = c.map(|x| (x * 1e6).round() as i64);
            match seen.iter_mut().find(|(k, _)| *k == key) {
                Some((_, s)) => {
                    if s.name.is_none() {
                        s.name = mat.map(String::from);
                    }
                }
                None => seen.push((
                    key,
                    Swatch {
                        rgb: c,
                        name: mat.map(String::from),
                        tip: String::new(),
                    },
                )),
            }
        };
        for (mid, f) in scene.render_faces() {
            let a = &scene.doc.mesh(mid).face(f).attrs;
            take(a.paint.color, a.paint.mat.as_deref());
        }
        for top in &scene.doc.groups {
            for (g, _) in iter_placements(top, None) {
                if let Some(p) = &g.material {
                    take(p.color, p.mat.as_deref());
                }
            }
        }
        self.in_model = seen
            .into_iter()
            .map(|(_, mut s)| {
                s.tip = s.name.clone().unwrap_or_else(|| tr("Color", &[]));
                s
            })
            .collect();
    }

    /// The named colour for a swatch: the registry's when the name
    /// exists, a fresh one otherwise (registered by the first paint).
    fn named_color_for(w: &MainWindow, name: Option<&str>, color: [f64; 3]) -> Option<Material> {
        let name = name?;
        Some(
            w.viewport
                .scene
                .doc
                .materials
                .get(name)
                .cloned()
                .unwrap_or_else(|| Material::new(name, Some(color), None)),
        )
    }

    /// A swatch clicked: paint with it.
    pub fn apply_color(&mut self, w: &mut MainWindow, rgb: [f64; 3], name: Option<&str>) {
        let mut st = paint_state();
        st.is_default = false;
        st.color = rgb;
        st.opacity = None;
        st.named_color = Self::named_color_for(w, name, rgb);
        set_paint_state(st);
        w.activate_tool("paint");
    }

    /// The «Default» swatch: paint the colour off.
    pub fn apply_default(&mut self, w: &mut MainWindow) {
        let mut st = paint_state();
        st.is_default = true;
        st.opacity = None;
        st.named_color = None;
        set_paint_state(st);
        w.activate_tool("paint");
    }

    /// «+ Color…»: a colour, and an optional name.
    pub fn add_color(&mut self, w: &mut MainWindow) {
        let current = paint_state().color;
        let Some(rgb) = w.dialogs.get_color(&tr("Color", &[]), current) else {
            return;
        };
        let name = w.dialogs.get_text(
            &tr("Color", &[]),
            &tr("Color name (optional):", &[]),
            "",
            false,
        );
        if w.dialogs.waiting() {
            // the name comes later: the colour is applied then, with it
            return;
        }
        let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        self.apply_color(w, rgb, name.as_deref());
    }

    /// Right-click ▸ Edit color… on a named colour: restamp every face.
    pub fn edit_named_color(&mut self, w: &mut MainWindow, name: &str, current: [f64; 3]) {
        let existing = w.viewport.scene.doc.materials.get(name).cloned();
        let base = existing.as_ref().and_then(|m| m.color).unwrap_or(current);
        let Some(rgb) = w
            .dialogs
            .get_color(&tr("Edit color: {name}", &[("name", &name)]), base)
        else {
            return;
        };
        let new = Material::new(name, Some(rgb), existing.and_then(|m| m.opacity));
        w.viewport.execute(&mut RestampMaterialCommand {
            name: name.to_string(),
            new,
        });
        w.pump_events();
        self.refresh_in_model(w);
        w.show_message(
            &tr(
                "Color '{name}' updated on every face that wears it",
                &[("name", &name)],
            ),
            3000,
        );
    }

    /// «Purge»: the registry's named colours no face wears.
    pub fn purge(&mut self, w: &mut MainWindow) {
        let mut cmd = PurgeUnusedCommand::new(false, true);
        let (_, names) = cmd.names(&w.viewport.scene);
        if names.is_empty() {
            w.dialogs
                .warn(&tr("Purge", &[]), &tr("Every color is in use.", &[]));
            return;
        }
        let n = names.len();
        let mut preview = names
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if n > 12 {
            preview.push('…');
        }
        let q = format!(
            "{}\n\n{preview}",
            tr("Delete {n} colors no face wears?", &[("n", &n)])
        );
        if !w.dialogs.confirm(&tr("Purge", &[]), &q) {
            return;
        }
        w.viewport.execute(&mut cmd);
        self.refresh_in_model(w);
        w.show_message(&tr("{n} unused colors purged", &[("n", &n)]), 3000);
    }
}
