// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Solids: Merge Solids, Union, Subtract, Trim, Intersect, Split (the
//! operations live in `xtrude_model::solids`).
//!
//! Pre-selection: Merge Solids, Union and Intersect run at once on every
//! solid selected, Split on two. Clicks: solid 1, then solid 2 — for
//! Subtract and Trim the FIRST is the cutter; Union and Merge Solids keep
//! going, every further click adds a solid to the result. The pointer
//! says what a click would do: a slashed circle, a «1» or a «2».

use std::cell::RefCell;
use std::collections::HashMap;

use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::solids::{is_solid, SolidError, SolidOp, SolidOperationCommand};

use super::{SnapPrefs, Tool, ToolContext, ToolInfo};
use crate::tool_any;
use crate::viewport::{Cursor, Viewport, ViewportEvent};

/// How the user is told why an operation did not run.
fn error_text(err: &SolidError) -> String {
    let msg = match err.0.as_str() {
        "not a solid" => {
            "That is not a solid — a group or component closed all round, with nothing nested inside"
        }
        "no overlap" => "The two solids do not overlap",
        "empty" => "The operation leaves nothing",
        "not manifold" => "That solid has a flaw the operation cannot read",
        "two solids" => "Pick two solids",
        other => other,
    };
    tr(msg, &[])
}

/// What the pointer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    No,
    One,
    Two,
}

pub struct SolidTool {
    pub op: SolidOp,
    pub first: Option<GroupId>,
    pub badge: Badge,
    /// `is_solid` per group, valid for one scene version (the hover asks
    /// on every move and the check walks the whole mesh).
    cache: RefCell<(u64, HashMap<GroupId, bool>)>,
}

impl SolidTool {
    pub fn new(op: SolidOp) -> SolidTool {
        SolidTool {
            op,
            first: None,
            badge: Badge::No,
            cache: RefCell::new((u64::MAX, HashMap::new())),
        }
    }

    /// Union and Merge Solids accept more solids after the first result.
    fn chains(&self) -> bool {
        matches!(self.op, SolidOp::Union | SolidOp::MergeSolids)
    }

    /// The first solid is picked and the tool waits for the second.
    pub fn dragging(&self) -> bool {
        self.first.is_some()
    }

    pub fn is_solid(&self, vp: &Viewport, g: GroupId) -> bool {
        let mut c = self.cache.borrow_mut();
        if c.0 != vp.scene.version {
            *c = (vp.scene.version, HashMap::new());
        }
        *c.1.entry(g).or_insert_with(|| {
            vp.scene
                .doc
                .find_group(g)
                .is_some_and(|grp| is_solid(&vp.scene.doc.meshes, grp))
        })
    }

    /// Run the operation; the groups it made (empty when it could not).
    fn execute(&self, vp: &mut Viewport, groups: Vec<GroupId>) -> Vec<GroupId> {
        let mut cmd = SolidOperationCommand::new(self.op, groups);
        if let Err(err) = cmd.prepare(&vp.scene) {
            vp.flash_status(&error_text(&err), 5000);
            return Vec::new();
        }
        vp.execute(&mut cmd);
        vp.update();
        cmd.created.clone()
    }
}

/// The six tools, `(key, op)`, in toolbar and menu order.
pub const SOLID_TOOLS: [(&str, SolidOp); 6] = [
    ("merge_solids", SolidOp::MergeSolids),
    ("solid_union", SolidOp::Union),
    ("solid_subtract", SolidOp::Subtract),
    ("solid_trim", SolidOp::Trim),
    ("solid_intersect", SolidOp::Intersect),
    ("solid_split", SolidOp::Split),
];

impl Tool for SolidTool {
    fn info(&self) -> &'static ToolInfo {
        static MERGE_SOLIDS: ToolInfo = ToolInfo {
            icon: Some("merge_solids"),
            description: Some("Merge the selected solids into one, keeping only their outer skin."),
            ..ToolInfo::new("merge_solids", "Merge Solids")
        };
        static UNION: ToolInfo = ToolInfo {
            icon: Some("solid_union"),
            description: Some("Join solids into a single solid."),
            ..ToolInfo::new("solid_union", "Union")
        };
        static SUBTRACT: ToolInfo = ToolInfo {
            icon: Some("solid_subtract"),
            description: Some(
                "Cut the first solid clicked out of the second, and remove the first.",
            ),
            ..ToolInfo::new("solid_subtract", "Subtract")
        };
        static TRIM: ToolInfo = ToolInfo {
            icon: Some("solid_trim"),
            description: Some("Cut the first solid clicked out of the second, and keep the first."),
            ..ToolInfo::new("solid_trim", "Trim")
        };
        static INTERSECT: ToolInfo = ToolInfo {
            icon: Some("solid_intersect"),
            description: Some("Keep only the part where the solids overlap."),
            ..ToolInfo::new("solid_intersect", "Intersect")
        };
        static SPLIT: ToolInfo = ToolInfo {
            icon: Some("solid_split"),
            description: Some(
                "Split two overlapping solids into their separate parts and their common part.",
            ),
            ..ToolInfo::new("solid_split", "Split")
        };
        match self.op {
            SolidOp::MergeSolids => &MERGE_SOLIDS,
            SolidOp::Union => &UNION,
            SolidOp::Subtract => &SUBTRACT,
            SolidOp::Trim => &TRIM,
            SolidOp::Intersect => &INTERSECT,
            SolidOp::Split => &SPLIT,
        }
    }
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs {
            uses_snap: false,
            ..SnapPrefs::default()
        }
    }
    /// Mid-operation once the first solid is picked.
    fn phase(&self) -> &'static str {
        if self.dragging() {
            "started"
        } else {
            "idle"
        }
    }
    fn busy(&self) -> bool {
        self.first.is_some()
    }
    fn cursor(&self, _vp: &Viewport) -> Option<Cursor> {
        Some(Cursor::Tool {
            key: match self.badge {
                Badge::No => "solid_no",
                Badge::One => "solid_1",
                Badge::Two => "solid_2",
            },
            plus: false,
        })
    }
    tool_any!();

    fn on_activate(&mut self, vp: &mut Viewport) {
        self.first = None;
        self.badge = Badge::No;
        // the selected top-level groups, in document order
        let picked: Vec<GroupId> = vp
            .scene
            .doc
            .groups
            .iter()
            .map(|g| g.id)
            .filter(|g| vp.scene.selection.contains(&EntityRef::Group(*g)))
            .collect();
        let solid: Vec<GroupId> = picked
            .iter()
            .copied()
            .filter(|&g| self.is_solid(vp, g))
            .collect();
        let runs_on_selection = (matches!(
            self.op,
            SolidOp::Union | SolidOp::MergeSolids | SolidOp::Intersect
        ) && solid.len() >= 2)
            || (self.op == SolidOp::Split && solid.len() == 2);
        if runs_on_selection && solid.len() == picked.len() && !self.execute(vp, solid).is_empty() {
            vp.outbox
                .events
                .push(ViewportEvent::ActivateTool("select".into()));
            return;
        }
        if !picked.is_empty() && matches!(self.op, SolidOp::Subtract | SolidOp::Trim) {
            vp.flash_status(
                &tr("Click the solid that cuts, then the one it cuts", &[]),
                4000,
            );
        }
        vp.scene.selection.clear();
        vp.scene.bump_view();
    }

    fn on_deactivate(&mut self, _vp: &mut Viewport) {
        self.first = None;
    }

    fn on_cancel(&mut self, vp: &mut Viewport) {
        self.first = None;
        vp.scene.selection.clear();
        vp.scene.bump_view();
        vp.update();
    }

    fn on_hover(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let g = vp.pick_group(ctx.screen.0, ctx.screen.1);
        let badge = match g {
            Some(g) if Some(g) != self.first && self.is_solid(vp, g) => {
                if self.first.is_none() {
                    Badge::One
                } else {
                    Badge::Two
                }
            }
            _ => Badge::No,
        };
        if badge != self.badge {
            self.badge = badge;
            vp.refresh_tool_cursor();
        }
    }

    fn on_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        let Some(g) = vp.pick_group(ctx.screen.0, ctx.screen.1) else {
            return;
        };
        if Some(g) == self.first {
            return;
        }
        if !self.is_solid(vp, g) {
            vp.flash_status(&error_text(&SolidError("not a solid".into())), 4000);
            return;
        }
        let Some(first) = self.first else {
            self.first = Some(g);
            vp.scene.selection.clear();
            vp.scene.selection.insert(EntityRef::Group(g));
            vp.scene.version += 1;
            vp.update();
            return;
        };
        let made = self.execute(vp, vec![first, g]);
        if !made.is_empty() && self.chains() {
            self.first = Some(made[0]); // keep adding solids to it
        } else {
            self.first = None;
            if made.is_empty() {
                vp.scene.selection.clear();
                vp.scene.version += 1;
            }
        }
        self.badge = Badge::No;
        vp.update();
    }
}
