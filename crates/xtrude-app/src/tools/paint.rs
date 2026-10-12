// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Paint tool: paint the side of a face you click (the selection when
//! the face is in it, a smooth surface as a whole); a face inside a closed
//! group paints the group; Alt (held or tapped) samples a color instead.

use crate::input::ScreenPt;
use std::cell::RefCell;

use xtrude_core::attrs::{Back, FaceAttrs, Paint, Rgb};
use xtrude_core::snap::face_plane_world;
use xtrude_core::FaceId;
use xtrude_model::commands::{
    SetFaceBackCommand, SetFaceColorCommand, SetFaceMaterialTagCommand, SetFaceOpacityCommand,
    SetGroupMaterialCommand,
};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::effective_material;
use xtrude_model::history::{Command, CompoundCommand};
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::materials::{effective_attrs, has_own_material, Material};

use super::{paint_sample_armed, set_paint_sample_armed, SnapPrefs, Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::{Cursor, FaceHit, Viewport, ViewportEvent};

/// The cream unpainted faces show.
pub const DEFAULT_FACE_COLOR: Rgb = [0.96, 0.95, 0.925];

/// The current paint (shared by every Paint tool).
#[derive(Clone, Debug)]
pub struct PaintState {
    pub color: Rgb,
    /// The registry entry the colour is named by, `None` = an anonymous
    /// colour.
    pub named_color: Option<Material>,
    /// Translucency, `None` = opaque.
    pub opacity: Option<f64>,
    /// The «Default» swatch: painting removes the paint.
    pub is_default: bool,
}

impl Default for PaintState {
    fn default() -> Self {
        PaintState {
            color: [0.80, 0.45, 0.30],
            named_color: None,
            opacity: None,
            is_default: false,
        }
    }
}

thread_local! {
    static STATE: RefCell<PaintState> = RefCell::new(PaintState::default());
}

pub fn paint_state() -> PaintState {
    STATE.with(|s| s.borrow().clone())
}

pub fn set_paint_state(state: PaintState) {
    STATE.with(|s| *s.borrow_mut() = state);
}

/// Whether the cursor sees the BACK of the hit face (no ray: the front).
pub fn clicked_back_side(vp: &Viewport, hit: &FaceHit, x: f64, y: f64) -> bool {
    let Some((_o, d)) = vp.pixel_to_ray(ScreenPt::new(x, y)) else {
        return false;
    };
    let (_p, n) = face_plane_world(vp.scene.doc.mesh(hit.mesh), hit.face, hit.xform.as_ref());
    n.dot(d) > 0.0
}

/// The current paint as a container's or a back side's paint.
pub fn current_as_paint(st: &PaintState) -> Paint {
    Paint {
        color: Some(st.color),
        mat: st.named_color.as_ref().map(|m| m.name.clone()),
        opacity: st.opacity,
    }
}

#[derive(Default)]
pub struct PaintTool;

impl PaintTool {
    pub fn new() -> PaintTool {
        PaintTool
    }

    fn sample(vp: &mut Viewport, hit: &FaceHit, back_side: bool) {
        let mesh = vp.scene.doc.mesh(hit.mesh);
        let paint_of = |g: Option<GroupId>| {
            g.and_then(|g| vp.scene.doc.find_group(g))
                .and_then(|g| g.material.as_ref())
        };
        let owner_paint = effective_material(paint_of(hit.placement), paint_of(hit.owner)).cloned();
        let attrs = mesh.face(hit.face).attrs.clone();
        let mut src: FaceAttrs = effective_attrs(&attrs, owner_paint.as_ref());
        if back_side {
            src = match &attrs.back {
                Some(Back::Own(p)) => FaceAttrs {
                    paint: p.clone(),
                    ..Default::default()
                },
                Some(Back::Mirror) => src,
                None => FaceAttrs::default(),
            };
        }
        let st = PaintState {
            is_default: !has_own_material(&src),
            color: src.paint.color.unwrap_or(DEFAULT_FACE_COLOR),
            opacity: src.paint.opacity,
            named_color: src
                .paint
                .mat
                .as_ref()
                .and_then(|n| vp.scene.doc.materials.get(n).cloned()),
        };
        set_paint_state(st);
        if paint_sample_armed() {
            set_paint_sample_armed(false);
        }
        vp.outbox.events.push(ViewportEvent::PaintSampled);
        vp.update();
        vp.flash_status(&tr("Color sampled — click a face to paint it", &[]), 2500);
    }
}

fn tag_cmd(faces: Vec<(MeshId, FaceId)>, mat: &Option<Material>) -> Box<dyn Command> {
    Box::new(SetFaceMaterialTagCommand {
        faces,
        name: mat.as_ref().map(|m| m.name.clone()),
        material: mat.clone(),
    })
}

impl Tool for PaintTool {
    fn info(&self) -> &'static ToolInfo {
        static INFO: ToolInfo = ToolInfo {
            icon: Some("paint"),
            shortcut: Some("B"),
            description: Some("Click faces to paint them with the chosen color."),
            ..ToolInfo::new("paint", "Paint")
        };
        &INFO
    }
    /// The eyedropper while a click would sample a color (Alt, or the
    /// sample action armed).
    fn cursor(&self, vp: &Viewport) -> Option<Cursor> {
        let key = if paint_sample_armed() || vp.alt_down() {
            "eyedropper"
        } else {
            self.info().icon?
        };
        Some(Cursor::Tool { key, plus: false })
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    tool_any!();

    fn on_deactivate(&mut self, vp: &mut Viewport) {
        vp.set_hover(None);
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let hit = vp.pick_face_any(ctx.screen.0, ctx.screen.1);
        vp.set_hover(hit.map(|h| h.entity()));
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let (x, y) = ctx.screen;
        let Some(hit) = vp.pick_face_any(x, y) else {
            return;
        };
        let back_side = clicked_back_side(vp, &hit, x, y);
        let alt = ctx.modifiers.alt || vp.alt_down();
        if alt || paint_sample_armed() {
            Self::sample(vp, &hit, back_side);
            return;
        }
        let st = paint_state();
        if hit.owner.is_some() {
            if let Some(obj) = vp.pick_group(x, y) {
                let material = (!st.is_default).then(|| current_as_paint(&st));
                vp.execute(&mut CompoundCommand::new(vec![
                    tag_cmd(Vec::new(), &st.named_color),
                    Box::new(SetGroupMaterialCommand {
                        group: obj,
                        material,
                    }),
                ]));
                let name = vp
                    .scene
                    .doc
                    .find_group(obj)
                    .map(|g| g.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| tr("Group", &[]));
                let msg = tr(
                    "{name} painted as a whole; a face that already had its own color keeps it",
                    &[("name", &name)],
                );
                vp.flash_status(&msg, 2500);
                vp.update();
                return;
            }
        }
        let mid = hit.mesh;
        let sel_faces: Vec<(MeshId, FaceId)> = vp
            .scene
            .selection
            .iter()
            .filter_map(|e| e.as_face())
            .collect();
        let faces: Vec<(MeshId, FaceId)> = if sel_faces.contains(&(mid, hit.face)) {
            sel_faces
        } else {
            vp.scene
                .doc
                .mesh(mid)
                .surface_of(hit.face)
                .into_iter()
                .map(|f| (mid, f))
                .collect()
        };
        if st.is_default {
            if back_side {
                vp.run(SetFaceBackCommand::uniform(faces, None));
            } else {
                vp.execute(&mut CompoundCommand::new(vec![
                    Box::new(SetFaceColorCommand {
                        faces: faces.clone(),
                        color: None,
                    }),
                    Box::new(SetFaceOpacityCommand {
                        faces: faces.clone(),
                        opacity: None,
                    }),
                    Box::new(SetFaceMaterialTagCommand {
                        faces,
                        name: None,
                        material: None,
                    }),
                ]));
            }
            vp.update();
            return;
        }
        if back_side {
            let back = Back::Own(current_as_paint(&st));
            let backs = vec![Some(back); faces.len()];
            vp.execute(&mut CompoundCommand::new(vec![
                tag_cmd(Vec::new(), &st.named_color),
                Box::new(SetFaceBackCommand { faces, backs }),
            ]));
            vp.update();
            return;
        }
        vp.execute(&mut CompoundCommand::new(vec![
            Box::new(SetFaceColorCommand {
                faces: faces.clone(),
                color: Some(st.color),
            }),
            Box::new(SetFaceOpacityCommand {
                faces: faces.clone(),
                opacity: st.opacity,
            }),
            tag_cmd(faces, &st.named_color),
        ]));
        vp.update();
    }

    fn busy(&self) -> bool {
        false
    }
}

/// The hovered face as an entity (for the window's status line).
pub fn hovered(vp: &Viewport) -> Option<EntityRef> {
    vp.hover_entity()
}
