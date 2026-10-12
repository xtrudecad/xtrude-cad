// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Select tool: pick edges, faces and objects.
//!
//! Shift+click toggles, Ctrl+click adds, Shift+Ctrl+click removes, a plain
//! click replaces; the rubber-band box reads the same modifiers. Hover
//! highlights what the click would pick.

use crate::input::ScreenPt;
use xtrude_core::{EdgeId, FaceId};
use xtrude_model::commands::{
    DeleteDimensionsCommand, DeleteGroupCommand, DeleteGuidesCommand, DeleteSectionPlanesCommand,
    DeleteTextLabelsCommand, EditDimensionTextCommand, EditTextLabelCommand, EraseSelectionCommand,
    SetActiveSectionCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::iter_placements;
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::scene::SelectMode;

use super::{selection_mode, SnapPrefs, Tool, ToolContext, ToolInfo};
use crate::input::{Key, Modifiers};
use crate::tool_any;
use crate::viewport::Viewport;

fn pt_in_rect(p: (f64, f64), r: (f64, f64, f64, f64)) -> bool {
    r.0 <= p.0 && p.0 <= r.2 && r.1 <= p.1 && p.1 <= r.3
}

/// Whether 2D segments `p1-p2` and `p3-p4` properly cross.
fn seg_seg_2d(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> bool {
    let ccw = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (c.1 - a.1) * (b.0 - a.0) - (b.1 - a.1) * (c.0 - a.0)
    };
    let (d1, d2, d3, d4) = (
        ccw(p3, p4, p1),
        ccw(p3, p4, p2),
        ccw(p1, p2, p3),
        ccw(p1, p2, p4),
    );
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// Whether segment `a-b` touches the rectangle (the crossing test).
pub fn seg_rect_overlap(a: (f64, f64), b: (f64, f64), r: (f64, f64, f64, f64)) -> bool {
    if pt_in_rect(a, r) || pt_in_rect(b, r) {
        return true;
    }
    let c = [(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)];
    (0..4).any(|i| seg_seg_2d(a, b, c[i], c[(i + 1) % 4]))
}

/// The pick of the Select tool: a text block, a group, a visible edge, a
/// dimension, a label, a section plane, a guide, a face.
pub fn pick(vp: &mut Viewport, x: f64, y: f64) -> Option<EntityRef> {
    if let Some(l) = vp.pick_text_label(x, y, true) {
        return Some(EntityRef::TextLabel(l));
    }
    let group = vp.pick_group(x, y);
    let edge = vp.pick_visible_edge(x, y);
    let edge_ref = edge
        .as_ref()
        .and_then(|e| e.edge)
        .map(|(m, e)| EntityRef::Edge(m, e));
    if let (Some(_), Some(e)) = (group, &edge) {
        // A loose line drawn ON a group's face is the deliberate target.
        if let Some(p) = vp.edge_point_under_cursor(e.a, e.b, x, y) {
            if !vp.is_occluded(p) {
                return edge_ref;
            }
        }
    }
    if let Some(g) = group {
        return Some(EntityRef::Group(g));
    }
    if edge_ref.is_some() {
        return edge_ref;
    }
    if let Some(d) = vp.pick_dimension(x, y) {
        return Some(EntityRef::Dimension(d));
    }
    if let Some(l) = vp.pick_text_label(x, y, false) {
        return Some(EntityRef::TextLabel(l));
    }
    if let Some(s) = vp.pick_section_plane(x, y) {
        return Some(EntityRef::SectionPlane(s));
    }
    if let Some(g) = vp.pick_guide(x, y) {
        return Some(EntityRef::Guide(g));
    }
    vp.pick_face(x, y).map(|(m, f)| EntityRef::Face(m, f))
}

/// Grow a pick to its natural whole: a smooth surface for a face, the whole
/// curve for one of its segments.
pub fn expand_pick(vp: &Viewport, entity: EntityRef) -> Vec<EntityRef> {
    let mid = vp.scene.active_mesh_id();
    let mesh = vp.scene.mesh();
    match entity {
        EntityRef::Face(m, f) if m == mid && mesh.has_face(f) => mesh
            .surface_of(f)
            .into_iter()
            .map(|f| EntityRef::Face(m, f))
            .collect(),
        EntityRef::Edge(m, e) if m == mid && mesh.has_edge(e) && mesh.edge(e).curve.is_some() => {
            mesh.curve_edges(e)
                .into_iter()
                .map(|e| EntityRef::Edge(m, e))
                .collect()
        }
        other => vec![other],
    }
}

/// Erase `entities` (any mix) as ONE undoable step.
pub fn erase_entities(vp: &mut Viewport, entities: &[EntityRef]) -> bool {
    let mid = vp.scene.active_mesh_id();
    let mut edges: Vec<EdgeId> = Vec::new();
    let mut faces: Vec<FaceId> = Vec::new();
    let mut groups = Vec::new();
    let (mut dims, mut labels, mut guides, mut planes) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for e in entities {
        match *e {
            EntityRef::Edge(m, x) if m == mid => edges.push(x),
            EntityRef::Face(m, x) if m == mid => faces.push(x),
            EntityRef::Group(g) => groups.push(g),
            EntityRef::Dimension(d) => dims.push(d),
            EntityRef::TextLabel(t) => labels.push(t),
            EntityRef::Guide(g) => guides.push(g),
            EntityRef::SectionPlane(p) => planes.push(p),
            _ => {}
        }
    }
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    if !edges.is_empty() || !faces.is_empty() {
        cmds.push(Box::new(EraseSelectionCommand::from_entities(
            vp.scene.mesh(),
            &edges,
            &faces,
        )));
    }
    for g in groups {
        cmds.push(Box::new(DeleteGroupCommand { group: g }));
    }
    if !guides.is_empty() {
        cmds.push(Box::new(DeleteGuidesCommand { ids: guides }));
    }
    if !planes.is_empty() {
        cmds.push(Box::new(DeleteSectionPlanesCommand { ids: planes }));
    }
    if !dims.is_empty() {
        cmds.push(Box::new(DeleteDimensionsCommand { ids: dims }));
    }
    if !labels.is_empty() {
        cmds.push(Box::new(DeleteTextLabelsCommand { ids: labels }));
    }
    if cmds.is_empty() {
        return false;
    }
    if cmds.len() == 1 {
        let mut c = cmds.pop().expect("one command");
        vp.execute(c.as_mut());
    } else {
        vp.execute(&mut CompoundCommand::new(cmds));
    }
    vp.update();
    true
}

/// The Delete key: erase the selection; with nothing selected, what is
/// highlighted under the cursor (grown like a click would).
pub fn delete_selection_or_hover(vp: &mut Viewport) -> bool {
    let selection: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
    if !selection.is_empty() {
        return erase_entities(vp, &selection);
    }
    let Some(h) = vp.hover_entity() else {
        return false;
    };
    vp.set_hover(None);
    let ents = expand_pick(vp, h);
    erase_entities(vp, &ents)
}

#[derive(Default)]
pub struct SelectTool;

impl SelectTool {
    pub fn new() -> SelectTool {
        SelectTool
    }

    fn box_select_impl(
        vp: &mut Viewport,
        rect: (f64, f64, f64, f64),
        crossing: bool,
        mode: SelectMode,
    ) {
        let mut picked: Vec<EntityRef> = Vec::new();
        let mid = vp.scene.active_mesh_id();
        // Loose edges and faces.
        {
            let mesh = vp.scene.mesh();
            for e in mesh.edge_ids() {
                if !vp.scene.entity_selectable(&EntityRef::Edge(mid, e)) || mesh.edge(e).hidden {
                    continue;
                }
                let (a, b) = mesh.edge_ab(e);
                let (Some(pa), Some(pb)) = (vp.world_to_pixel(a), vp.world_to_pixel(b)) else {
                    continue;
                };
                let hit = if crossing {
                    seg_rect_overlap(pa.tuple(), pb.tuple(), rect)
                } else {
                    pt_in_rect(pa.tuple(), rect) && pt_in_rect(pb.tuple(), rect)
                };
                if hit {
                    picked.push(EntityRef::Edge(mid, e));
                }
            }
            for f in mesh.face_ids() {
                if !vp.scene.entity_selectable(&EntityRef::Face(mid, f)) {
                    continue;
                }
                let pts: Option<Vec<(f64, f64)>> = mesh
                    .face_positions(f)
                    .into_iter()
                    .map(|p| vp.world_to_pixel(p).map(ScreenPt::tuple))
                    .collect();
                let Some(pts) = pts else { continue };
                let n = pts.len();
                let hit = if crossing {
                    pts.iter().any(|p| pt_in_rect(*p, rect))
                        || (0..n).any(|i| seg_rect_overlap(pts[i], pts[(i + 1) % n], rect))
                } else {
                    pts.iter().all(|p| pt_in_rect(*p, rect))
                };
                if hit {
                    picked.push(EntityRef::Face(mid, f));
                }
            }
        }
        // Groups: inside a group, its children.
        let candidates: Vec<xtrude_model::ids::GroupId> = match vp.scene.edit_group() {
            None => vp.scene.doc.groups.iter().map(|g| g.id).collect(),
            Some(ctx) => vp
                .scene
                .doc
                .find_group(ctx)
                .map(|g| g.children.iter().map(|c| c.id).collect())
                .unwrap_or_default(),
        };
        for gid in candidates {
            if !vp.scene.entity_selectable(&EntityRef::Group(gid)) {
                continue;
            }
            let Some(group) = vp.scene.doc.find_group(gid) else {
                continue;
            };
            let places: Vec<(xtrude_model::ids::MeshId, Option<glam::DMat4>)> =
                iter_placements(group, None)
                    .into_iter()
                    .filter(|(pg, _)| !vp.scene.doc.mesh(pg.mesh).verts.is_empty())
                    .map(|(pg, m)| (pg.mesh, m))
                    .collect();
            if places.is_empty() {
                continue;
            }
            let gw2p = |p: glam::DVec3, xf: Option<glam::DMat4>| {
                use xtrude_core::math::MatExt;
                vp.world_to_pixel(xf.map_or(p, |m| m.project_point(p)))
            };
            let hit = if crossing {
                let mut hit = places.iter().any(|(m, xf)| {
                    vp.scene.doc.mesh(*m).verts.iter().any(|(_, v)| {
                        gw2p(v.position, *xf).is_some_and(|p| pt_in_rect(p.tuple(), rect))
                    })
                });
                if !hit {
                    hit = places.iter().any(|(m, xf)| {
                        let mesh = vp.scene.doc.mesh(*m);
                        mesh.edge_ids().into_iter().any(|e| {
                            let (a, b) = mesh.edge_ab(e);
                            match (gw2p(a, *xf), gw2p(b, *xf)) {
                                (Some(pa), Some(pb)) => {
                                    seg_rect_overlap(pa.tuple(), pb.tuple(), rect)
                                }
                                _ => false,
                            }
                        })
                    });
                }
                hit
            } else {
                places.iter().all(|(m, xf)| {
                    vp.scene.doc.mesh(*m).verts.iter().all(|(_, v)| {
                        gw2p(v.position, *xf).is_some_and(|p| pt_in_rect(p.tuple(), rect))
                    })
                })
            };
            if hit {
                picked.push(EntityRef::Group(gid));
            }
        }
        // Guides: a line only by a crossing box.
        for g in &vp.scene.doc.guides {
            if !vp.scene.entity_selectable(&EntityRef::Guide(g.id)) {
                continue;
            }
            if g.is_line() {
                if !crossing {
                    continue;
                }
                let (a, b) = g.segment();
                let Some((a, b)) = vp.clip_segment_front(a, b) else {
                    continue;
                };
                if let (Some(pa), Some(pb)) = (vp.world_to_pixel(a), vp.world_to_pixel(b)) {
                    if seg_rect_overlap(pa.tuple(), pb.tuple(), rect) {
                        picked.push(EntityRef::Guide(g.id));
                    }
                }
            } else if vp
                .world_to_pixel(g.point)
                .is_some_and(|p| pt_in_rect(p.tuple(), rect))
            {
                picked.push(EntityRef::Guide(g.id));
            }
        }
        let doc = vp.scene.doc_view();
        for d in &vp.scene.doc.dimensions {
            if !vp.scene.entity_selectable(&EntityRef::Dimension(d.id)) {
                continue;
            }
            let (ap, bp) = d.line_points(&doc);
            let (Some(pa), Some(pb)) = (vp.world_to_pixel(ap), vp.world_to_pixel(bp)) else {
                continue;
            };
            let hit = if crossing {
                seg_rect_overlap(pa.tuple(), pb.tuple(), rect)
            } else {
                pt_in_rect(pa.tuple(), rect) && pt_in_rect(pb.tuple(), rect)
            };
            if hit {
                picked.push(EntityRef::Dimension(d.id));
            }
        }
        for lab in &vp.scene.doc.text_labels {
            if !vp.scene.entity_selectable(&EntityRef::TextLabel(lab.id)) {
                continue;
            }
            let pa = vp.world_to_pixel(lab.anchor);
            let Some(pp) = vp.world_to_pixel(lab.position()) else {
                continue;
            };
            let hit = if crossing {
                pt_in_rect(pp.tuple(), rect)
                    || pa.is_some_and(|a| seg_rect_overlap(a.tuple(), pp.tuple(), rect))
            } else {
                pt_in_rect(pp.tuple(), rect) && pa.is_none_or(|a| pt_in_rect(a.tuple(), rect))
            };
            if hit {
                picked.push(EntityRef::TextLabel(lab.id));
            }
        }
        vp.scene.select(picked, mode);
        vp.update();
    }
}

impl Tool for SelectTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("select"),
            shortcut: Some(""),
            description: Some(
                "Pick edges, faces and objects. Shift+click adds or takes away, Ctrl+click adds, \
             Shift+Ctrl+click takes away; the same with a box.",
            ),
            box_select: true,
            ..ToolInfo::new("select", "Select")
        };
        &INFO
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    tool_any!();

    /// The arrow: Select picks, it does not aim.
    fn cursor(&self, _vp: &Viewport) -> Option<crate::viewport::Cursor> {
        None
    }

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let mode = selection_mode(ctx.modifiers);
        match pick(vp, ctx.screen.0, ctx.screen.1) {
            None => {
                if vp.scene.edit_group().is_some()
                    && mode == SelectMode::Replace
                    && vp.scene.selection.is_empty()
                {
                    vp.end_one_group_edit();
                    return;
                }
                if mode == SelectMode::Replace {
                    vp.scene.clear_selection();
                }
            }
            Some(e) => {
                let picked = expand_pick(vp, e);
                vp.scene.select(picked, mode);
            }
        }
        vp.update();
    }

    fn on_double_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let entity = pick(vp, ctx.screen.0, ctx.screen.1);
        match entity {
            Some(EntityRef::Group(g)) => {
                vp.begin_group_edit(g);
            }
            Some(EntityRef::SectionPlane(id)) => {
                let active = vp
                    .scene
                    .doc
                    .section_planes
                    .iter()
                    .any(|p| p.id == id && p.active);
                vp.run(SetActiveSectionCommand {
                    id: if active { None } else { Some(id) },
                });
                vp.scene
                    .select([EntityRef::SectionPlane(id)], SelectMode::Replace);
                vp.update();
            }
            Some(EntityRef::TextLabel(id)) => {
                // Double-clicking a leader text edits its text.
                let current = vp
                    .scene
                    .doc
                    .text_labels
                    .iter()
                    .find(|l| l.id == id)
                    .map(|l| l.text.clone());
                if let Some(current) = current {
                    let title = tr("Text", &[]);
                    let label = tr("Label text:", &[]);
                    if let Some(text) = vp.prompt_text(&title, &label, &current, true) {
                        let t = text.trim();
                        if !t.is_empty() && t != current {
                            vp.run(EditTextLabelCommand {
                                id,
                                text: t.to_string(),
                            });
                        }
                    }
                }
                vp.update();
            }
            Some(EntityRef::Dimension(id)) => {
                // "<>" stands for the measured value; empty (or a bare "<>")
                // goes back to the automatic value.
                let doc = vp.scene.doc_view();
                let found = vp
                    .scene
                    .doc
                    .dimensions
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| (d.value(&doc), d.text.clone()));
                if let Some((value, text)) = found {
                    let measured =
                        crate::viewport::format_dim_value(value, &vp.scene.dimension_style);
                    let current = text.clone().unwrap_or_else(|| measured.clone());
                    let title = tr("Dimension", &[]);
                    let label = tr("Dimension text (<> = measured value):", &[]);
                    if let Some(t) = vp.prompt_text(&title, &label, &current, false) {
                        let t = t.trim().to_string();
                        let new = if t.is_empty() || t == "<>" || t == measured {
                            None
                        } else {
                            Some(t)
                        };
                        if new != text {
                            vp.run(EditDimensionTextCommand { id, text: new });
                        }
                    }
                }
                vp.update();
            }
            Some(e @ (EntityRef::Face(..) | EntityRef::Edge(..))) => {
                let mid = vp.scene.active_mesh_id();
                let mut picked = expand_pick(vp, e);
                let mesh = vp.scene.mesh();
                if e.is_face() {
                    for p in picked.clone() {
                        let EntityRef::Face(_, f) = p else { continue };
                        for lp in mesh.face(f).loops() {
                            let n = lp.len();
                            for i in 0..n {
                                if let Some(ed) = mesh.find_edge(lp[i], lp[(i + 1) % n]) {
                                    picked.push(EntityRef::Edge(mid, ed));
                                }
                            }
                        }
                    }
                } else {
                    for p in picked.clone() {
                        let EntityRef::Edge(_, ed) = p else { continue };
                        for &f in &mesh.edge(ed).faces {
                            picked.push(EntityRef::Face(mid, f));
                        }
                    }
                }
                vp.scene.select(picked, selection_mode(ctx.modifiers));
                vp.update();
            }
            _ => self.on_click(vp, ctx),
        }
    }

    fn on_triple_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let entity = pick(vp, ctx.screen.0, ctx.screen.1);
        match entity {
            Some(e @ (EntityRef::Face(..) | EntityRef::Edge(..))) => {
                let mid = vp.scene.active_mesh_id();
                let all = xtrude_model::select_ops::all_connected(vp.scene.mesh(), mid, &[e]);
                vp.scene.select(all, selection_mode(ctx.modifiers));
                vp.update();
            }
            _ => self.on_click(vp, ctx),
        }
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let p = pick(vp, ctx.screen.0, ctx.screen.1);
        vp.set_hover(p);
    }

    fn on_box_select(
        &mut self,
        vp: &mut Viewport,
        rect: (f64, f64, f64, f64),
        crossing: bool,
        mode: SelectMode,
    ) {
        Self::box_select_impl(vp, rect, crossing, mode);
    }

    fn on_key(&mut self, vp: &mut Viewport, key: Key, _m: Modifiers) -> bool {
        match key {
            Key::Delete => {
                delete_selection_or_hover(vp);
                true
            }
            Key::Backspace => {
                let sel: Vec<EntityRef> = vp.scene.selection.iter().copied().collect();
                erase_entities(vp, &sel);
                true
            }
            _ => false,
        }
    }

    fn busy(&self) -> bool {
        false
    }
}
