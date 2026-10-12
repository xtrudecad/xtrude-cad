// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Eraser tool: erase by clicking or by dragging over geometry.
//!
//! Press marks the edge under the cursor, dragging keeps marking everything
//! the cursor sweeps over (shown red), and release erases the whole stroke
//! as ONE undo step. Erasing an edge takes its faces with it; a segment of
//! a drawn curve erases its whole contour. Guides, dimensions and leader
//! texts are erased too; a group or component under the cursor goes
//! WHOLE. Shift held at press starts a HIDE stroke instead: swept
//! edges and objects are hidden, not erased. Esc cancels the stroke.

use glam::DVec3;
use xtrude_model::commands::{
    DeleteDimensionsCommand, DeleteGroupCommand, DeleteGuidesCommand, DeleteTextLabelsCommand,
    EraseSelectionCommand, HideCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::oriented_box_corners;
use xtrude_model::history::{Command, CompoundCommand};

use super::{Rgba, SnapPrefs, Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::Viewport;

#[derive(Default)]
pub struct EraserTool {
    pub stroke: bool,
    /// Shift at press: hide the stroke, don't erase.
    pub hide: bool,
    /// What the stroke marked, in the order it was swept.
    pub marked: Vec<EntityRef>,
}

impl EraserTool {
    pub fn new() -> EraserTool {
        EraserTool::default()
    }

    fn reset(&mut self) {
        self.stroke = false;
        self.hide = false;
        self.marked.clear();
    }

    fn add(&mut self, e: EntityRef) {
        if !self.marked.contains(&e) {
            self.marked.push(e);
        }
    }

    /// Mark whatever is under the cursor.
    pub fn mark(&mut self, vp: &mut Viewport, x: f64, y: f64) {
        if !self.hide {
            // The text block overdraws all geometry, so the glyphs outrank
            // every 3D pick — the same order as Select.
            if let Some(l) = vp.pick_text_label(x, y, true) {
                self.add(EntityRef::TextLabel(l));
                return;
            }
        }
        if let Some((mid, e)) = vp.pick_edge(x, y).and_then(|p| p.edge) {
            // A curve is one entity: marking a segment marks its contour.
            let mesh = vp.scene.doc.mesh(mid);
            if mesh.has_edge(e) {
                for c in mesh.curve_edges(e) {
                    self.add(EntityRef::Edge(mid, c));
                }
            }
            return;
        }
        // A group or component is one entity too: the eraser takes it
        // whole. From inside a group its own content is the loose
        // part and pick_group answers None.
        if let Some(g) = vp.pick_group(x, y) {
            self.add(EntityRef::Group(g));
            return;
        }
        if self.hide {
            return; // only edges and objects can hide
        }
        if let Some(g) = vp.pick_guide(x, y) {
            self.add(EntityRef::Guide(g));
            return;
        }
        if let Some(d) = vp.pick_dimension(x, y) {
            self.add(EntityRef::Dimension(d));
            return;
        }
        // The label's thin leader keeps the normal, post-edge priority.
        if let Some(l) = vp.pick_text_label(x, y, false) {
            self.add(EntityRef::TextLabel(l));
        }
    }

    /// The twelve edges of the group's oriented box — its stroke mark.
    fn group_box_segments(vp: &Viewport, g: xtrude_model::ids::GroupId) -> Vec<(DVec3, DVec3)> {
        let Some((frame, lo, hi)) = vp.group_obb(g) else {
            return Vec::new();
        };
        let c = oriented_box_corners(frame, lo, hi);
        if c.len() != 8 {
            return Vec::new();
        }
        const PAIRS: [(usize, usize); 12] = [
            (0, 1),
            (1, 3),
            (3, 2),
            (2, 0),
            (4, 5),
            (5, 7),
            (7, 6),
            (6, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        PAIRS.iter().map(|&(i, j)| (c[i], c[j])).collect()
    }

    fn run(vp: &mut Viewport, mut cmds: Vec<Box<dyn Command>>) {
        match cmds.len() {
            0 => {}
            1 => {
                let c = cmds.pop().expect("one command");
                vp.execute_boxed(c);
            }
            _ => {
                vp.execute(&mut CompoundCommand::new(cmds));
            }
        }
    }
}

impl Tool for EraserTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("eraser"),
            shortcut: Some("E"),
            description: Some(
                "Click or drag over edges to erase them, together with the faces they bound.",
            ),
            ..ToolInfo::new("eraser", "Eraser")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    /// Stroke marks show red.
    fn wireframe_color(&self) -> Option<Rgba> {
        Some([0.90, 0.20, 0.15, 1.0])
    }
    tool_any!();

    fn on_activate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.reset();
    }

    /// Press: start a stroke and mark whatever is under the cursor. The
    /// modifier latches here — a whole stroke is one gesture.
    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.stroke = true;
        self.hide = ctx.modifiers.shift;
        self.mark(vp, ctx.screen.0, ctx.screen.1);
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (x, y) = ctx.screen;
        if self.stroke {
            self.mark(vp, x, y);
        } else {
            // Preview what a click would take; a text's glyphs first.
            let hover = match vp.pick_text_label(x, y, true) {
                Some(l) => Some(EntityRef::TextLabel(l)),
                None => vp
                    .pick_edge(x, y)
                    .and_then(|p| p.edge)
                    .map(|(m, e)| EntityRef::Edge(m, e)),
            };
            vp.set_hover(hover);
        }
        vp.update();
    }

    /// Release: erase everything the stroke marked, as one undo step.
    fn on_release(&mut self, vp: &mut Viewport) {
        if !self.stroke {
            return;
        }
        let marked = std::mem::take(&mut self.marked);
        self.stroke = false;
        if marked.is_empty() {
            vp.update();
            return;
        }
        let mid = vp.scene.active_mesh_id();
        let edges: Vec<_> = marked
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Edge(m, x) if m == mid => Some(x),
                _ => None,
            })
            .collect();
        let groups: Vec<_> = marked
            .iter()
            .filter_map(|e| match *e {
                EntityRef::Group(g) => Some(g),
                _ => None,
            })
            .collect();
        let mut cmds: Vec<Box<dyn Command>> = Vec::new();
        if self.hide {
            let mesh = vp.scene.mesh();
            let edges: Vec<EntityRef> = edges
                .into_iter()
                .filter(|&e| mesh.has_edge(e) && !mesh.edge(e).hidden)
                .map(|e| EntityRef::Edge(mid, e))
                .collect();
            let groups: Vec<EntityRef> = groups
                .into_iter()
                .filter(|&g| vp.scene.doc.find_group(g).is_some_and(|g| !g.hidden))
                .map(EntityRef::Group)
                .collect();
            if !edges.is_empty() {
                cmds.push(Box::new(HideCommand {
                    entities: edges,
                    hidden: true,
                }));
            }
            if !groups.is_empty() {
                cmds.push(Box::new(HideCommand {
                    entities: groups,
                    hidden: true,
                }));
            }
            Self::run(vp, cmds);
            vp.update();
            return;
        }
        let pick = |f: fn(&EntityRef) -> Option<xtrude_model::ids::ObjId>| {
            marked.iter().filter_map(f).collect::<Vec<_>>()
        };
        let guides = pick(|e| match *e {
            EntityRef::Guide(g) => Some(g),
            _ => None,
        });
        let dims = pick(|e| match *e {
            EntityRef::Dimension(d) => Some(d),
            _ => None,
        });
        let labels = pick(|e| match *e {
            EntityRef::TextLabel(l) => Some(l),
            _ => None,
        });
        if !edges.is_empty() {
            cmds.push(Box::new(EraseSelectionCommand::from_entities(
                vp.scene.mesh(),
                &edges,
                &[],
            )));
        }
        for g in groups {
            cmds.push(Box::new(DeleteGroupCommand { group: g }));
        }
        if !guides.is_empty() {
            cmds.push(Box::new(DeleteGuidesCommand { ids: guides }));
        }
        if !dims.is_empty() {
            cmds.push(Box::new(DeleteDimensionsCommand { ids: dims }));
        }
        if !labels.is_empty() {
            cmds.push(Box::new(DeleteTextLabelsCommand { ids: labels }));
        }
        Self::run(vp, cmds);
        vp.update();
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.reset();
        vp.update();
    }

    /// The marked stroke, drawn in the tool's red wireframe colour.
    fn rubber_band_lines(&self, vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        let doc = &vp.scene.doc;
        let mut segs = Vec::new();
        for m in &self.marked {
            match *m {
                EntityRef::Edge(mid, e) => {
                    let mesh = doc.mesh(mid);
                    if mesh.has_edge(e) {
                        segs.push(mesh.edge_ab(e));
                    }
                }
                EntityRef::Group(g) => segs.extend(Self::group_box_segments(vp, g)),
                EntityRef::Guide(g) => {
                    if let Some(g) = doc.guides.iter().find(|x| x.id == g) {
                        segs.push(g.segment());
                    }
                }
                EntityRef::Dimension(d) => {
                    if let Some(d) = doc.dimension(d) {
                        segs.push(d.line_points(&doc.doc_view(vp.scene.active_mesh_id())));
                    }
                }
                EntityRef::TextLabel(l) => {
                    if let Some(l) = doc.text_labels.iter().find(|x| x.id == l) {
                        segs.push((l.anchor, l.position()));
                    }
                }
                _ => {}
            }
        }
        segs
    }
}
