// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Linear dimension — a measured annotation between two world points.
//!
//! An endpoint placed ON a vertex is **anchored** to it: the endpoint reads
//! the vertex's position live, so scaling or moving the drawing takes the
//! dimension along. When the vertex is gone the endpoint freezes where it
//! last was.
//!
//! Two kinds: **aligned** (`axis` is `None`) and **linear** (`axis` is a
//! world [`Axis`]), the line parallel to that axis.
//!
//! Design note: the anchor holds `(MeshId, VertexId)` plus the chain of group
//! ids instead of object references; reading a live endpoint takes the
//! document.

use serde_json::{json, Map, Value};
use xtrude_core::math::{DMat4, DVec3, MatExt};
use xtrude_core::mesh::VertexId;
use xtrude_core::Axis;

use crate::group::{iter_placements, Group, MeshTable};
use crate::ids::{next_id, GroupId, MeshId, ObjId};

/// A hold on one mesh vertex: the vertex, its mesh, and the chain of groups
/// whose transforms place it in the world (empty for loose geometry).
#[derive(Clone, Debug, PartialEq)]
pub struct VertexAnchor {
    pub vertex: VertexId,
    pub mesh: MeshId,
    pub chain: Vec<GroupId>,
}

/// The document pieces an anchor reads.
pub struct DocView<'a> {
    pub meshes: &'a MeshTable,
    pub groups: &'a [Group],
    pub loose: MeshId,
}

fn find_in(groups: &[Group], id: GroupId) -> Option<&Group> {
    groups.iter().find_map(|g| g.find(id))
}

impl VertexAnchor {
    /// The composed placement transform, or `None`.
    pub fn matrix(&self, doc: &DocView) -> Option<DMat4> {
        let mut m: Option<DMat4> = None;
        for gid in &self.chain {
            let Some(g) = find_in(doc.groups, *gid) else {
                continue;
            };
            if let Some(x) = g.xform {
                m = Some(match m {
                    None => x,
                    Some(mm) => mm * x,
                });
            }
        }
        m
    }

    /// Whether the hold still means something.
    pub fn alive(&self, doc: &DocView) -> bool {
        let Some(mesh) = doc.meshes.get(&self.mesh) else {
            return false;
        };
        if !mesh.has_vertex(self.vertex) {
            return false;
        }
        if mesh.vertex_at(mesh.pos(self.vertex)) != Some(self.vertex)
            || mesh.verts[self.vertex].edges.is_empty()
        {
            return false;
        }
        match self.chain.first() {
            None => self.mesh == doc.loose,
            Some(top) => {
                doc.groups.iter().any(|g| g.id == *top)
                    && find_in(doc.groups, *self.chain.last().unwrap_or(top))
                        .is_some_and(|g| g.mesh == self.mesh)
            }
        }
    }

    /// The vertex's world position now — `None` once not alive.
    pub fn position(&self, doc: &DocView) -> Option<DVec3> {
        if !self.alive(doc) {
            return None;
        }
        let p = doc.meshes[&self.mesh].pos(self.vertex);
        Some(match self.matrix(doc) {
            Some(m) => m.project_point(p),
            None => p,
        })
    }
}

fn chain_to(root: &Group, target: GroupId) -> Vec<GroupId> {
    if root.id == target {
        return vec![root.id];
    }
    for c in &root.children {
        let below = chain_to(c, target);
        if !below.is_empty() {
            let mut out = vec![root.id];
            out.extend(below);
            return out;
        }
    }
    Vec::new()
}

/// The vertex sitting at `point` (world), as a [`VertexAnchor`]: loose
/// geometry first, then every group placement.
pub fn resolve_vertex_anchor(doc: &DocView, point: DVec3, tol: f64) -> Option<VertexAnchor> {
    let hit = |mesh_id: MeshId, local: DVec3, chain: Vec<GroupId>| -> Option<VertexAnchor> {
        let mesh = doc.meshes.get(&mesh_id)?;
        let v = mesh.vertex_at(local)?;
        if (mesh.pos(v) - local).length() > tol {
            return None;
        }
        if mesh.verts[v].edges.is_empty() {
            return None; // an orphan holds nothing
        }
        Some(VertexAnchor {
            vertex: v,
            mesh: mesh_id,
            chain,
        })
    };
    if let Some(a) = hit(doc.loose, point, Vec::new()) {
        return Some(a);
    }
    for g in doc.groups {
        for (placed, m) in iter_placements(g, None) {
            let mut local = point;
            if let Some(m) = m {
                let Some(inv) = m.try_inverse() else {
                    continue;
                };
                local = inv.project_point(local);
            }
            if let Some(a) = hit(placed.mesh, local, chain_to(g, placed.id)) {
                return Some(a);
            }
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dimension {
    pub id: ObjId,
    /// Frozen endpoint positions (the last live reading).
    pub a0: DVec3,
    pub b0: DVec3,
    /// Displacement from the a–b segment to the dimension line.
    pub offset: DVec3,
    /// `None` = aligned; `Some(axis)` = linear along that world axis.
    pub axis: Option<Axis>,
    pub layer: Option<String>,
    /// Custom text; `<>` stands for the measured value.
    pub text: Option<String>,
    pub anchor_a: Option<VertexAnchor>,
    pub anchor_b: Option<VertexAnchor>,
}

/// How the document's dimensions look: the document's `"dimension_style"`
/// object. Keys this version does not know are kept in `extra` and written
/// back after the known ones, so a document from another version keeps
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionStyle {
    pub decimals: i64,
    /// Length unit key (`"mm"`, `"cm"`, …).
    pub units: String,
    pub font_size: i64,
    pub color: [u8; 3],
    pub norma: String,
    /// Spacing of stacked baseline dimensions, millimetres.
    pub base_step_mm: f64,
    /// `"arrow"`, `"tick"` or `"none"`.
    pub ends: String,
    pub extra: Map<String, Value>,
}

impl Default for DimensionStyle {
    fn default() -> Self {
        DimensionStyle {
            decimals: 2,
            units: "mm".into(),
            font_size: 9,
            color: [45, 55, 75],
            norma: "iso".into(),
            base_step_mm: 8.0,
            ends: "arrow".into(),
            extra: Map::new(),
        }
    }
}

impl DimensionStyle {
    /// Lay the keys of a stored style over this one. A known key of the
    /// wrong type reads as its default; numbers are truncated to integers
    /// where the field is one, and each colour channel is clamped to
    /// 0–255 (a missing channel keeps its default). An empty `ends` is
    /// arrows.
    pub fn merge_json(&mut self, m: &Map<String, Value>) {
        let d = DimensionStyle::default();
        for (k, v) in m {
            match k.as_str() {
                "decimals" => self.decimals = v.as_f64().map_or(d.decimals, |x| x as i64),
                "units" => self.units = v.as_str().unwrap_or(&d.units).to_string(),
                "font_size" => {
                    self.font_size = v.as_f64().map_or(d.font_size, |x| x.trunc() as i64)
                }
                "color" => {
                    let a = v.as_array();
                    self.color = [0, 1, 2].map(|i| {
                        a.and_then(|a| a.get(i))
                            .and_then(Value::as_f64)
                            .map_or(d.color[i], |x| x.clamp(0.0, 255.0) as u8)
                    });
                }
                "norma" => self.norma = v.as_str().unwrap_or(&d.norma).to_string(),
                "base_step_mm" => self.base_step_mm = v.as_f64().unwrap_or(d.base_step_mm),
                "ends" => {
                    self.ends = v
                        .as_str()
                        .filter(|e| !e.is_empty())
                        .unwrap_or(&d.ends)
                        .to_string()
                }
                _ => {
                    self.extra.insert(k.clone(), v.clone());
                }
            }
        }
    }

    /// The stored form: the known keys in their usual order, then `extra`.
    pub fn to_json(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("decimals".into(), json!(self.decimals));
        m.insert("units".into(), json!(self.units));
        m.insert("font_size".into(), json!(self.font_size));
        m.insert("color".into(), json!(self.color));
        m.insert("norma".into(), json!(self.norma));
        m.insert("base_step_mm".into(), json!(self.base_step_mm));
        m.insert("ends".into(), json!(self.ends));
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        m
    }
}

/// One end of a dimension.
#[derive(Clone, Copy, PartialEq)]
enum End {
    A,
    B,
}

/// `(u, e)` for a linear dimension along `axis`, or `None`.
pub fn linear_frame(a: DVec3, b: DVec3, axis: Option<Axis>) -> Option<(DVec3, DVec3)> {
    let u = axis?.world();
    let ab = b - a;
    let length = ab.length();
    if length < 1e-9 {
        return None;
    }
    let d = ab / length;
    let e = d - u * d.dot(u);
    if e.length() < 1e-6 {
        return None;
    }
    Some((u, e / e.length()))
}

const LINEAR_MIN_DEG: f64 = 15.0;
const ALIGNED_CONE_DEG: f64 = 20.0;

impl Dimension {
    pub fn new(a: DVec3, b: DVec3, offset: DVec3) -> Self {
        Dimension {
            id: next_id(),
            a0: a,
            b0: b,
            offset,
            axis: None,
            layer: None,
            text: None,
            anchor_a: None,
            anchor_b: None,
        }
    }

    pub fn with_axis(mut self, axis: Option<Axis>) -> Self {
        self.axis = axis;
        self
    }

    fn live(&self, doc: &DocView, which: End) -> DVec3 {
        let (anchor, frozen) = if which == End::A {
            (&self.anchor_a, self.a0)
        } else {
            (&self.anchor_b, self.b0)
        };
        let Some(anchor) = anchor else { return frozen };
        if let Some(p) = anchor.position(doc) {
            return p;
        }
        // The hold is gone — but the geometry may just have changed hands.
        resolve_vertex_anchor(doc, frozen, 1e-4)
            .and_then(|a| a.position(doc))
            .unwrap_or(frozen)
    }

    pub fn a(&self, doc: &DocView) -> DVec3 {
        self.live(doc, End::A)
    }

    pub fn b(&self, doc: &DocView) -> DVec3 {
        self.live(doc, End::B)
    }

    /// Anchor each endpoint to the vertex under it, if any.
    pub fn bind(&mut self, doc: &DocView) {
        self.anchor_a = resolve_vertex_anchor(doc, self.a0, 1e-4);
        self.anchor_b = resolve_vertex_anchor(doc, self.b0, 1e-4);
    }

    /// Re-take a hold that died; update the frozen positions from the live
    /// ones. The history calls this after every command.
    pub fn refresh_anchors(&mut self, doc: &DocView) {
        for which in [End::A, End::B] {
            let (anchor, frozen) = if which == End::A {
                (&mut self.anchor_a, &mut self.a0)
            } else {
                (&mut self.anchor_b, &mut self.b0)
            };
            let Some(an) = anchor.as_ref() else { continue };
            if let Some(p) = an.position(doc) {
                *frozen = p;
                continue;
            }
            *anchor = resolve_vertex_anchor(doc, *frozen, 1e-4);
            if let Some(p) = anchor.as_ref().and_then(|a| a.position(doc)) {
                *frozen = p;
            }
        }
    }

    pub fn anchored(&self) -> bool {
        self.anchor_a.is_some() && self.anchor_b.is_some()
    }

    pub fn display_text(&self, measured: &str) -> String {
        match &self.text {
            Some(t) if !t.is_empty() => t.replace("<>", measured),
            _ => measured.to_string(),
        }
    }

    /// Measured length: the segment's, or — linear — its extent along the
    /// axis.
    pub fn value_of(a: DVec3, b: DVec3, axis: Option<Axis>) -> f64 {
        match linear_frame(a, b, axis) {
            Some((u, _)) => (b - a).dot(u).abs(),
            None => (b - a).length(),
        }
    }

    pub fn value(&self, doc: &DocView) -> f64 {
        Self::value_of(self.a(doc), self.b(doc), self.axis)
    }

    pub fn label(&self, doc: &DocView) -> String {
        crate::units::fmt_len(self.value(doc))
    }

    /// The offset with any component ALONG the measured segment removed.
    pub fn perpendicular_offset_of(a: DVec3, b: DVec3, offset: DVec3, axis: Option<Axis>) -> DVec3 {
        if let Some((_, e)) = linear_frame(a, b, axis) {
            return e * offset.dot(e);
        }
        let ab = b - a;
        let length = ab.length();
        if length < 1e-9 {
            return offset;
        }
        let d = ab / length;
        offset - d * offset.dot(d)
    }

    /// The dimension line's endpoints.
    pub fn line_points_of(a: DVec3, b: DVec3, offset: DVec3, axis: Option<Axis>) -> (DVec3, DVec3) {
        if let Some((_, e)) = linear_frame(a, b, axis) {
            let t_a = offset.dot(e);
            let t_b = t_a - (b - a).dot(e);
            return (a + e * t_a, b + e * t_b);
        }
        let off = Self::perpendicular_offset_of(a, b, offset, axis);
        (a + off, b + off)
    }

    pub fn line_points(&self, doc: &DocView) -> (DVec3, DVec3) {
        Self::line_points_of(self.a(doc), self.b(doc), self.offset, self.axis)
    }

    pub fn midpoint(&self, doc: &DocView) -> DVec3 {
        let (p, q) = self.line_points(doc);
        (p + q) * 0.5
    }

    /// Offset placing the dimension line through `cursor`, parallel to a–b.
    pub fn offset_for_cursor(a: DVec3, b: DVec3, cursor: DVec3) -> DVec3 {
        let ab = b - a;
        let length = ab.length();
        if length < 1e-9 {
            return cursor - a;
        }
        let d = ab / length;
        let to_cursor = cursor - a;
        to_cursor - d * to_cursor.dot(d)
    }

    /// `(offset, axis)` for the cursor's position — which dimension the
    /// drafter is asking for, and where its line goes.
    pub fn placement_for_cursor(a: DVec3, b: DVec3, cursor: DVec3) -> (DVec3, Option<Axis>) {
        let ab = b - a;
        let length = ab.length();
        if length < 1e-9 {
            return (cursor - a, None);
        }
        let d = ab / length;
        let aligned = Self::offset_for_cursor(a, b, cursor);
        let w = cursor - (a + b) * 0.5;
        let along = w.dot(d).abs();
        let perp = (w - d * w.dot(d)).length();
        if along <= ALIGNED_CONE_DEG.to_radians().tan() * perp {
            return (aligned, None);
        }
        let lo = LINEAR_MIN_DEG.to_radians().sin();
        let hi = LINEAR_MIN_DEG.to_radians().cos();
        let mut best: Option<(f64, Axis, DVec3)> = None;
        let mut beyond = 0;
        for name in Axis::ALL {
            let Some((u, e)) = linear_frame(a, b, Some(name)) else {
                continue;
            };
            let along = d.dot(u).abs();
            if along < lo || along > hi {
                continue;
            }
            let t = (cursor - a).dot(e);
            let span = ab.dot(e);
            let (low, high) = (0f64.min(span), 0f64.max(span));
            let over = if t < low {
                low - t
            } else if t > high {
                t - high
            } else {
                0.0
            };
            if over <= 1e-9 {
                continue;
            }
            beyond += 1;
            if best.is_none_or(|(bo, _, _)| over > bo) {
                best = Some((over, name, e * t));
            }
        }
        match best {
            Some((_, name, off)) if beyond <= 1 => (off, Some(name)),
            _ => (aligned, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(v: Value) -> DimensionStyle {
        let mut s = DimensionStyle::default();
        s.merge_json(v.as_object().unwrap());
        s
    }

    #[test]
    fn style_keys_keep_their_order_and_unknown_ones_follow() {
        let s = merged(json!({"zeta": 1, "ends": "tick", "alpha": true}));
        let json = s.to_json();
        let keys: Vec<&str> = json.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "decimals",
                "units",
                "font_size",
                "color",
                "norma",
                "base_step_mm",
                "ends",
                "zeta",
                "alpha"
            ]
        );
        assert_eq!(merged(Value::Object(s.to_json())), s);
    }

    #[test]
    fn style_values_of_the_wrong_type_read_as_the_drawing_reads_them() {
        let s = merged(json!({
            "decimals": 3.9, "units": 5, "font_size": 12.7,
            "color": [300, -4, "x"], "base_step_mm": "wide", "ends": ""
        }));
        assert_eq!(s.decimals, 3);
        assert_eq!(s.units, "mm");
        assert_eq!(s.font_size, 12);
        assert_eq!(s.color, [255, 0, 75]);
        assert_eq!(s.base_step_mm, 8.0);
        assert_eq!(s.ends, "arrow");
        assert!(s.extra.is_empty());
    }
}
