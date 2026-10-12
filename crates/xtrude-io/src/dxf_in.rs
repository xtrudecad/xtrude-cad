// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! DXF import — the classic CAD import of push/pull modellers.
//!
//! - LINE / LWPOLYLINE / POLYLINE / ARC / CIRCLE / ELLIPSE / SPLINE come in
//!   as edges; curved entities are flattened and share ONE curve id each.
//! - Blocks become components: each used block builds ONE prototype mesh in
//!   local metres and every INSERT is a placement. INSERTs inside a block,
//!   and MINSERT arrays, explode in place.
//! - 3DFACE / SOLID / TRACE become faces, stitched and coplanar-merged.
//! - Text, dimensions, hatches and points are skipped (and counted).
//! - CAD layers become tagged groups.
//! - Units: `$INSUNITS` is only a claim; [`suggest_unit_scale`] measures
//!   the drawing and vetoes a header that makes no sense.
//! - Survey (UTM) coordinates far from the origin are recentred.
//!
//! Design note: this module reads the ASCII group-code stream itself
//! (leniently, like ezdxf's recover
//! mode) and reimplements ezdxf's path construction and adaptive flattening, so
//! curves flatten the same way.

use std::collections::HashSet;
use std::path::Path;

use indexmap::IndexMap;
use xtrude_core::math::{rotation_deg, DMat4, MatExt};
use xtrude_core::mesh::EdgeFlags;
use xtrude_core::stitch::run_stitch;
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::group::Group;
use xtrude_model::layers::Layer;
use xtrude_model::scene::Scene;

/// `$INSUNITS` code → metres per unit.
pub fn insunits_to_m(code: i64) -> Option<f64> {
    Some(match code {
        1 => 0.0254,
        2 => 0.3048,
        3 => 1609.344,
        4 => 0.001,
        5 => 0.01,
        6 => 1.0,
        7 => 1000.0,
        10 => 0.9144,
        _ => return None,
    })
}

/// Coordinates beyond this many metres from the origin are recentred.
pub const RECENTER_BEYOND_M: f64 = 100_000.0;
const SAGITTA_M: f64 = 0.01;
const SANE_MEDIAN_M: (f64, f64) = (0.05, 80.0);
const MAX_INSERT_DEPTH: usize = 8;
const CURVED: [&str; 4] = ["CIRCLE", "ARC", "ELLIPSE", "SPLINE"];
const FACES: [&str; 3] = ["3DFACE", "SOLID", "TRACE"];
const SKIP_SILENT: [&str; 4] = ["ATTDEF", "VIEWPORT", "XLINE", "RAY"];
const TAU: f64 = std::f64::consts::TAU;

#[derive(Debug, thiserror::Error)]
pub enum DxfError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(m: impl Into<String>) -> DxfError {
    DxfError::Invalid(m.into())
}

// ---- Reading the group-code stream ------------------------------------------

/// One DXF entity: its type and raw tags (POLYLINE carries its VERTEX
/// entities in `children`).
#[derive(Clone, Debug)]
pub struct Entity {
    pub kind: String,
    pub tags: Vec<(i32, String)>,
    pub children: Vec<Entity>,
}

impl Entity {
    fn get(&self, code: i32) -> Option<&str> {
        self.tags
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }

    fn f(&self, code: i32) -> Option<f64> {
        self.get(code).and_then(|v| v.trim().parse().ok())
    }

    fn fd(&self, code: i32, default: f64) -> f64 {
        self.f(code).unwrap_or(default)
    }

    fn i(&self, code: i32) -> Option<i64> {
        self.get(code).and_then(|v| v.trim().parse().ok())
    }

    fn point(&self, code: i32) -> Option<DVec3> {
        Some(DVec3::new(
            self.f(code)?,
            self.f(code + 10)?,
            self.f(code + 20).unwrap_or(0.0),
        ))
    }

    fn layer(&self) -> String {
        match self.get(8).map(str::trim) {
            Some(l) if !l.is_empty() => decode_unicode_escapes(l),
            _ => "0".into(),
        }
    }

    fn extrusion(&self) -> DVec3 {
        self.point(210).unwrap_or(DVec3::Z)
    }
}

/// A block definition.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub base: DVec3,
    pub entities: Vec<Entity>,
}

/// A parsed DXF document (the parts the importer reads).
#[derive(Clone, Debug, Default)]
pub struct DxfDoc {
    pub header: IndexMap<String, Vec<(i32, String)>>,
    pub entities: Vec<Entity>,
    pub blocks: IndexMap<String, Block>,
}

impl DxfDoc {
    /// A header variable's first value.
    pub fn header_int(&self, name: &str) -> Option<i64> {
        self.header.get(name)?.first()?.1.trim().parse().ok()
    }
}

/// `\U+XXXX` escapes (how DXF writes non-ASCII names).
fn decode_unicode_escapes(s: &str) -> String {
    if !s.contains("\\U+") {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\U+") {
        out.push_str(&rest[..i]);
        let hex = rest.get(i + 3..i + 7).unwrap_or("");
        match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
            Some(c) if hex.len() == 4 => {
                out.push(c);
                rest = &rest[i + 7..];
            }
            _ => {
                out.push_str("\\U+");
                rest = &rest[i + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn read_tags(text: &str) -> Vec<(i32, String)> {
    let mut lines = text.lines();
    let mut tags = Vec::new();
    while let Some(code) = lines.next() {
        let Some(value) = lines.next() else { break };
        let Ok(code) = code.trim().parse::<i32>() else {
            continue;
        };
        tags.push((code, value.trim_end_matches('\r').to_string()));
    }
    tags
}

fn group_entities(tags: &[(i32, String)], end_markers: &[&str]) -> (Vec<Entity>, usize) {
    // tags start at a (0, TYPE) pair; returns the entities and how many tags
    // were consumed (stopping before an end marker)
    let mut out: Vec<Entity> = Vec::new();
    let mut i = 0;
    while i < tags.len() {
        let (c, v) = &tags[i];
        if *c != 0 {
            i += 1;
            continue;
        }
        let kind = v.trim().to_uppercase();
        if end_markers.contains(&kind.as_str()) {
            return (out, i);
        }
        let mut j = i + 1;
        while j < tags.len() && tags[j].0 != 0 {
            j += 1;
        }
        let ent = Entity {
            kind: kind.clone(),
            tags: tags[i + 1..j].to_vec(),
            children: Vec::new(),
        };
        i = j;
        match kind.as_str() {
            "VERTEX" | "SEQEND" | "ATTRIB" => {
                // attach VERTEX to the open POLYLINE; drop ATTRIB / SEQEND
                if kind == "VERTEX" {
                    if let Some(p) = out.last_mut().filter(|e| e.kind == "POLYLINE") {
                        p.children.push(ent);
                    }
                }
            }
            _ => out.push(ent),
        }
    }
    (out, i)
}

/// Read a DXF file (ASCII; UTF-8 or a single-byte code page).
pub fn open_document(path: &Path) -> Result<DxfDoc, DxfError> {
    let raw = std::fs::read(path)?;
    if raw.starts_with(b"AutoCAD Binary DXF") {
        return Err(invalid(
            "binary DXF is not supported — save it as ASCII DXF",
        ));
    }
    let text = match String::from_utf8(raw) {
        Ok(t) => t,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    };
    let tags = read_tags(text.trim_start_matches('\u{feff}'));
    let mut doc = DxfDoc::default();
    let mut i = 0;
    while i < tags.len() {
        if !(tags[i].0 == 0 && tags[i].1.trim() == "SECTION") {
            i += 1;
            continue;
        }
        let name = tags
            .get(i + 1)
            .map(|t| t.1.trim().to_uppercase())
            .unwrap_or_default();
        let start = i + 2;
        let mut end = start;
        while end < tags.len() && !(tags[end].0 == 0 && tags[end].1.trim() == "ENDSEC") {
            end += 1;
        }
        let body = &tags[start..end.min(tags.len())];
        match name.as_str() {
            "HEADER" => {
                let mut cur: Option<String> = None;
                for (c, v) in body {
                    if *c == 9 {
                        cur = Some(v.trim().to_string());
                        doc.header.entry(v.trim().to_string()).or_default();
                    } else if let Some(n) = &cur {
                        doc.header
                            .entry(n.clone())
                            .or_default()
                            .push((*c, v.clone()));
                    }
                }
            }
            "ENTITIES" => doc.entities = group_entities(body, &[]).0,
            "BLOCKS" => {
                let mut k = 0;
                while k < body.len() {
                    if !(body[k].0 == 0 && body[k].1.trim() == "BLOCK") {
                        k += 1;
                        continue;
                    }
                    let mut h = k + 1;
                    while h < body.len() && body[h].0 != 0 {
                        h += 1;
                    }
                    let head = Entity {
                        kind: "BLOCK".into(),
                        tags: body[k + 1..h].to_vec(),
                        children: Vec::new(),
                    };
                    let (ents, used) = group_entities(&body[h..], &["ENDBLK"]);
                    let name = decode_unicode_escapes(head.get(2).unwrap_or("").trim());
                    doc.blocks.insert(
                        name,
                        Block {
                            base: head.point(10).unwrap_or(DVec3::ZERO),
                            entities: ents,
                        },
                    );
                    k = h + used + 1;
                }
            }
            _ => {}
        }
        i = end + 1;
    }
    Ok(doc)
}

/// `(metres_per_unit, code)` from the header; `(None, 0)` when unitless.
pub fn read_unit_scale(doc: &DxfDoc) -> (Option<f64>, i64) {
    let code = doc.header_int("$INSUNITS").unwrap_or(0);
    (insunits_to_m(code), code)
}

/// The header's declared unit unless the drawing's own content (median
/// entity size) rules it out; then the first sane candidate.
pub fn suggest_unit_scale(doc: &DxfDoc) -> (Option<f64>, i64) {
    let (header_scale, code) = read_unit_scale(doc);
    let mut sizes: Vec<f64> = Vec::new();
    for e in &doc.entities {
        if sizes.len() >= 4000 {
            break;
        }
        match e.kind.as_str() {
            "LINE" => {
                if let (Some(a), Some(b)) = (e.point(10), e.point(11)) {
                    sizes.push((b - a).length());
                }
            }
            "CIRCLE" | "ARC" => {
                if let Some(r) = e.f(40) {
                    sizes.push(2.0 * r);
                }
            }
            "LWPOLYLINE" => {
                let pts = lw_points(e);
                for w in pts.windows(2) {
                    sizes.push((w[1].0 - w[0].0).hypot(w[1].1 - w[0].1));
                }
            }
            _ => {}
        }
    }
    sizes.retain(|&s| s > 1e-9);
    if sizes.is_empty() {
        return (header_scale, code);
    }
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sizes[sizes.len() / 2];
    let sane = |s: f64| SANE_MEDIAN_M.0 <= median * s && median * s <= SANE_MEDIAN_M.1;
    if let Some(h) = header_scale.filter(|&h| sane(h)) {
        return (Some(h), code);
    }
    for c in [1.0, 0.01, 0.001, 0.0254, 0.3048] {
        if sane(c) {
            return (Some(c), code);
        }
    }
    (header_scale, code)
}

// ---- Geometry: ezdxf's paths and flattening ---------------------------------

fn isclose(a: f64, b: f64, rel: f64, abs: f64) -> bool {
    a == b || (a - b).abs() <= (rel * a.abs().max(b.abs())).max(abs)
}

fn vclose(a: DVec3, b: DVec3, rel: f64, abs: f64) -> bool {
    isclose(a.x, b.x, rel, abs) && isclose(a.y, b.y, rel, abs) && isclose(a.z, b.z, rel, abs)
}

fn vclose_default(a: DVec3, b: DVec3) -> bool {
    vclose(a, b, 1e-9, 1e-12)
}

const IS_CLOSE_TOL: f64 = 1e-10;

#[derive(Clone, Copy, Debug)]
enum Seg {
    Line(DVec3),
    Curve(DVec3, DVec3, DVec3),
}

#[derive(Clone, Debug, Default)]
struct PathB {
    start: DVec3,
    segs: Vec<Seg>,
}

impl PathB {
    fn new(start: DVec3) -> Self {
        PathB {
            start,
            segs: Vec::new(),
        }
    }
    fn end(&self) -> DVec3 {
        match self.segs.last() {
            Some(Seg::Line(e)) | Some(Seg::Curve(_, _, e)) => *e,
            None => self.start,
        }
    }
    fn line_to(&mut self, p: DVec3) {
        self.segs.push(Seg::Line(p));
    }
    fn map(&mut self, f: impl Fn(DVec3) -> DVec3) {
        self.start = f(self.start);
        for s in self.segs.iter_mut() {
            *s = match *s {
                Seg::Line(e) => Seg::Line(f(e)),
                Seg::Curve(a, b, e) => Seg::Curve(f(a), f(b), f(e)),
            };
        }
    }
    /// `Path.flattening(distance, segments)`.
    fn flattening(&self, distance: f64, segments: usize) -> Vec<DVec3> {
        if self.segs.is_empty() {
            return Vec::new();
        }
        let mut out = vec![self.start];
        let mut start = self.start;
        for s in &self.segs {
            match *s {
                Seg::Line(e) => {
                    out.push(e);
                    start = e;
                }
                Seg::Curve(c1, c2, e) => {
                    let pts = bezier_flattening([start, c1, c2, e], distance, segments);
                    out.extend(pts.into_iter().skip(1));
                    start = e;
                }
            }
        }
        out
    }
}

fn bezier_point(cp: &[DVec3; 4], t: f64) -> DVec3 {
    let u = 1.0 - t;
    cp[0] * (u * u * u)
        + cp[1] * (3.0 * u * u * t)
        + cp[2] * (3.0 * u * t * t)
        + cp[3] * (t * t * t)
}

/// `Bezier4P.flattening(distance, segments)`.
fn bezier_flattening(cp: [DVec3; 4], distance: f64, segments: usize) -> Vec<DVec3> {
    let mut out = vec![cp[0]];
    let dt = 1.0 / segments.max(1) as f64;
    let mut t0 = 0.0;
    let mut start = cp[0];
    let mut stack: Vec<(f64, DVec3)> = Vec::new();
    let mut guard = 0usize;
    while t0 < 1.0 {
        let mut t1 = t0 + dt;
        let mut end = if isclose(t1, 1.0, 1e-9, 0.0) {
            t1 = 1.0;
            cp[3]
        } else {
            bezier_point(&cp, t1)
        };
        loop {
            guard += 1;
            if guard > 1_000_000 {
                return out; // degenerate input: never spin
            }
            let mid_t = (t0 + t1) * 0.5;
            let mid = bezier_point(&cp, mid_t);
            let chk = start.lerp(end, 0.5);
            if chk.distance(mid) < distance {
                out.push(end);
                t0 = t1;
                start = end;
                match stack.pop() {
                    Some((t, e)) => {
                        t1 = t;
                        end = e;
                    }
                    None => break,
                }
            } else {
                stack.push((t1, end));
                t1 = mid_t;
                end = mid;
            }
        }
    }
    out
}

fn reverse_curves(curves: &[[DVec3; 4]]) -> Vec<[DVec3; 4]> {
    curves
        .iter()
        .rev()
        .map(|c| [c[3], c[2], c[1], c[0]])
        .collect()
}

/// `tools.add_bezier4p`.
fn add_bezier4p(path: &mut PathB, curves: Vec<[DVec3; 4]>) {
    if curves.is_empty() {
        return;
    }
    let mut curves = curves;
    if vclose_default(path.end(), curves[curves.len() - 1][3]) {
        curves = reverse_curves(&curves);
    }
    for [s, c1, c2, e] in curves {
        if !vclose_default(s, path.end()) {
            path.line_to(s);
        }
        if vclose(s, c1, 1e-15, 0.0) && vclose(e, c2, 1e-15, 0.0) {
            path.line_to(e);
        } else {
            path.segs.push(Seg::Curve(c1, c2, e));
        }
    }
}

/// `cubic_bezier_arc_parameters` for the unit circle.
fn arc_parameters(start: f64, end: f64, segments: usize) -> Vec<[DVec3; 4]> {
    let delta = end - start;
    if delta <= 0.0 {
        return Vec::new();
    }
    let count = ((delta / std::f64::consts::PI * 2.0).ceil() as usize).max(segments);
    let seg_angle = delta / count as f64;
    let tl = 4.0 / 3.0 * (seg_angle / 4.0).tan();
    let mut angle = start;
    let from_angle = |a: f64| DVec3::new(a.cos(), a.sin(), 0.0);
    let mut end_p = from_angle(angle);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let s = end_p;
        angle += seg_angle;
        end_p = from_angle(angle);
        let c1 = s + DVec3::new(-s.y * tl, s.x * tl, 0.0);
        let c2 = end_p + DVec3::new(end_p.y * tl, -end_p.x * tl, 0.0);
        out.push([s, c1, c2, end_p]);
    }
    out
}

fn arc_angle_span_rad(start: f64, end: f64) -> f64 {
    const RAD_ABS_TOL: f64 = 1e-15;
    if isclose(start, end, 1e-9, RAD_ABS_TOL) {
        return 0.0;
    }
    let s = start.rem_euclid(TAU);
    if isclose(s, end.rem_euclid(TAU), 1e-9, RAD_ABS_TOL) {
        return TAU;
    }
    let mut e = if isclose(end, TAU, 1e-9, RAD_ABS_TOL) {
        end
    } else {
        end.rem_euclid(TAU)
    };
    if e < s {
        e += TAU;
    }
    e - s
}

/// `ConstructionEllipse`.
#[derive(Clone, Copy, Debug)]
struct Ellipse {
    center: DVec3,
    major: DVec3,
    extrusion: DVec3,
    ratio: f64,
    start_param: f64,
    end_param: f64,
}

impl Ellipse {
    fn minor(&self) -> DVec3 {
        let d = self.extrusion.cross(self.major);
        let len = d.length();
        if len == 0.0 {
            return DVec3::ZERO;
        }
        d / len * (self.major.length() * self.ratio)
    }
    fn vertex(&self, param: f64) -> DVec3 {
        let xa = self.major.normalize_or_zero();
        let ya = self.minor().normalize_or_zero();
        let rx = self.major.length();
        let ry = rx * self.ratio;
        self.center + xa * (param.cos() * rx) + ya * (param.sin() * ry)
    }
    fn from_arc(
        center: DVec3,
        radius: f64,
        extrusion: DVec3,
        start_deg: f64,
        end_deg: f64,
    ) -> Ellipse {
        let ocs = Ocs::new(extrusion);
        Ellipse {
            center: ocs.to_wcs(center),
            major: ocs.to_wcs(DVec3::new(radius.abs(), 0.0, 0.0)),
            extrusion,
            ratio: 1.0,
            start_param: start_deg.to_radians(),
            end_param: end_deg.to_radians(),
        }
    }
    fn beziers(&self, segments: usize) -> Vec<[DVec3; 4]> {
        let span = arc_angle_span_rad(self.start_param, self.end_param);
        if span.abs() < 1e-9 {
            return Vec::new();
        }
        let start = self.start_param.rem_euclid(TAU);
        let mut end = start + span;
        while start > end {
            end += TAU;
        }
        let (c, x, y) = (self.center, self.major, self.minor());
        arc_parameters(start, end, segments)
            .into_iter()
            .map(|cp| cp.map(|p| c + x * p.x + y * p.y))
            .collect()
    }
}

/// `tools.add_ellipse(path, ellipse, segments, reset=True)` on a new path.
fn ellipse_path(e: &Ellipse, segments: usize) -> PathB {
    let mut path = PathB::new(DVec3::ZERO);
    if arc_angle_span_rad(e.start_param, e.end_param).abs() < 1e-9 {
        return path;
    }
    path.start = e.vertex(e.start_param);
    add_bezier4p(&mut path, e.beziers(segments));
    path
}

/// The object coordinate system of an extrusion vector.
struct Ocs {
    transform: bool,
    ax: DVec3,
    ay: DVec3,
    az: DVec3,
}

impl Ocs {
    fn new(extrusion: DVec3) -> Ocs {
        let az = extrusion.normalize_or_zero();
        let transform = !vclose_default(az, DVec3::Z);
        if !transform {
            return Ocs {
                transform,
                ax: DVec3::X,
                ay: DVec3::Y,
                az: DVec3::Z,
            };
        }
        let ax = if az.x.abs() < 1.0 / 64.0 && az.y.abs() < 1.0 / 64.0 {
            DVec3::Y.cross(az)
        } else {
            DVec3::Z.cross(az)
        }
        .normalize_or_zero();
        let ay = az.cross(ax).normalize_or_zero();
        Ocs {
            transform,
            ax,
            ay,
            az,
        }
    }
    fn to_wcs(&self, p: DVec3) -> DVec3 {
        if self.transform {
            self.ax * p.x + self.ay * p.y + self.az * p.z
        } else {
            p
        }
    }
}

fn angle2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.1 - a.1).atan2(b.0 - a.0)
}

/// `bulge_to_arc`: `(center, start_angle, end_angle, radius)`, CCW.
fn bulge_to_arc(p1: (f64, f64), p2: (f64, f64), bulge: f64) -> ((f64, f64), f64, f64, f64) {
    let d = (p2.0 - p1.0).hypot(p2.1 - p1.1);
    let r = d * (1.0 + bulge * bulge) / 4.0 / bulge;
    let a = angle2(p1, p2) + (std::f64::consts::FRAC_PI_2 - bulge.atan() * 2.0);
    let c = (p1.0 + a.cos() * r, p1.1 + a.sin() * r);
    if bulge < 0.0 {
        (c, angle2(c, p2), angle2(c, p1), r.abs())
    } else {
        (c, angle2(c, p1), angle2(c, p2), r.abs())
    }
}

/// `tools.add_2d_polyline` on a new path.
fn polyline_path(
    points: &[(f64, f64, f64)],
    close: bool,
    extrusion: DVec3,
    elevation: f64,
) -> PathB {
    let mut path = PathB::new(DVec3::ZERO);
    let bulge_to = |path: &mut PathB, p1: DVec3, p2: DVec3, bulge: f64| {
        if vclose(p1, p2, IS_CLOSE_TOL, 0.0) {
            return;
        }
        let (c, sa, ea, r) = bulge_to_arc((p1.x, p1.y), (p2.x, p2.y), bulge);
        let sa = sa.rem_euclid(TAU);
        let mut ea = ea.rem_euclid(TAU);
        if sa > ea {
            ea += TAU;
        }
        let e = Ellipse::from_arc(
            DVec3::new(c.0, c.1, 0.0),
            r,
            DVec3::Z,
            sa.to_degrees(),
            ea.to_degrees(),
        );
        let mut curves = e.beziers(1);
        if curves
            .first()
            .is_some_and(|c0| vclose(c0[0], p2, IS_CLOSE_TOL, 0.0))
        {
            curves = reverse_curves(&curves);
        }
        add_bezier4p(path, curves);
    };
    let mut prev: Option<DVec3> = None;
    let mut prev_bulge = 0.0;
    for &(x, y, b) in points {
        let b = if b.abs() < 1e-6 { 0.0 } else { b };
        let p = DVec3::new(x, y, 0.0);
        match prev {
            None => path.start = p,
            Some(pp) => {
                if prev_bulge != 0.0 {
                    bulge_to(&mut path, pp, p, prev_bulge);
                } else {
                    path.line_to(p);
                }
            }
        }
        prev = Some(p);
        prev_bulge = b;
    }
    if prev.is_none() {
        return PathB::new(DVec3::ZERO);
    }
    if close && !vclose(path.start, path.end(), IS_CLOSE_TOL, 0.0) {
        let (s, e) = (path.start, path.end());
        if prev_bulge != 0.0 {
            bulge_to(&mut path, e, s, prev_bulge);
        } else {
            path.line_to(s);
        }
    }
    let ocs = Ocs::new(extrusion);
    if ocs.transform || elevation != 0.0 {
        path.map(|v| ocs.to_wcs(DVec3::new(v.x, v.y, elevation)));
    }
    path
}

fn lw_points(e: &Entity) -> Vec<(f64, f64, f64)> {
    let mut pts: Vec<(f64, f64, f64)> = Vec::new();
    let mut x: Option<f64> = None;
    for (c, v) in &e.tags {
        let val = || v.trim().parse::<f64>().unwrap_or(0.0);
        match c {
            10 => x = Some(val()),
            20 => {
                if let Some(xv) = x.take() {
                    pts.push((xv, val(), 0.0));
                }
            }
            42 => {
                if let Some(last) = pts.last_mut() {
                    last.2 = val();
                }
            }
            _ => {}
        }
    }
    pts
}

/// A SPLINE as a polyline of curve samples: degree-3 non-rational clamped
/// splines decompose into exact Béziers, others are sampled densely.
fn spline_path(e: &Entity) -> Option<PathB> {
    let degree = e.i(71).unwrap_or(3).max(1) as usize;
    let mut knots: Vec<f64> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();
    let mut ctrl: Vec<DVec3> = Vec::new();
    let mut fit: Vec<DVec3> = Vec::new();
    let (mut cx, mut cy, mut fx, mut fy) = (None, None, None, None);
    for (c, v) in &e.tags {
        let val = v.trim().parse::<f64>().unwrap_or(0.0);
        match c {
            40 => knots.push(val),
            41 => weights.push(val),
            10 => cx = Some(val),
            20 => cy = Some(val),
            30 => {
                if let (Some(x), Some(y)) = (cx.take(), cy.take()) {
                    ctrl.push(DVec3::new(x, y, val));
                }
            }
            11 => fx = Some(val),
            21 => fy = Some(val),
            31 => {
                if let (Some(x), Some(y)) = (fx.take(), fy.take()) {
                    fit.push(DVec3::new(x, y, val));
                }
            }
            _ => {}
        }
    }
    if ctrl.is_empty() && fit.len() >= 2 {
        // fit points only: a Catmull-Rom cubic through them
        let mut path = PathB::new(fit[0]);
        let n = fit.len();
        for i in 0..n - 1 {
            let p0 = if i == 0 { fit[0] } else { fit[i - 1] };
            let p3 = if i + 2 < n { fit[i + 2] } else { fit[n - 1] };
            let (p1, p2) = (fit[i], fit[i + 1]);
            let c1 = p1 + (p2 - p0) / 6.0;
            let c2 = p2 - (p3 - p1) / 6.0;
            path.segs.push(Seg::Curve(c1, c2, p2));
        }
        return Some(path);
    }
    let n = ctrl.len();
    if n < 2 {
        return None;
    }
    let degree = degree.min(n - 1);
    if knots.len() != n + degree + 1 {
        // a clamped uniform knot vector
        knots = (0..n + degree + 1)
            .map(|i| {
                if i <= degree {
                    0.0
                } else if i >= n {
                    (n - degree) as f64
                } else {
                    (i - degree) as f64
                }
            })
            .collect();
    }
    if weights.len() != n {
        weights = vec![1.0; n];
    }
    let rational = weights.iter().any(|&w| (w - 1.0).abs() > 1e-12);
    let eval = |t: f64| -> DVec3 {
        // de Boor on homogeneous points
        let mut k = degree;
        while k + 1 < n && knots[k + 1] <= t {
            k += 1;
        }
        let mut d: Vec<(DVec3, f64)> = (0..=degree)
            .map(|j| {
                (
                    ctrl[j + k - degree] * weights[j + k - degree],
                    weights[j + k - degree],
                )
            })
            .collect();
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = j + k - degree;
                let den = knots[i + 1 + degree - r] - knots[i];
                let a = if den.abs() < 1e-300 {
                    0.0
                } else {
                    (t - knots[i]) / den
                };
                d[j] = (
                    d[j - 1].0 * (1.0 - a) + d[j].0 * a,
                    d[j - 1].1 * (1.0 - a) + d[j].1 * a,
                );
            }
        }
        let (p, w) = d[degree];
        if w.abs() < 1e-300 {
            p
        } else {
            p / w
        }
    };
    let (t0, t1) = (knots[degree], knots[n]);
    let mut path = PathB::new(eval(t0));
    let samples = if rational || degree != 3 {
        (n - 1) * 16
    } else {
        (n - degree) * 24
    }
    .max(8);
    for s in 1..=samples {
        let t = t0 + (t1 - t0) * s as f64 / samples as f64;
        path.line_to(eval(t.min(t1 - 1e-12 * (t1 - t0).abs()).max(t0)));
    }
    if let Some(Seg::Line(e)) = path.segs.last_mut() {
        *e = if (t1 - t0).abs() > 0.0 {
            ctrl[n - 1]
        } else {
            *e
        };
        if rational || knots[n] != knots[n + degree] {
            *e = eval(t1 - 1e-12 * (t1 - t0).abs());
        }
    }
    Some(path)
}

/// The linework path of an entity, `None` for kinds that are not linework,
/// `Some(None)` when the entity is linework but yields no path.
fn entity_path(e: &Entity) -> Option<Option<PathB>> {
    match e.kind.as_str() {
        "LWPOLYLINE" => {
            let closed = e.i(70).unwrap_or(0) & 1 == 1;
            Some(Some(polyline_path(
                &lw_points(e),
                closed,
                e.extrusion(),
                e.fd(38, 0.0),
            )))
        }
        "POLYLINE" => {
            let flags = e.i(70).unwrap_or(0);
            if flags & (16 | 64) != 0 {
                return Some(None); // polygon / polyface mesh
            }
            if e.children.is_empty() {
                return Some(Some(PathB::default()));
            }
            let verts: Vec<(DVec3, f64)> = e
                .children
                .iter()
                .map(|v| (v.point(10).unwrap_or(DVec3::ZERO), v.fd(42, 0.0)))
                .collect();
            let closed = flags & 1 == 1;
            if flags & 8 != 0 {
                // 3D polyline: straight segments through the points
                let mut path = PathB::new(verts[0].0);
                for &(p, _) in &verts[1..] {
                    if !vclose_default(p, path.end()) {
                        path.line_to(p);
                    }
                }
                if closed && !vclose_default(path.start, path.end()) {
                    let s = path.start;
                    path.line_to(s);
                }
                return Some(Some(path));
            }
            let elevation = e.point(10).map(|p| p.z).unwrap_or(verts[0].0.z);
            let pts: Vec<(f64, f64, f64)> = verts.iter().map(|(p, b)| (p.x, p.y, *b)).collect();
            Some(Some(polyline_path(&pts, closed, e.extrusion(), elevation)))
        }
        "ARC" | "CIRCLE" => {
            let r = e.fd(40, 0.0).abs();
            if r <= 1e-12 {
                return Some(Some(PathB::default()));
            }
            let (sa, ea) = if e.kind == "ARC" {
                (e.fd(50, 0.0), e.fd(51, 360.0))
            } else {
                (0.0, 360.0)
            };
            let ell =
                Ellipse::from_arc(e.point(10).unwrap_or(DVec3::ZERO), r, e.extrusion(), sa, ea);
            Some(Some(ellipse_path(&ell, 1)))
        }
        "ELLIPSE" => {
            let ell = Ellipse {
                center: e.point(10).unwrap_or(DVec3::ZERO),
                major: e.point(11).unwrap_or(DVec3::X),
                extrusion: e.extrusion(),
                ratio: e.fd(40, 1.0),
                start_param: e.fd(41, 0.0),
                end_param: e.fd(42, TAU),
            };
            Some(Some(ellipse_path(&ell, 1)))
        }
        "SPLINE" => Some(spline_path(e)),
        _ => None,
    }
}

/// The corner loop of a face entity (SOLID/TRACE corners 2 and 3 swap to
/// walk the perimeter).
fn entity_face(e: &Entity, m: Option<&DMat4>) -> Option<Vec<DVec3>> {
    if !FACES.contains(&e.kind.as_str()) {
        return None;
    }
    let v0 = e.point(10).unwrap_or(DVec3::ZERO);
    let v1 = e.point(11).unwrap_or(DVec3::ZERO);
    let v2 = e.point(12).unwrap_or(DVec3::ZERO);
    let v3 = e.point(13).unwrap_or(v2);
    let pts = if e.kind == "3DFACE" {
        let mut p = vec![v0, v1, v2];
        if (v3 - v2).length() > 1e-12 {
            p.push(v3);
        }
        p
    } else {
        let p = vec![v0, v1, v3, v2];
        if (p[2] - p[3]).length() < 1e-12 {
            p[..3].to_vec()
        } else {
            p
        }
    };
    let mut seen: Vec<DVec3> = Vec::new();
    for p in pts {
        let p = m.map_or(p, |m| m.project_point(p));
        if seen.last().is_none_or(|l| (p - *l).length() > 1e-12) {
            seen.push(p);
        }
    }
    if seen.len() >= 3 {
        Some(seen)
    } else {
        None
    }
}

/// The block reference matrix (`Insert.matrix44`) in column-vector form,
/// before the array offset of a MINSERT cell.
fn insert_matrix(e: &Entity, base: DVec3, cell: (usize, usize)) -> DMat4 {
    let (sx, sy, sz) = (e.fd(41, 1.0), e.fd(42, 1.0), e.fd(43, 1.0));
    let ocs = Ocs::new(e.extrusion());
    let (ux, uy, uz) = (ocs.to_wcs(DVec3::X), ocs.to_wcs(DVec3::Y), ocs.az);
    let mut m = DMat4::from_cols(
        (ux * sx).extend(0.0),
        (uy * sy).extend(0.0),
        (uz * sz).extend(0.0),
        glam::DVec4::W,
    );
    let rotation = e.fd(50, 0.0);
    if rotation != 0.0 {
        m = rotation_deg(uz, rotation) * m;
    }
    let mut insert_ocs = e.point(10).unwrap_or(DVec3::ZERO);
    let (row, col) = cell;
    if row > 0 || col > 0 {
        let off = DVec3::new(col as f64 * e.fd(44, 0.0), row as f64 * e.fd(45, 0.0), 0.0);
        let off = if rotation != 0.0 {
            rotation_deg(DVec3::Z, rotation).transform_vector(off)
        } else {
            off
        };
        insert_ocs += off;
    }
    let insert = ocs.to_wcs(insert_ocs) - m.transform_vector(base);
    m.w_axis = insert.extend(1.0);
    m
}

fn insert_cells(e: &Entity) -> Vec<(usize, usize)> {
    let rows = e.i(71).unwrap_or(1).max(1) as usize;
    let cols = e.i(70).unwrap_or(1).max(1) as usize;
    let (dx, dy) = (e.fd(44, 0.0), e.fd(45, 0.0));
    let mut done: Vec<(i64, i64)> = Vec::new();
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let key = (
                (c as f64 * dx).to_bits() as i64,
                (r as f64 * dy).to_bits() as i64,
            );
            if !done.contains(&key) {
                done.push(key);
                out.push((r, c));
            }
        }
    }
    out
}

// ---- Collecting -------------------------------------------------------------

#[derive(Default)]
struct Bucket {
    runs: Vec<(Vec<DVec3>, bool)>,
    faces: Vec<Vec<DVec3>>,
}

/// What [`Ctx::flat`] hands each flattened entity to: the entity, its
/// INSERT transform and its layer; returns whether it counted.
type FlatSink<'s, 'a> = dyn FnMut(&mut Ctx<'a>, &Entity, Option<&DMat4>, String) -> bool + 's;

struct Ctx<'a> {
    doc: &'a DxfDoc,
    sagitta: f64,
    skipped: IndexMap<String, usize>,
}

impl<'a> Ctx<'a> {
    /// One entity (transformed by `m`) into `bucket`; returns whether it
    /// counted as an imported entity.
    fn bucket_one(&mut self, e: &Entity, m: Option<&DMat4>, bucket: &mut Bucket) -> bool {
        if let Some(lp) = entity_face(e, m) {
            bucket.faces.push(lp);
            return true;
        }
        if FACES.contains(&e.kind.as_str()) {
            return false;
        }
        if e.kind == "LINE" {
            let (a, b) = (
                e.point(10).unwrap_or(DVec3::ZERO),
                e.point(11).unwrap_or(DVec3::ZERO),
            );
            let tf = |p: DVec3| m.map_or(p, |m| m.project_point(p));
            bucket.runs.push((vec![tf(a), tf(b)], false));
            return true;
        }
        match entity_path(e) {
            None => {
                if !SKIP_SILENT.contains(&e.kind.as_str()) {
                    *self.skipped.entry(e.kind.clone()).or_insert(0) += 1;
                }
                false
            }
            Some(None) => false,
            Some(Some(mut path)) => {
                if let Some(m) = m {
                    path.map(|p| m.project_point(p));
                }
                let pts = path.flattening(self.sagitta, 8);
                if pts.len() < 2 {
                    return false;
                }
                let curved = CURVED.contains(&e.kind.as_str())
                    || match e.kind.as_str() {
                        "LWPOLYLINE" => lw_points(e).iter().any(|p| p.2 != 0.0),
                        "POLYLINE" => e.children.iter().any(|v| v.fd(42, 0.0).abs() > 1e-12),
                        _ => false,
                    };
                bucket.runs.push((pts, curved));
                true
            }
        }
    }

    /// Entities with INSERTs expanded in place (block contents, MINSERT
    /// arrays); a child on layer "0" takes its INSERT's layer.
    fn flat(
        &mut self,
        ents: &[Entity],
        m: Option<DMat4>,
        layer: Option<&str>,
        depth: usize,
        sink: &mut FlatSink<'_, 'a>,
    ) -> usize {
        let mut counted = 0;
        for e in ents {
            let own = e.layer();
            let lay = match layer {
                Some(l) if own == "0" => l.to_string(),
                _ => own.clone(),
            };
            if e.kind == "INSERT" && depth < MAX_INSERT_DEPTH {
                let name = decode_unicode_escapes(e.get(2).unwrap_or("").trim());
                let Some(block) = self.doc.blocks.get(&name).cloned() else {
                    continue;
                };
                for cell in insert_cells(e) {
                    let local = insert_matrix(e, block.base, cell);
                    let world = m.map_or(local, |m| m * local);
                    counted += self.flat(&block.entities, Some(world), Some(&lay), depth + 1, sink);
                }
                continue;
            }
            if sink(self, e, m.as_ref(), lay) {
                counted += 1;
            }
        }
        counted
    }
}

/// One welded mesh from a bucket, in metres, recentred by `offset`; faces
/// stitch and coplanar-merge afterwards.
fn build_mesh(bucket: &Bucket, scale: f64, offset: DVec3) -> Mesh {
    let mut mesh = Mesh::new();
    let mut pts: Vec<DVec3> = Vec::new();
    let (mut ia, mut ib, mut flags) = (Vec::new(), Vec::new(), Vec::new());
    for (run, curved) in &bucket.runs {
        let cid = if *curved && run.len() > 2 {
            Some(Mesh::next_curve_id())
        } else {
            None
        };
        let base = pts.len();
        pts.extend(run.iter().map(|p| *p * scale - offset));
        for k in 0..run.len() - 1 {
            ia.push(base + k);
            ib.push(base + k + 1);
            flags.push(EdgeFlags {
                soft: false,
                curve: cid,
                layer: None,
                hidden: false,
            });
        }
    }
    if !pts.is_empty() {
        let (vobjs, ids) = mesh.bulk_weld(&pts);
        let ia: Vec<usize> = ia.iter().map(|&i| ids[i]).collect();
        let ib: Vec<usize> = ib.iter().map(|&i| ids[i]).collect();
        mesh.add_edges_welded(&vobjs, &ia, &ib, Some(&flags));
    }
    if !bucket.faces.is_empty() {
        let mut added: HashSet<FaceId> = HashSet::new();
        let mut seed = Vec::new();
        for lp in &bucket.faces {
            let corners: Vec<DVec3> = lp.iter().map(|p| *p * scale - offset).collect();
            let f = mesh.add_face(&corners, &[]);
            added.insert(f);
            seed.extend(corners);
        }
        if !added.is_empty() {
            run_stitch(&mut mesh, &seed, Some(&mut added), true, true);
        }
    }
    mesh
}

/// What an import made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfStats {
    pub entities: usize,
    pub edges: usize,
    pub faces: usize,
    pub groups: usize,
    pub components: IndexMap<String, usize>,
    pub skipped: IndexMap<String, usize>,
    pub offset: Option<DVec3>,
}

/// Progress callback: `(fraction, text)`.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str);

fn empty_error() -> DxfError {
    invalid("no importable geometry (lines, polylines, arcs, circles, ellipses, splines or faces) in the model space")
}

/// Import a DXF: one tagged group per CAD layer, one shared prototype per
/// used block. `scale` is metres per drawing unit; `name` names the
/// layer-0 group (default: the file's stem).
pub fn load_dxf(
    scene: &mut Scene,
    path: &Path,
    mut progress: Option<Progress<'_>>,
    scale: f64,
    doc: Option<&DxfDoc>,
    name: Option<&str>,
) -> Result<DxfStats, DxfError> {
    let mut tick = |f: f64, t: &str| {
        if let Some(p) = progress.as_mut() {
            p(f, t);
        }
    };
    tick(0.05, "Reading file…");
    let owned;
    let doc = match doc {
        Some(d) => d,
        None => {
            owned = open_document(path)?;
            &owned
        }
    };
    let sagitta = (SAGITTA_M / scale).max(1e-6);
    tick(0.2, "Collecting entities…");
    let mut ctx = Ctx {
        doc,
        sagitta,
        skipped: IndexMap::new(),
    };
    let mut per_layer: IndexMap<String, Bucket> = IndexMap::new();
    let mut placements: Vec<(String, DMat4, String)> = Vec::new();
    let mut counter = 0usize;
    for e in &doc.entities {
        let layer = e.layer();
        if e.kind == "INSERT" {
            let bname = decode_unicode_escapes(e.get(2).unwrap_or("").trim());
            let cells = insert_cells(e);
            if cells.len() == 1 {
                let base = doc.blocks.get(&bname).map_or(DVec3::ZERO, |b| b.base);
                placements.push((bname, insert_matrix(e, base, (0, 0)), layer));
                counter += 1;
                continue;
            }
            // a MINSERT array explodes in place
            counter += ctx.flat(
                std::slice::from_ref(e),
                None,
                None,
                0,
                &mut |c, ent, m, lay| {
                    let b = per_layer.entry(lay).or_default();
                    c.bucket_one(ent, m, b)
                },
            );
            continue;
        }
        let b = per_layer.entry(layer).or_default();
        if ctx.bucket_one(e, None, b) {
            counter += 1;
        }
    }
    per_layer.retain(|_, b| !b.runs.is_empty() || !b.faces.is_empty());

    let mut proto_buckets: IndexMap<String, Bucket> = IndexMap::new();
    for (bname, _, _) in &placements {
        if proto_buckets.contains_key(bname) {
            continue;
        }
        let Some(block) = doc.blocks.get(bname) else {
            continue;
        };
        let mut bucket = Bucket::default();
        ctx.flat(&block.entities, None, None, 0, &mut |c, ent, m, _lay| {
            c.bucket_one(ent, m, &mut bucket)
        });
        proto_buckets.insert(bname.clone(), bucket);
    }
    if per_layer.is_empty()
        && !proto_buckets
            .values()
            .any(|b| !b.runs.is_empty() || !b.faces.is_empty())
    {
        return Err(empty_error());
    }

    tick(0.45, "Scaling…");
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    let mut absorb = |p: DVec3| {
        lo = lo.min(p);
        hi = hi.max(p);
    };
    for b in per_layer.values() {
        for (run, _) in &b.runs {
            run.iter().for_each(|p| absorb(*p * scale));
        }
        for lp in &b.faces {
            lp.iter().for_each(|p| absorb(*p * scale));
        }
    }
    for (_, m, _) in &placements {
        absorb(m.w_axis.truncate() * scale);
    }
    let far = [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
        .iter()
        .map(|c| c.abs())
        .fold(0.0, f64::max);
    let offset = if lo.x < hi.x && far > RECENTER_BEYOND_M {
        lo.map(f64::trunc)
    } else {
        DVec3::ZERO
    };

    tick(0.6, "Building geometry…");
    let mut stats = DxfStats {
        entities: counter,
        ..Default::default()
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ensure_tag = |scene: &mut Scene, l: &str| {
        if l != "0" && scene.doc.layer(l).is_none() {
            scene.doc.layers.push(Layer::new(l));
        }
    };
    let mut layers: Vec<&String> = per_layer.keys().collect();
    layers.sort();
    for layer in layers {
        let mesh = build_mesh(&per_layer[layer], scale, offset);
        if mesh.edges.is_empty() && mesh.faces.is_empty() {
            continue;
        }
        stats.edges += mesh.edges.len();
        stats.faces += mesh.faces.len();
        let gname = if layer != "0" {
            layer.clone()
        } else {
            name.map(str::to_string).unwrap_or_else(|| stem.clone())
        };
        let id = scene.doc.add_mesh(mesh);
        let mut g = Group::new(id, Some(&gname));
        if layer != "0" {
            g.layer = Some(layer.clone());
            ensure_tag(scene, layer);
        }
        scene.doc.groups.push(g);
        stats.groups += 1;
    }

    tick(0.75, "Placing components…");
    let mut protos: IndexMap<String, (xtrude_model::ids::MeshId, usize)> = IndexMap::new();
    for (bname, bucket) in &proto_buckets {
        let mesh = build_mesh(bucket, scale, DVec3::ZERO);
        if !mesh.edges.is_empty() || !mesh.faces.is_empty() {
            let n = mesh.edges.len();
            protos.insert(bname.clone(), (scene.doc.add_mesh(mesh), n));
        }
    }
    for (bname, m, layer) in &placements {
        let Some(&(pid, n_edges)) = protos.get(bname) else {
            continue;
        };
        let mut inst = Group::new(pid, Some(bname));
        let mut x = *m;
        x.w_axis = (m.w_axis.truncate() * scale - offset).extend(1.0);
        inst.xform = Some(x);
        if layer != "0" {
            inst.layer = Some(layer.clone());
            ensure_tag(scene, layer);
        }
        scene.doc.groups.push(inst);
        stats.groups += 1;
        let c = stats.components.entry(bname.clone()).or_insert(0);
        *c += 1;
        if *c == 1 {
            stats.edges += n_edges;
        }
    }
    if stats.groups == 0 {
        return Err(empty_error());
    }
    stats.skipped = ctx.skipped;
    stats.offset = if offset != DVec3::ZERO {
        Some(offset)
    } else {
        None
    };
    scene.version += 1;
    tick(1.0, "Done");
    Ok(stats)
}
