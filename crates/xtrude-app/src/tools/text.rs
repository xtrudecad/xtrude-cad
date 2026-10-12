// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Text tool: place a leader-text annotation in two clicks — the anchor on the
//! model, then where the label floats — and type the text. The default text
//! describes what was clicked: an edge's length, a face's area, else the
//! point's coordinates.

use glam::DVec3;
use xtrude_model::commands::AddTextLabelCommand;
use xtrude_model::i18n::tr;
use xtrude_model::textlabel::TextLabel;
use xtrude_model::units::{fmt_area, fmt_len};

use super::{Plane, SnapPrefs, Tool, ToolContext, ToolInfo, WorkPlane};
use crate::tool_any;
use crate::viewport::Viewport;

#[derive(Default)]
pub struct TextTool {
    pub anchor: Option<DVec3>,
    pub hover_point: Option<DVec3>,
    pub default_text: String,
    pub work_plane: Option<Plane>,
}

impl TextTool {
    pub fn new() -> TextTool {
        TextTool::default()
    }

    fn reset(&mut self) {
        self.anchor = None;
        self.work_plane = None;
        self.default_text.clear();
    }

    /// The default label: what did the anchor click land on?
    fn describe(vp: &mut Viewport, ctx: &ToolContext) -> String {
        let (x, y) = ctx.screen;
        if let Some(e) = vp.pick_edge(x, y) {
            return fmt_len((e.b - e.a).length());
        }
        if let Some(hit) = vp.pick_face_any(x, y) {
            let mesh = vp.scene.doc.mesh(hit.mesh);
            if mesh.has_face(hit.face) {
                return fmt_area(mesh.face_area(hit.face));
            }
        }
        let p = ctx.world;
        format!("({:.2}, {:.2}, {:.2})", p.x, p.y, p.z)
    }
}

impl Tool for TextTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("text"),
            shortcut: Some("X"),
            description: Some(
                "Place a text with a leader pointing at the model; it suggests the length, \
             area or coordinates of what you click.",
            ),
            ..ToolInfo::new("text", "Text")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs::axis_magnet(true)
    }
    tool_any!();

    fn start_point(&self) -> Option<DVec3> {
        self.anchor
    }
    fn work_plane(&self) -> Option<WorkPlane<'_>> {
        Some(WorkPlane {
            plane: self.work_plane,
            lock: None,
        })
    }
    fn set_work_plane(&mut self, plane: Option<Plane>) {
        self.work_plane = plane;
    }

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
        self.hover_point = None;
    }

    fn on_hover(&mut self, _vp: &mut Viewport, ctx: &ToolContext) {
        self.hover_point = Some(ctx.world);
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(anchor) = self.anchor else {
            self.anchor = Some(ctx.world);
            self.default_text = Self::describe(vp, ctx);
            return;
        };
        // Second click: label position → ask for the text.
        let offset = ctx.world - anchor;
        let text = vp.prompt_text(
            &tr("Text", &[]),
            &tr("Label text:", &[]),
            &self.default_text,
            true,
        );
        if let Some(t) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            vp.execute(&mut AddTextLabelCommand {
                label: Some(TextLabel::new(anchor, offset, &t)),
            });
        }
        self.reset();
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    /// The leader forming.
    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        match (self.anchor, self.hover_point) {
            (Some(a), Some(h)) => vec![(a, h)],
            _ => Vec::new(),
        }
    }

    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        let (_a, h) = (self.anchor?, self.hover_point?);
        let text = if self.default_text.is_empty() {
            "…".to_string()
        } else {
            self.default_text.clone()
        };
        Some((text, Some(h)))
    }
}
