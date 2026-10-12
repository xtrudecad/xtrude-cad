// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Inspect ▸ Solid Inspector: why a solid is not watertight. The diagnosis and
//! the dialog's rows live here; the shell draws them.
//!
//! One row per solid candidate (the geometry being edited, plus every
//! group). Per edge, counting non-interior incident faces:
//!
//! - `0` faces → a **stray edge**: loose drawing, not part of any surface;
//! - `1` face → an **open border**: the rim of a hole in the skin;
//! - `>2` faces → **overconnected**: an internal wall / T-junction the
//!   boundary should not have.
//!
//! Watertight = every edge has exactly 2. The volume comes from
//! `measure::face_set_volume`, the same computation as the Solids tools.
//! Read-only: inspecting never modifies the model.

use xtrude_core::measure::face_set_volume;
use xtrude_core::{EdgeId, FaceId, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::scene::{Scene, SelectMode};

use super::model_info::fmt_count;
use crate::viewport::Viewport;

/// The menu entry.
pub const NAME: &str = "Solid Inspector";
/// Its status tip.
pub const DESCRIPTION: &str = "Find out why a group is not a solid, and show the edges to blame.";

/// `vp.flash_status`'s default duration.
const FLASH_MS: u32 = 2500;

/// What `inspect_mesh` finds in one mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolidReport {
    pub faces: usize,
    pub edges: usize,
    pub stray: Vec<EdgeId>,
    pub open: Vec<EdgeId>,
    pub over: Vec<EdgeId>,
    pub watertight: bool,
    /// m³, only when the mesh is closed.
    pub volume: Option<f64>,
}

/// Classify every edge of `mesh` and compute the closed volume
/// (`inspect_mesh`).
pub fn inspect_mesh(mesh: &Mesh) -> SolidReport {
    let (mut stray, mut open, mut over) = (Vec::new(), Vec::new(), Vec::new());
    for (e, edge) in mesh.edges.iter() {
        let n = edge
            .faces
            .iter()
            .filter(|&&f| !mesh.face(f).interior)
            .count();
        match n {
            0 => stray.push(e),
            1 => open.push(e),
            2 => {}
            _ => over.push(e),
        }
    }

    let watertight =
        stray.is_empty() && open.is_empty() && over.is_empty() && !mesh.faces.is_empty();
    let volume = if watertight {
        let shell: Vec<FaceId> = mesh
            .faces
            .iter()
            .filter(|(_, f)| !f.interior)
            .map(|(id, _)| id)
            .collect();
        face_set_volume(mesh, &shell)
    } else {
        None
    };

    SolidReport {
        faces: mesh.faces.len(),
        edges: mesh.edges.len(),
        stray,
        open,
        over,
        watertight,
        volume,
    }
}

/// The problems in words, empty when there are none.
pub fn problem_text(report: &SolidReport) -> String {
    let mut parts = Vec::new();
    if !report.open.is_empty() {
        parts.push(tr("{n} open borders", &[("n", &report.open.len())]));
    }
    if !report.stray.is_empty() {
        parts.push(tr("{n} stray edges", &[("n", &report.stray.len())]));
    }
    if !report.over.is_empty() {
        parts.push(tr("{n} overconnected", &[("n", &report.over.len())]));
    }
    parts.join(" · ")
}

/// What a row stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowKind {
    /// The mesh being edited: its problem edges can be selected directly.
    Active(MeshId),
    Group(GroupId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct InspectorRow {
    pub name: String,
    pub kind: RowKind,
    pub report: SolidReport,
}

/// The Status column's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Empty,
    Watertight,
    NotWatertight,
}

impl Verdict {
    pub fn of(report: &SolidReport) -> Self {
        if report.faces == 0 && report.edges == 0 {
            Verdict::Empty
        } else if report.watertight {
            Verdict::Watertight
        } else {
            Verdict::NotWatertight
        }
    }

    /// The cell text. A failure is drawn as ✖, which egui's fonts have (✘
    /// is not).
    pub fn text(self) -> String {
        match self {
            Verdict::Empty => tr("(empty)", &[]),
            Verdict::Watertight => format!("✔ {}", tr("watertight", &[])),
            Verdict::NotWatertight => format!("✖ {}", tr("not watertight", &[])),
        }
    }
}

/// The open inspector: its rows and the table's
/// current row. Modeless: fix, press Refresh, repeat until everything ✔.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidInspector {
    pub rows: Vec<InspectorRow>,
    /// The table's current row.
    pub current: Option<usize>,
}

impl SolidInspector {
    pub fn new(scene: &Scene) -> Self {
        let mut dlg = SolidInspector {
            rows: Vec::new(),
            current: None,
        };
        dlg.refresh(scene);
        dlg
    }

    /// Re-inspect the scene (`refresh`).
    pub fn refresh(&mut self, scene: &Scene) {
        self.rows.clear();

        // The mesh being edited right now: the open group's, or the loose
        // one. Problem edges here CAN be highlighted directly.
        let edit_group = scene.edit_group();
        let name = match edit_group.and_then(|g| scene.doc.find_group(g)) {
            Some(g) => tr("Open group: {name}", &[("name", &g.name)]),
            None => tr("Loose geometry", &[]),
        };
        self.rows.push(InspectorRow {
            name,
            kind: RowKind::Active(scene.active_mesh_id()),
            report: inspect_mesh(scene.mesh()),
        });

        // Every group; the one being edited is the live row above, so it
        // is not listed twice.
        for g in &scene.doc.groups {
            if Some(g.id) == edit_group {
                continue;
            }
            self.rows.push(InspectorRow {
                name: g.name.clone(),
                kind: RowKind::Group(g.id),
                report: inspect_mesh(scene.doc.mesh(g.mesh)),
            });
        }
        // a table keeps its current row while it still exists
        self.current = self.current.filter(|&i| i < self.rows.len());
    }

    /// The five cells of row `i`: Object, Faces, Status, Problems, Volume.
    pub fn cells(&self, i: usize) -> [String; 5] {
        let row = &self.rows[i];
        let rep = &row.report;
        [
            row.name.clone(),
            fmt_count(rep.faces),
            Verdict::of(rep).text(),
            problem_text(rep),
            rep.volume
                .map_or_else(|| "—".to_string(), |v| format!("{v:.3} m³")),
        ]
    }

    /// The line above the table.
    pub fn summary(&self) -> String {
        let count = |v| {
            self.rows
                .iter()
                .filter(|r| Verdict::of(&r.report) == v)
                .count()
        };
        tr(
            "{ok} watertight solids, {bad} with problems",
            &[
                ("ok", &count(Verdict::Watertight)),
                ("bad", &count(Verdict::NotWatertight)),
            ],
        )
    }

    /// Select the problems of the current row in the viewport
    /// (`highlight`). Active-mesh row → its offending edges; group row →
    /// the group (enter it and press Refresh to see its edges).
    pub fn highlight(&self, vp: &mut Viewport) {
        let Some(row) = self.current.and_then(|i| self.rows.get(i)) else {
            return;
        };
        match row.kind {
            RowKind::Active(mid) => {
                let rep = &row.report;
                // edges deleted since the last Refresh are left out
                let mesh = vp.scene.doc.mesh(mid);
                let edges: Vec<EntityRef> = rep
                    .open
                    .iter()
                    .chain(&rep.stray)
                    .chain(&rep.over)
                    .filter(|&&e| mesh.edges.contains(e))
                    .map(|&e| EntityRef::Edge(mid, e))
                    .collect();
                let n = edges.len();
                vp.scene.select(edges, SelectMode::Replace);
                if n > 0 {
                    vp.flash_status(&tr("{n} problem edges selected", &[("n", &n)]), FLASH_MS);
                } else {
                    vp.flash_status(&tr("Nothing to highlight — watertight", &[]), FLASH_MS);
                }
            }
            RowKind::Group(g) => {
                vp.scene.select([EntityRef::Group(g)], SelectMode::Replace);
                vp.flash_status(
                    &tr(
                        "Group selected — double-click into it and press Refresh",
                        &[],
                    ),
                    FLASH_MS,
                );
            }
        }
        vp.update();
    }
}
