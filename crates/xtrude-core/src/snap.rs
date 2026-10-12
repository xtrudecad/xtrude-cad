// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Snap engine: pick the best snap target near the cursor.
//!
//! Snap kinds (priority high → low):
//! 1. `axis` — explicit axis lock from arrow keys.
//! 2. `reference` — parallel/perpendicular lock to a reference edge.
//! 3. `axis` via Shift — Shift held while an axis inference is active.
//! 4. `close` — closing the current polygon chain.
//! 5. `endpoint` — vertex of an existing edge.
//! 6. `intersection` — where two edges / guide lines actually cross.
//! 7. `midpoint` — midpoint of an existing edge.
//! 8. `origin` — world origin.
//! 9. `on_edge` — arbitrary point along an edge.
//! 10. `aligned` — level with / in line with an encouraged point across the
//!     face under the cursor.
//! 11. `axis_inference` — soft auto-detected axis alignment (visual cue).
//! 12. `none` — no snap.
//!
//! Distance checks for point snaps are done in **screen-space pixels** so
//! the snap radius stays constant under zoom. The caller supplies a
//! `world_to_pixel` callback so this module does not depend on the
//! viewport.
//!
//! Design note: mesh edges and the viewport's pseudo-edges are both
//! [`SnapEdge`].

use glam::DVec3;

use crate::axes::{self, Axis};
use crate::math::VecExt;
use crate::topology::{segment_intersection, SPLIT_TOLERANCE};

pub type Rgb = [f64; 3];

pub const COLOR_ENDPOINT: Rgb = [0.16, 0.62, 0.36];
pub const COLOR_MIDPOINT: Rgb = [0.20, 0.66, 0.74];
pub const COLOR_ON_EDGE: Rgb = [0.86, 0.22, 0.27];
pub const COLOR_ON_FACE: Rgb = [0.42, 0.46, 0.92];
pub const COLOR_ORIGIN: Rgb = [0.95, 0.45, 0.16];
pub const COLOR_CLOSE: Rgb = [0.20, 0.40, 0.78];
pub const COLOR_AXIS_X: Rgb = [0.86, 0.22, 0.27];
pub const COLOR_AXIS_Y: Rgb = [0.16, 0.62, 0.36];
pub const COLOR_AXIS_Z: Rgb = [0.20, 0.40, 0.78];
pub const COLOR_REFERENCE: Rgb = [0.85, 0.30, 0.80];
pub const COLOR_EXTENSION: Rgb = [0.55, 0.55, 0.58];
pub const COLOR_TANGENT: Rgb = [0.20, 0.66, 0.74];
pub const COLOR_NONE: Rgb = [0.0, 0.0, 0.0];

fn axis_vectors() -> [(Axis, DVec3); 3] {
    Axis::ALL.map(|a| (a, a.vector()))
}

/// What a snap landed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Axis,
    Reference,
    Close,
    Endpoint,
    Intersection,
    Midpoint,
    Origin,
    OnEdge,
    OnLine,
    OnFace,
    OnAxis,
    Aligned,
    AxisInference,
    FromPoint,
    Extension,
    ThroughPoint,
    PerpFace,
    Center,
    ComponentOrigin,
    ArcMidpoint,
    Tangent,
    None,
    /// A tool's own inference, named by the tool.
    Other(String),
}

impl SnapKind {
    /// The kind string (tool code and ScreenTips key on it).
    pub fn as_str(&self) -> &str {
        match self {
            SnapKind::Axis => "axis",
            SnapKind::Reference => "reference",
            SnapKind::Close => "close",
            SnapKind::Endpoint => "endpoint",
            SnapKind::Intersection => "intersection",
            SnapKind::Midpoint => "midpoint",
            SnapKind::Origin => "origin",
            SnapKind::OnEdge => "on_edge",
            SnapKind::OnLine => "on_line",
            SnapKind::OnFace => "on_face",
            SnapKind::OnAxis => "on_axis",
            SnapKind::Aligned => "aligned",
            SnapKind::AxisInference => "axis_inference",
            SnapKind::FromPoint => "from_point",
            SnapKind::Extension => "extension",
            SnapKind::ThroughPoint => "through_point",
            SnapKind::PerpFace => "perp_face",
            SnapKind::Center => "center",
            SnapKind::ComponentOrigin => "component_origin",
            SnapKind::ArcMidpoint => "arc_midpoint",
            SnapKind::Tangent => "tangent",
            SnapKind::None => "none",
            SnapKind::Other(s) => s,
        }
    }
}

/// Whether a snapped point belongs to a group or a component (the
/// ScreenTip adds "in group" / "in component").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapContext {
    Group,
    Component,
}

/// A dashed guide: two world points and a colour.
pub type Guide = (DVec3, DVec3, Rgb);

#[derive(Clone, Debug, PartialEq)]
pub struct SnapResult {
    pub point: DVec3,
    pub kind: SnapKind,
    pub color: Rgb,
    /// The axis when kind is `axis` or `axis_inference`.
    pub axis: Option<Axis>,
    /// Two world points of a dashed guide line to draw.
    pub guide: Option<(DVec3, DVec3)>,
    /// The guide's colour when it differs from the marker's.
    pub guide_color: Option<Rgb>,
    /// Extra dashed guides.
    pub guides: Option<Vec<Guide>>,
    pub context: Option<SnapContext>,
    /// The ScreenTip's own words, already translated.
    pub label: Option<String>,
}

impl SnapResult {
    pub fn new(point: DVec3, kind: SnapKind, color: Rgb) -> Self {
        SnapResult {
            point,
            kind,
            color,
            axis: None,
            guide: None,
            guide_color: None,
            guides: None,
            context: None,
            label: None,
        }
    }

    fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = Some(axis);
        self
    }

    fn with_guide(mut self, a: DVec3, b: DVec3) -> Self {
        self.guide = Some((a, b));
        self
    }

    fn with_guide_color(mut self, c: Rgb) -> Self {
        self.guide_color = Some(c);
        self
    }

    fn with_context(mut self, c: Option<SnapContext>) -> Self {
        self.context = c;
        self
    }
}

/// An edge as the snap engine sees it: a mesh edge in world coordinates, or
/// one of the viewport's pseudo-edges (a circle centre, a group origin, an
/// arc midpoint, a guide line).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapEdge {
    pub a: DVec3,
    pub b: DVec3,
    pub soft: bool,
    pub hidden: bool,
    pub curve: Option<i64>,
    /// The degenerate pseudo-edge that carries a circle centre.
    pub center: bool,
    /// A group's insertion point.
    pub component_origin: bool,
    /// The middle of an arc's sweep.
    pub arc_midpoint: bool,
    /// A construction guide: 'on line', no endpoints.
    pub guide: bool,
    pub in_group: bool,
    pub context: Option<SnapContext>,
    /// The top-level owner group (raw id), when known.
    pub group: Option<u64>,
}

impl SnapEdge {
    pub fn segment(a: DVec3, b: DVec3) -> Self {
        SnapEdge {
            a,
            b,
            ..Default::default()
        }
    }
}

// ---- Helpers ---------------------------------------------------------------

fn dominant_local(f: DVec3) -> (f64, Axis) {
    let lf = axes::to_local(f);
    let comps = [
        (lf.x.abs(), Axis::X),
        (lf.y.abs(), Axis::Y),
        (lf.z.abs(), Axis::Z),
    ];
    // The max over (value, name) tuples: ties go to the larger name
    comps
        .into_iter()
        .fold((f64::NEG_INFINITY, Axis::X), |best, c| {
            if c.0 > best.0 || (c.0 == best.0 && c.1 > best.1) {
                c
            } else {
                best
            }
        })
}

/// Project `point` onto the plane through `reference` facing the camera,
/// when the view is axis-aligned (a standard front/plan/side view):
/// a dimension then reads the FRONTAL span. Oblique views return the point
/// untouched.
pub fn project_to_view_plane(
    point: DVec3,
    reference: DVec3,
    forward: DVec3,
    threshold: f64,
) -> DVec3 {
    if forward.length() < 1e-9 {
        return point;
    }
    let f = forward.unit_or_zero();
    if !axes::is_world() {
        let (best, name) = dominant_local(f);
        if best < threshold {
            return point;
        }
        let a = axes::axis(name);
        return point + a * (reference - point).dot(a);
    }
    let (ax, ay, az) = (f.x.abs(), f.y.abs(), f.z.abs());
    if ax.max(ay).max(az) < threshold {
        return point;
    }
    if ax >= ay && ax >= az {
        return DVec3::new(reference.x, point.y, point.z);
    }
    if ay >= az {
        return DVec3::new(point.x, reference.y, point.z);
    }
    DVec3::new(point.x, point.y, reference.z)
}

/// Fallback work plane for a tool's FIRST point clicked in empty space:
/// in an axis-aligned view, the plane facing the camera through the
/// model's centre, as `(point, normal)`; `None` for oblique views.
pub fn first_point_work_plane(
    forward: DVec3,
    scene_center: DVec3,
    threshold: f64,
) -> Option<(DVec3, DVec3)> {
    if forward.length() < 1e-9 {
        return None;
    }
    let f = forward.unit_or_zero();
    if !axes::is_world() {
        let (best, name) = dominant_local(f);
        if best < threshold {
            return None;
        }
        let a = axes::axis(name);
        let sign = if f.dot(a) > 0.0 { 1.0 } else { -1.0 };
        return Some((scene_center, a * sign));
    }
    if f.x.abs().max(f.y.abs()).max(f.z.abs()) < threshold {
        return None;
    }
    let sgn = |v: f64| if v > 0.0 { 1.0 } else { -1.0 };
    let normal = if f.x.abs() >= f.y.abs() && f.x.abs() >= f.z.abs() {
        DVec3::new(sgn(f.x), 0.0, 0.0)
    } else if f.y.abs() >= f.z.abs() {
        DVec3::new(0.0, sgn(f.y), 0.0)
    } else {
        DVec3::new(0.0, 0.0, sgn(f.z))
    };
    Some((scene_center, normal))
}

/// If the start→candidate direction is within `angle_deg` of an axis, that
/// axis.
pub fn detect_axis_alignment(start: DVec3, candidate: DVec3, angle_deg: f64) -> Option<Axis> {
    let mut delta = candidate - start;
    let length = delta.length();
    if length < 1e-6 {
        return None;
    }
    let cos_thresh = angle_deg.to_radians().cos();
    if !axes::is_world() {
        delta = axes::to_local(delta);
    }
    let mut c = [
        (delta.x.abs() / length, Axis::X),
        (delta.y.abs() / length, Axis::Y),
        (delta.z.abs() / length, Axis::Z),
    ];
    // sort(reverse=True) on (value, name) tuples
    c.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    if c[0].0 >= cos_thresh {
        Some(c[0].1)
    } else {
        None
    }
}

/// An axis whose screen projection is shorter than this points at the
/// camera and is skipped.
const MIN_AXIS_SCREEN_PX: f64 = 12.0;

/// How far along an axis the resolved point may land, as a multiple of how
/// far the cursor itself is from the start.
pub const MAX_AXIS_REACH: f64 = 5.0;

/// Which axis the cursor is sitting on, measured in PIXELS.
pub fn detect_axis_on_screen(
    start: DVec3,
    candidate_world: DVec3,
    candidate_pixel: (f64, f64),
    world_to_pixel: &dyn Fn(DVec3) -> Option<(f64, f64)>,
    threshold_px: f64,
) -> Option<Axis> {
    let reach = (candidate_world - start).length();
    if reach < 1e-6 {
        return None;
    }
    let (sx, sy) = world_to_pixel(start)?;
    let (cx, cy) = candidate_pixel;
    let mut best: Option<(f64, Axis)> = None;
    for (name, a) in axis_vectors() {
        let Some((ax, ay)) = world_to_pixel(start + a * reach) else {
            continue;
        };
        let (dx, dy) = (ax - sx, ay - sy);
        let span = dx.hypot(dy);
        if span < MIN_AXIS_SCREEN_PX {
            continue;
        }
        let dist = ((cx - sx) * dy - (cy - sy) * dx).abs() / span;
        if dist <= threshold_px && best.is_none_or(|b| dist < b.0) {
            best = Some((dist, name));
        }
    }
    best.map(|b| b.1)
}

/// Parallel/perpendicular direction of a reference edge, IN THE PLANE
/// BEING DRAWN ON for the perpendicular.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefMode {
    Parallel,
    Perpendicular,
}

pub fn direction_from_edge(
    a: DVec3,
    b: DVec3,
    mode: RefMode,
    plane_normal: Option<DVec3>,
) -> Option<DVec3> {
    let direction = b - a;
    if direction.length() < 1e-6 {
        return None;
    }
    if mode == RefMode::Perpendicular {
        let normal = plane_normal.unwrap_or_else(|| axes::axis(Axis::Z));
        let perp = normal.cross(direction);
        if perp.length() < 1e-9 {
            return None;
        }
        return Some(perp / perp.length());
    }
    Some(direction.unit_or_zero())
}

/// Closest point on 2D segment `ab` to `p`: `(distance, t)`.
pub fn closest_on_segment_2d(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let denom = dx * dx + dy * dy;
    if denom == 0.0 {
        return ((p.0 - a.0).hypot(p.1 - a.1), 0.0);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / denom).clamp(0.0, 1.0);
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - qx).hypot(p.1 - qy), t)
}

/// The closest point on the infinite line (`start`, `dir`) to the camera
/// ray through the cursor.
pub type ProjectOntoLine<'a> = &'a dyn Fn(DVec3, DVec3) -> DVec3;
pub type WorldToPixel<'a> = &'a dyn Fn(DVec3) -> Option<(f64, f64)>;
pub type IsOccluded<'a> = &'a dyn Fn(DVec3) -> bool;

/// The cursor on screen and what a snap measures it with: the helpers'
/// shared arguments.
#[derive(Clone, Copy)]
struct Cursor<'a> {
    cx: f64,
    cy: f64,
    world_to_pixel: WorldToPixel<'a>,
    /// How near (pixels) a target must be.
    threshold_px: f64,
    is_occluded: Option<IsOccluded<'a>>,
}

fn on_edge_point(edge: &SnapEdge, t: f64, project: Option<ProjectOntoLine<'_>>) -> DVec3 {
    let ab = edge.b - edge.a;
    if let Some(project) = project {
        if ab.length() > 1e-9 {
            let proj = project(edge.a, ab);
            let tt = (proj - edge.a).dot(ab) / ab.dot(ab);
            return edge.a + ab * tt.clamp(0.0, 1.0);
        }
    }
    edge.a + ab * t
}

/// Where the infinite line through `p` (unit `u`) crosses segment `a`–`b`.
pub fn line_segment_intersection(
    p: DVec3,
    u: DVec3,
    a: DVec3,
    b: DVec3,
    tol: f64,
) -> Option<DVec3> {
    let v = b - a;
    if v.length() < 1e-9 {
        return None;
    }
    let w0 = p - a;
    let bb = u.dot(v);
    let cc = v.dot(v);
    let dd = u.dot(w0);
    let ee = v.dot(w0);
    let denom = cc - bb * bb;
    if denom.abs() < 1e-12 {
        return None;
    }
    let sc = (bb * ee - cc * dd) / denom;
    let tc = (ee - bb * dd) / denom;
    if !(-1e-6..=1.0 + 1e-6).contains(&tc) {
        return None;
    }
    let on_line = p + u * sc;
    let on_seg = a + v * tc;
    if (on_line - on_seg).length() > tol {
        return None;
    }
    Some(on_seg)
}

fn point_on_segment_world(p: DVec3, a: DVec3, b: DVec3, tol: f64) -> bool {
    let ab = b - a;
    let len_sq = ab.dot(ab);
    if len_sq < 1e-12 {
        return (p - a).length() < tol;
    }
    let t = (p - a).dot(ab) / len_sq;
    if !(-1e-6..=1.0 + 1e-6).contains(&t) {
        return false;
    }
    (p - (a + ab * t)).length() < tol
}

/// The circle through `points` (world, coplanar): `(centre, radius)` or
/// `None` when they do not sit on one (every point within `tol_rel` of the
/// radius, or 2 mm).
pub fn fit_circle(points: &[DVec3], tol_rel: f64) -> Option<(DVec3, f64)> {
    if points.len() < 3 {
        return None;
    }
    let c0 = points.iter().copied().sum::<DVec3>() / points.len() as f64;
    let q: Vec<DVec3> = points.iter().map(|p| *p - c0).collect();
    let mut m = [[0.0; 3]; 3];
    for p in &q {
        let a = p.to_array();
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += a[i] * a[j];
            }
        }
    }
    // the two dominant directions of the point cloud
    let mut vs: Vec<(f64, DVec3)> = crate::math::symmetric_eigenvectors(m)
        .into_iter()
        .map(|v| {
            let mv = DVec3::new(
                m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
                m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
                m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
            );
            (v.dot(mv), v)
        })
        .collect();
    vs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let (u, w) = (vs[0].1, vs[1].1);
    let xs: Vec<f64> = q.iter().map(|p| p.dot(u)).collect();
    let ys: Vec<f64> = q.iter().map(|p| p.dot(w)).collect();
    // least squares of [2x 2y 1]·[cx cy k] = x²+y²
    let mut ata = [[0.0; 3]; 3];
    let mut atb = [0.0; 3];
    for (&x, &y) in xs.iter().zip(&ys) {
        let row = [2.0 * x, 2.0 * y, 1.0];
        let rhs = x * x + y * y;
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * rhs;
        }
    }
    let sol = solve3(ata, atb)?;
    let (cx, cy) = (sol[0], sol[1]);
    let r2 = sol[2] + cx * cx + cy * cy;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    let dev = xs
        .iter()
        .zip(&ys)
        .map(|(&x, &y)| ((x - cx).hypot(y - cy) - r).abs())
        .fold(0.0, f64::max);
    if dev > 0.002f64.max(tol_rel * r) {
        return None;
    }
    Some((c0 + u * cx + w * cy, r))
}

// Gaussian elimination indexes rows and columns together.
#[allow(clippy::needless_range_loop)]
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let piv = (col..3).max_by(|&i, &j| {
            a[i][col]
                .abs()
                .partial_cmp(&a[j][col].abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        if a[piv][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for r in 0..3 {
            if r != col {
                let f = a[r][col] / a[col][col];
                for c in col..3 {
                    a[r][c] -= f * a[col][c];
                }
                b[r] -= f * b[col];
            }
        }
    }
    let x = [b[0] / a[0][0], b[1] / a[1][1], b[2] / a[2][2]];
    if x.iter().all(|v| v.is_finite()) {
        Some(x)
    } else {
        None
    }
}

/// One segment of a boundary loop for [`boundary_runs`]: its curve id and
/// soft flag (from the edge joining the two vertices).
#[derive(Clone, Copy, Debug, Default)]
pub struct LoopSeg {
    pub curve: Option<i64>,
    pub soft: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum RunKey {
    Id(i64),
    Soft,
    Geo,
}

/// Split a boundary loop into runs of consecutive segments that belong to
/// one circle or arc, as `(vertex indices in order, drawn)`. A drawn curve
/// says so with its `curve` id; an imported one usually only with `soft`
/// segments; a bare polyline gives itself away by equal segments turning by
/// a constant angle.
pub fn boundary_runs(positions: &[DVec3], segs: &[LoopSeg]) -> Vec<(Vec<usize>, bool)> {
    let n = positions.len();
    if n < 3 {
        return Vec::new();
    }
    let turn = |i: usize| -> (Option<f64>, f64, f64) {
        let a = positions[(i + n - 1) % n];
        let b = positions[i];
        let c = positions[(i + 1) % n];
        let (u, w) = (b - a, c - b);
        let (lu, lw) = (u.length(), w.length());
        if lu < 1e-9 || lw < 1e-9 {
            return (None, lu, lw);
        }
        let cosang = (u.dot(w) / (lu * lw)).clamp(-1.0, 1.0);
        (Some(cosang.acos().to_degrees()), lu, lw)
    };
    let keys: Vec<Option<RunKey>> = (0..n)
        .map(|i| {
            let s = segs.get(i).copied().unwrap_or_default();
            if let Some(cid) = s.curve {
                return Some(RunKey::Id(cid));
            }
            if s.soft {
                return Some(RunKey::Soft);
            }
            let (t, _lu, lw) = turn(i);
            let (tn, lu2, _lw2) = turn((i + 1) % n);
            let same = match (t, tn) {
                (Some(t), Some(tn)) => {
                    2.0 < t
                        && t < 60.0
                        && (t - tn).abs() < 1.5
                        && (lu2 - lw).abs() < 0.05 * lw.max(1e-9)
                }
                _ => false,
            };
            if same {
                Some(RunKey::Geo)
            } else {
                None
            }
        })
        .collect();
    if keys.iter().all(|k| k.is_some() && *k == keys[0]) {
        let drawn = matches!(keys[0], Some(RunKey::Id(_)));
        return vec![((0..n).collect(), drawn)];
    }
    let mut start = 0;
    for j in 0..n {
        if keys[j] != keys[(j + n - 1) % n] {
            start = j;
            break;
        }
    }
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_key: Option<RunKey> = None;
    for jj in 0..n {
        let j = (start + jj) % n;
        let k = &keys[j];
        if k.is_some() && *k == cur_key {
            cur.push(j);
        } else {
            if cur_key.is_some() && cur.len() >= 2 {
                runs.push(std::mem::take(&mut cur));
            }
            cur = if k.is_some() { vec![j] } else { Vec::new() };
            cur_key = k.clone();
        }
    }
    if cur_key.is_some() && cur.len() >= 2 {
        runs.push(cur);
    }
    runs.into_iter()
        .map(|r| {
            let drawn = matches!(keys[r[0]], Some(RunKey::Id(_)));
            let last = (r[r.len() - 1] + 1) % n;
            let mut verts = r;
            verts.push(last);
            (verts, drawn)
        })
        .collect()
}

/// The *Center* inference: the centre of every circle or arc on a face's
/// boundary loops (outer first, then holes), `(centre, radius, (loop, run))`
/// in the coordinates of `loops`.
pub fn curve_centers(loops: &[(Vec<DVec3>, Vec<LoopSeg>)]) -> Vec<(DVec3, f64, (usize, usize))> {
    let mut out = Vec::new();
    for (li, (positions, segs)) in loops.iter().enumerate() {
        for (ri, (verts, drawn)) in boundary_runs(positions, segs).into_iter().enumerate() {
            // three points always sit on SOME circle and four often do: a
            // run read from its shape or soft flags needs five; a drawn
            // curve says what it is
            if verts.len() < if drawn { 3 } else { 5 } {
                continue;
            }
            let pts: Vec<DVec3> = verts.iter().map(|&i| positions[i]).collect();
            if let Some((c, r)) = fit_circle(&pts, 0.01) {
                out.push((c, r, (li, ri)));
            }
        }
    }
    out
}

/// The centres of the curves on `face` of `mesh`, mapped by `xform`.
pub fn curve_centers_of_face(
    mesh: &crate::Mesh,
    face: crate::FaceId,
    xform: Option<&glam::DMat4>,
) -> Vec<(DVec3, f64, (usize, usize))> {
    use crate::math::MatExt;
    let f = mesh.face(face);
    let loops: Vec<(Vec<DVec3>, Vec<LoopSeg>)> = f
        .loops()
        .map(|ring| {
            let n = ring.len();
            let pos: Vec<DVec3> = ring
                .iter()
                .map(|&v| {
                    let p = mesh.pos(v);
                    xform.map_or(p, |m| m.project_point(p))
                })
                .collect();
            let segs: Vec<LoopSeg> = (0..n)
                .map(|i| match mesh.find_edge(ring[i], ring[(i + 1) % n]) {
                    Some(e) => LoopSeg {
                        curve: mesh.edge(e).curve,
                        soft: mesh.edge(e).soft,
                    },
                    None => LoopSeg::default(),
                })
                .collect();
            (pos, segs)
        })
        .collect();
    curve_centers(&loops)
}

fn vertex_on_line(vertex: DVec3, line_start: DVec3, line_dir: DVec3, tol: f64) -> bool {
    let rel = vertex - line_start;
    let proj = line_dir * rel.dot(line_dir);
    (rel - proj).length() < tol
}

fn hyp(p: (f64, f64), cx: f64, cy: f64) -> f64 {
    (p.0 - cx).hypot(p.1 - cy)
}

fn occluded(is_occluded: Option<IsOccluded<'_>>, p: DVec3) -> bool {
    is_occluded.is_some_and(|f| f(p))
}

/// Where the axis lines through two encouraged points cross.
fn two_point_snap(refs: &[DVec3], cursor: &Cursor<'_>) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px,
        is_occluded,
    } = *cursor;
    if refs.len() < 2 {
        return None;
    }
    let axes_v = axis_vectors();
    let mut best: Option<(f64, DVec3, DVec3, Axis, DVec3, Axis)> = None;
    for i in 0..refs.len() {
        for j in i + 1..refs.len() {
            let (p, q) = (refs[i], refs[j]);
            for &(ka, a) in &axes_v {
                for &(kb, b) in &axes_v {
                    if ka == kb {
                        continue;
                    }
                    let w = p - q;
                    let ab = a.dot(b);
                    let denom = 1.0 - ab * ab;
                    if denom < 1e-9 {
                        continue;
                    }
                    let (wa, wb) = (w.dot(a), w.dot(b));
                    let s_ = (ab * wb - wa) / denom;
                    let t_ = (wb - ab * wa) / denom;
                    let pa = p + a * s_;
                    let pb = q + b * t_;
                    if (pa - pb).length() > 1e-3 {
                        continue;
                    }
                    if s_.abs() < 1e-3 || t_.abs() < 1e-3 {
                        continue;
                    }
                    let Some(px) = world_to_pixel(pa) else {
                        continue;
                    };
                    let d = hyp(px, cx, cy);
                    if d > threshold_px || occluded(is_occluded, pa) {
                        continue;
                    }
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, pa, p, ka, q, kb));
                    }
                }
            }
        }
    }
    let (_, cross, p, ka, q, kb) = best?;
    let mut r = SnapResult::new(cross, SnapKind::FromPoint, COLOR_ENDPOINT)
        .with_guide(p, cross)
        .with_guide_color(ka.color());
    r.guides = Some(vec![(q, cross, kb.color())]);
    Some(r)
}

/// The foot of the cursor on an axis line through `reference` (the last
/// hovered corner); on a face (`plane_normal`) the answer stays on it.
fn first_point_from_point(
    reference: DVec3,
    candidate: DVec3,
    cursor: &Cursor<'_>,
    plane_normal: Option<DVec3>,
) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px,
        is_occluded,
    } = *cursor;
    let n = plane_normal
        .filter(|n| n.length() > 1e-9)
        .map(|n| n.unit_or_zero());
    let mut best: Option<(f64, DVec3, Axis)> = None;
    for (axis, a) in axis_vectors() {
        let an = n.map_or(0.0, |n| a.dot(n));
        let foot = if an.abs() < 1e-6 {
            let s = (candidate - reference).dot(a);
            if s.abs() < 1e-3 {
                continue;
            }
            if let Some(n) = n {
                if (reference - candidate).dot(n).abs() > 1e-4 {
                    continue;
                }
            }
            reference + a * s
        } else {
            let t = (candidate - reference).dot(n.unwrap_or(DVec3::ZERO)) / an;
            if t.abs() < 1e-3 {
                continue;
            }
            reference + a * t
        };
        let Some(fp) = world_to_pixel(foot) else {
            continue;
        };
        let d = hyp(fp, cx, cy);
        if d > threshold_px || occluded(is_occluded, foot) {
            continue;
        }
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, foot, axis));
        }
    }
    let (_, foot, axis) = best?;
    Some(
        SnapResult::new(foot, SnapKind::FromPoint, COLOR_ENDPOINT)
            .with_guide(reference, foot)
            .with_guide_color(axis.color()),
    )
}

/// Where an axis line through `reference` crosses `edge` near the cursor.
fn edge_from_point_crossing(
    edge: &SnapEdge,
    reference: DVec3,
    cursor: &Cursor<'_>,
) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px,
        is_occluded,
    } = *cursor;
    let mut best: Option<(f64, DVec3, Axis)> = None;
    for (axis, a) in axis_vectors() {
        let Some(hit) = line_segment_intersection(reference, a, edge.a, edge.b, 1e-3) else {
            continue;
        };
        if (hit - reference).length() < 1e-4 {
            continue;
        }
        let Some(hp) = world_to_pixel(hit) else {
            continue;
        };
        let d = hyp(hp, cx, cy);
        if d > threshold_px || occluded(is_occluded, hit) {
            continue;
        }
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, hit, axis));
        }
    }
    let (_, hit, axis) = best?;
    Some(
        SnapResult::new(hit, SnapKind::FromPoint, COLOR_ENDPOINT)
            .with_guide(reference, hit)
            .with_guide_color(axis.color())
            .with_context(edge.context),
    )
}

fn colour_along(v: DVec3) -> Rgb {
    if v.length() < 1e-6 {
        return COLOR_EXTENSION;
    }
    let u = v.unit_or_zero();
    // max() keeps the first of equal keys
    let mut best = (Axis::X, f64::NEG_INFINITY);
    for (k, a) in axis_vectors() {
        let d = u.dot(a).abs();
        if d > best.1 {
            best = (k, d);
        }
    }
    best.0.color()
}

/// Level with — or in line with — an encouraged point ACROSS the face under
/// the cursor.
fn in_plane_with_point_snap(
    reference: DVec3,
    candidate: DVec3,
    plane_normal: Option<DVec3>,
    cursor: &Cursor<'_>,
) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px,
        is_occluded,
    } = *cursor;
    let n = plane_normal.filter(|n| n.length() >= 1e-9)?.unit_or_zero();
    let mut best: Option<(f64, DVec3, Axis, DVec3)> = None;
    for (axis, a) in axis_vectors() {
        let d = a.cross(n);
        if d.length() < 1e-6 {
            continue;
        }
        let d = d.unit_or_zero();
        let m = n.cross(d);
        let ma = m.dot(a);
        let s = (candidate - reference).dot(a);
        let foot = candidate - m * (s / ma);
        let Some(fp) = world_to_pixel(foot) else {
            continue;
        };
        let dist = hyp(fp, cx, cy);
        if dist > threshold_px || occluded(is_occluded, foot) {
            continue;
        }
        if best.is_none_or(|b| dist < b.0) {
            best = Some((dist, foot, axis, d));
        }
    }
    let (_, foot, axis, d) = best?;
    let elbow = foot + d * (reference - foot).dot(d);
    let mut guides = Vec::new();
    if (elbow - reference).length() > 1e-4 {
        guides.push((reference, elbow, colour_along(elbow - reference)));
    }
    let mut r = SnapResult::new(foot, SnapKind::Aligned, COLOR_ENDPOINT)
        .with_axis(axis)
        .with_guide(elbow, foot)
        .with_guide_color(colour_along(foot - elbow));
    if !guides.is_empty() {
        r.guides = Some(guides);
    }
    Some(r)
}

/// Extension / intersection inference along an edge's continuation.
fn extension_snap(
    candidate_world: DVec3,
    cursor: &Cursor<'_>,
    edges: &[SnapEdge],
    start_point: Option<DVec3>,
    project: Option<ProjectOntoLine<'_>>,
) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px: et,
        is_occluded,
    } = *cursor;
    let start_point = start_point?;
    let draw = candidate_world - start_point;
    if draw.length() < 1e-6 {
        return None;
    }
    let draw = draw.unit_or_zero();
    let sp = if project.is_some() {
        world_to_pixel(start_point)
    } else {
        None
    };
    let (sdx, sdy) = sp.map_or((0.0, 0.0), |sp| (cx - sp.0, cy - sp.1));
    let sdl = sdx.hypot(sdy);
    let mut best_ext: Option<(f64, DVec3, DVec3, usize, DVec3)> = None;
    for (idx, edge) in edges.iter().enumerate() {
        if edge.soft || edge.hidden || edge.curve.is_some() {
            continue;
        }
        let ab = edge.b - edge.a;
        if ab.length() < 1e-9 {
            continue;
        }
        let u = ab.unit_or_zero();
        let mut at = candidate_world;
        if draw.dot(u).abs() < 0.966 {
            if sdl < 4.0 {
                continue;
            }
            let off = start_point - edge.a;
            if (off - u * off.dot(u)).length() > 1e-3 {
                continue;
            }
            let (Some(pa), Some(pb)) = (world_to_pixel(edge.a), world_to_pixel(edge.b)) else {
                continue;
            };
            let (edx, edy) = (pb.0 - pa.0, pb.1 - pa.1);
            let edl = edx.hypot(edy);
            if edl < 20.0 || (sdx * edx + sdy * edy).abs() < 0.966 * sdl * edl {
                continue;
            }
            let Some(project) = project else { continue };
            at = project(edge.a, u);
        }
        let t = (at - edge.a).dot(u);
        if -1e-6 <= t && t <= ab.length() + 1e-6 {
            continue;
        }
        let proj = edge.a + u * t;
        let Some(pp) = world_to_pixel(proj) else {
            continue;
        };
        let d = hyp(pp, cx, cy);
        if d > et || occluded(is_occluded, proj) {
            continue;
        }
        if best_ext.is_none_or(|b| d < b.0) {
            let from_end = if t < 0.0 { edge.a } else { edge.b };
            best_ext = Some((d, proj, from_end, idx, u));
        }
    }
    let (_, proj, from_end, src, u) = best_ext?;
    let mut best_hit: Option<(f64, DVec3)> = None;
    for (idx, other) in edges.iter().enumerate() {
        if idx == src {
            continue;
        }
        let Some(hit) = line_segment_intersection(proj, u, other.a, other.b, 1e-3) else {
            continue;
        };
        let Some(hp) = world_to_pixel(hit) else {
            continue;
        };
        let dh = hyp(hp, cx, cy);
        if dh > et || occluded(is_occluded, hit) {
            continue;
        }
        if best_hit.is_none_or(|b| dh < b.0) {
            best_hit = Some((dh, hit));
        }
    }
    if let Some((_, hit)) = best_hit {
        return Some(
            SnapResult::new(hit, SnapKind::Intersection, COLOR_ENDPOINT).with_guide(from_end, hit),
        );
    }
    Some(SnapResult::new(proj, SnapKind::Extension, COLOR_EXTENSION).with_guide(from_end, proj))
}

/// The line a 'from point' foot lies on.
#[derive(Clone, Copy)]
enum FromLine {
    /// The axis within [`FROM_POINT_AXIS_DEG`] of this draw direction.
    NearAxis(DVec3),
    /// A locked line; the hovered references are targets too.
    Locked(DVec3),
}

/// How close to an axis a draw runs for 'from point' to follow that axis.
const FROM_POINT_AXIS_DEG: f64 = 10.0;

/// 'From point' inference: the fixed foot of a corner (or midpoint, or the
/// origin) on the axis-aligned draw line — or, under a lock, on the locked
/// line, with the hovered references themselves as targets.
fn from_point_snap(
    edges: &[SnapEdge],
    start_point: Option<DVec3>,
    line: FromLine,
    cursor: &Cursor<'_>,
    extra_point: Option<DVec3>,
    project: Option<ProjectOntoLine<'_>>,
) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px,
        is_occluded,
    } = *cursor;
    let (draw_dir, line_dir, hovered_refs) = match line {
        FromLine::NearAxis(d) => (d, None, false),
        FromLine::Locked(d) => (d, Some(d), true),
    };
    let axis_deg = FROM_POINT_AXIS_DEG;
    let start_point = start_point?;
    if draw_dir.length() < 1e-6 {
        return None;
    }
    let adir = if let Some(ld) = line_dir {
        if ld.length() < 1e-6 {
            return None;
        }
        ld.unit_or_zero()
    } else {
        let u = draw_dir.unit_or_zero();
        let cos_axis = axis_deg.to_radians().cos();
        let mut adir = None;
        for (_, a) in axis_vectors() {
            let dp = u.dot(a);
            if dp.abs() >= cos_axis {
                adir = Some(if dp > 0.0 { a } else { -a });
                break;
            }
        }
        adir?
    };
    // (point, kind, colour, is the hovered edge body)
    let mut refs: Vec<(DVec3, SnapKind, Rgb, bool)> = Vec::new();
    if let Some(p) = extra_point {
        refs.push((p, SnapKind::FromPoint, COLOR_ENDPOINT, false));
    }
    for e in edges {
        refs.push((e.a, SnapKind::FromPoint, COLOR_ENDPOINT, false));
        refs.push((e.b, SnapKind::FromPoint, COLOR_ENDPOINT, false));
        refs.push(((e.a + e.b) * 0.5, SnapKind::Midpoint, COLOR_MIDPOINT, false));
    }
    refs.push((DVec3::ZERO, SnapKind::Origin, COLOR_ORIGIN, false));
    if hovered_refs {
        let mut best_edge: Option<(f64, DVec3)> = None;
        for e in edges {
            let (Some(pa), Some(pb)) = (world_to_pixel(e.a), world_to_pixel(e.b)) else {
                continue;
            };
            let (d, t) = closest_on_segment_2d((cx, cy), pa, pb);
            if d <= threshold_px && best_edge.is_none_or(|b| d < b.0) {
                best_edge = Some((d, on_edge_point(e, t, project)));
            }
        }
        if let Some((_, p)) = best_edge {
            refs.push((p, SnapKind::FromPoint, COLOR_ENDPOINT, true));
        }
    }
    let mut best: Option<(f64, DVec3, DVec3, SnapKind, Rgb)> = None;
    for (r, kind, color, body) in refs {
        if body && best.is_some() {
            continue;
        }
        let s = (r - start_point).dot(adir);
        if !hovered_refs && s <= 1e-6 {
            continue;
        }
        let foot = start_point + adir * s;
        if (foot - r).length() < 1e-4 {
            continue;
        }
        let Some(qp) = world_to_pixel(foot) else {
            continue;
        };
        let mut d = hyp(qp, cx, cy);
        if hovered_refs {
            if let Some(rp) = world_to_pixel(r) {
                d = d.min(hyp(rp, cx, cy));
            }
        }
        if d > threshold_px || occluded(is_occluded, foot) {
            continue;
        }
        if best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, foot, r, kind, color));
        }
    }
    let (_, foot, r, kind, color) = best?;
    let mut res = SnapResult::new(foot, kind.clone(), color).with_guide(r, foot);
    if kind == SnapKind::FromPoint {
        res.guide_color = Some(colour_along_first_max(foot - r));
    }
    Some(res)
}

/// The guide colour of a 'from point': the axis the guide most aligns with
/// (`max` over x, y, z — the first wins a tie).
fn colour_along_first_max(v: DVec3) -> Rgb {
    let gdn = v.unit_or_zero();
    let mut best = (Axis::X, f64::NEG_INFINITY);
    for (k, a) in axis_vectors() {
        let d = gdn.dot(a).abs();
        if d > best.1 {
            best = (k, d);
        }
    }
    best.0.color()
}

fn through_point_snap(
    start_point: Option<DVec3>,
    through_point: Option<DVec3>,
    draw_dir: DVec3,
    project: Option<ProjectOntoLine<'_>>,
    inference_deg: f64,
) -> Option<SnapResult> {
    let (start_point, through_point, project) = (start_point?, through_point?, project?);
    if draw_dir.length() < 1e-6 {
        return None;
    }
    let dirv = through_point - start_point;
    if dirv.length() < 1e-6 {
        return None;
    }
    let dir_u = dirv.unit_or_zero();
    if draw_dir.unit_or_zero().dot(dir_u).abs() < inference_deg.to_radians().cos() {
        return None;
    }
    let locked = project(start_point, dir_u);
    Some(
        SnapResult::new(locked, SnapKind::ThroughPoint, COLOR_REFERENCE)
            .with_guide(through_point, locked),
    )
}

fn perpendicular_face_snap(
    start_point: Option<DVec3>,
    face_normal: Option<DVec3>,
    draw_dir: DVec3,
    project: Option<ProjectOntoLine<'_>>,
    inference_deg: f64,
) -> Option<SnapResult> {
    let (start_point, face_normal, project) = (start_point?, face_normal?, project?);
    if draw_dir.length() < 1e-6 || face_normal.length() < 1e-6 {
        return None;
    }
    let n_u = face_normal.unit_or_zero();
    if draw_dir.unit_or_zero().dot(n_u).abs() < inference_deg.to_radians().cos() {
        return None;
    }
    Some(SnapResult::new(
        project(start_point, n_u),
        SnapKind::PerpFace,
        COLOR_REFERENCE,
    ))
}

/// Green intersection point where two edges / guide lines actually cross.
fn intersection_snap(cursor: &Cursor<'_>, edges: &[SnapEdge]) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px: et,
        is_occluded,
    } = *cursor;
    let mut near: Vec<&SnapEdge> = Vec::new();
    for e in edges {
        if e.center {
            continue;
        }
        let ab = e.b - e.a;
        if ab.dot(ab) < 1e-12 {
            continue;
        }
        let (Some(pa), Some(pb)) = (world_to_pixel(e.a), world_to_pixel(e.b)) else {
            continue;
        };
        if closest_on_segment_2d((cx, cy), pa, pb).0 <= et {
            near.push(e);
        }
    }
    if near.len() < 2 {
        return None;
    }
    let mut best: Option<(f64, DVec3)> = None;
    for i in 0..near.len() {
        for j in i + 1..near.len() {
            let (e1, e2) = (near[i], near[j]);
            let Some(hit) = segment_intersection(e1.a, e1.b, e2.a, e2.b, SPLIT_TOLERANCE) else {
                continue;
            };
            let Some(hp) = world_to_pixel(hit) else {
                continue;
            };
            let dh = hyp(hp, cx, cy);
            if dh > et || occluded(is_occluded, hit) {
                continue;
            }
            if best.is_none_or(|b| dh < b.0) {
                best = Some((dh, hit));
            }
        }
    }
    best.map(|(_, hit)| SnapResult::new(hit, SnapKind::Intersection, COLOR_ENDPOINT))
}

/// What a directional lock still lets you fetch, in order: the chain's own
/// first point (closing), a vertex ON the lock line, the crossing of the
/// lock line with another edge, and a corner, midpoint or origin.
fn lock_line_snaps(
    edges: &[SnapEdge],
    start_point: DVec3,
    line_dir: DVec3,
    cursor: &Cursor<'_>,
    acquired_point: Option<DVec3>,
    chain_first_point: Option<DVec3>,
    project: Option<ProjectOntoLine<'_>>,
) -> Option<SnapResult> {
    let Cursor {
        cx,
        cy,
        world_to_pixel,
        threshold_px,
        is_occluded,
    } = *cursor;
    if let Some(fp) = chain_first_point {
        if vertex_on_line(fp, start_point, line_dir, 1e-4) {
            if let Some(px) = world_to_pixel(fp) {
                if hyp(px, cx, cy) <= threshold_px {
                    return Some(SnapResult::new(fp, SnapKind::Close, COLOR_CLOSE));
                }
            }
        }
    }
    for e in edges {
        for v in [e.a, e.b] {
            if !vertex_on_line(v, start_point, line_dir, 1e-4) {
                continue;
            }
            let Some(vp) = world_to_pixel(v) else {
                continue;
            };
            if hyp(vp, cx, cy) <= threshold_px {
                return Some(SnapResult::new(v, SnapKind::Endpoint, COLOR_ENDPOINT));
            }
        }
    }
    let mut best_hit: Option<(f64, DVec3)> = None;
    for e in edges {
        let Some(hit) = line_segment_intersection(start_point, line_dir, e.a, e.b, 1e-3) else {
            continue;
        };
        let Some(hp) = world_to_pixel(hit) else {
            continue;
        };
        let dh = hyp(hp, cx, cy);
        if dh > threshold_px || occluded(is_occluded, hit) {
            continue;
        }
        if best_hit.is_none_or(|b| dh < b.0) {
            best_hit = Some((dh, hit));
        }
    }
    if let Some((_, hit)) = best_hit {
        return Some(
            SnapResult::new(hit, SnapKind::Intersection, COLOR_ENDPOINT)
                .with_guide(start_point, hit),
        );
    }
    from_point_snap(
        edges,
        Some(start_point),
        FromLine::Locked(line_dir),
        cursor,
        acquired_point,
        project,
    )
}

/// Two point candidates closer than this on screen are a tie, and the tie
/// goes to the drawing context.
const TIE_PX: f64 = 0.5;

/// The linear-inference toggle (Alt).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LinearMode {
    /// Every inference.
    #[default]
    All,
    /// Point snaps only.
    Off,
    /// Only parallel/perpendicular.
    ParallelPerp,
}

/// Everything [`compute_snap`] is asked with.
pub struct SnapQuery<'a> {
    pub candidate_world: DVec3,
    pub candidate_pixel: (f64, f64),
    pub world_to_pixel: WorldToPixel<'a>,
    pub threshold_px: f64,
    pub project_onto_line: Option<ProjectOntoLine<'a>>,
    pub chain_first_point: Option<DVec3>,
    pub start_point: Option<DVec3>,
    pub axis_lock: Option<Axis>,
    pub shift_held: bool,
    /// The reference edge's ends (Down arrow + edge under cursor).
    pub reference_edge: Option<(DVec3, DVec3)>,
    pub reference_mode: Option<RefMode>,
    pub inference_angle_deg: f64,
    pub screen_axis_px: Option<f64>,
    pub is_occluded: Option<IsOccluded<'a>>,
    pub face_under_cursor: bool,
    pub edge_threshold_px: Option<f64>,
    pub magnetic_axis_deg: Option<f64>,
    pub acquired_edge: Option<(DVec3, DVec3)>,
    pub acquired_point: Option<DVec3>,
    pub acquired_face_normal: Option<DVec3>,
    pub acquired_points: Vec<DVec3>,
    pub shift_lock_dir: Option<DVec3>,
    pub shift_lock_color: Option<Rgb>,
    pub linear_mode: LinearMode,
    pub work_plane_normal: Option<DVec3>,
    pub radial_arm: bool,
}

impl<'a> SnapQuery<'a> {
    /// A query with the defaults for every optional argument.
    pub fn new(
        candidate_world: DVec3,
        candidate_pixel: (f64, f64),
        world_to_pixel: WorldToPixel<'a>,
        threshold_px: f64,
    ) -> Self {
        SnapQuery {
            candidate_world,
            candidate_pixel,
            world_to_pixel,
            threshold_px,
            project_onto_line: None,
            chain_first_point: None,
            start_point: None,
            axis_lock: None,
            shift_held: false,
            reference_edge: None,
            reference_mode: None,
            inference_angle_deg: 3.0,
            screen_axis_px: None,
            is_occluded: None,
            face_under_cursor: false,
            edge_threshold_px: None,
            magnetic_axis_deg: None,
            acquired_edge: None,
            acquired_point: None,
            acquired_face_normal: None,
            acquired_points: Vec::new(),
            shift_lock_dir: None,
            shift_lock_color: None,
            linear_mode: LinearMode::All,
            work_plane_normal: None,
            radial_arm: false,
        }
    }
}

struct Pending {
    d: f64,
    world: DVec3,
    kind: SnapKind,
    color: Rgb,
    context: Option<SnapContext>,
    occludable: bool,
}

/// The best snap for the cursor (see the module doc for the priority).
pub fn compute_snap(q: &SnapQuery<'_>, edges: &[SnapEdge]) -> SnapResult {
    compute_snap_with_reach(q, edges, MAX_AXIS_REACH)
}

/// [`compute_snap`] with another cap on the on-screen axis reach (the
/// tests lift it to show the runaway it prevents).
#[doc(hidden)]
pub fn compute_snap_with_reach(
    q: &SnapQuery<'_>,
    edges: &[SnapEdge],
    max_axis_reach: f64,
) -> SnapResult {
    let allow_axis = q.linear_mode == LinearMode::All;
    let allow_parperp = q.linear_mode != LinearMode::Off;
    // a protractor ARM is a direction from the centre, not a line drawn
    let line_inferences = allow_axis && !q.radial_arm;
    let w2p = q.world_to_pixel;
    let (cx, cy) = q.candidate_pixel;
    let thr = q.threshold_px;
    let occ = q.is_occluded;
    let project = q.project_onto_line;
    let cur = Cursor {
        cx,
        cy,
        world_to_pixel: w2p,
        threshold_px: thr,
        is_occluded: occ,
    };

    // 1. Explicit axis lock (arrow keys).
    if let (Some(lock), Some(start), Some(proj)) = (q.axis_lock, q.start_point, project) {
        let mut axis_dir = axes::axis(lock);
        let locked = proj(start, axis_dir);
        // a lock line runs BOTH ways: point it at the cursor
        if (locked - start).dot(axis_dir) < 0.0 {
            axis_dir = -axis_dir;
        }
        if let Some(hit) = lock_line_snaps(
            edges,
            start,
            axis_dir,
            &cur,
            q.acquired_point,
            q.chain_first_point,
            project,
        ) {
            return hit;
        }
        return SnapResult::new(locked, SnapKind::Axis, lock.color()).with_axis(lock);
    }

    // 1.5 Sticky inference lock (Shift captured an active inference).
    if let (Some(dir), Some(start), Some(proj)) = (q.shift_lock_dir, q.start_point, project) {
        let mut lock_dir = dir;
        let locked = proj(start, lock_dir);
        if (locked - start).dot(lock_dir) < 0.0 {
            lock_dir = -lock_dir;
        }
        if let Some(hit) = lock_line_snaps(
            edges,
            start,
            lock_dir,
            &cur,
            q.acquired_point,
            q.chain_first_point,
            project,
        ) {
            return hit;
        }
        return SnapResult::new(
            locked,
            SnapKind::Reference,
            q.shift_lock_color.unwrap_or(COLOR_REFERENCE),
        );
    }

    // 2. Reference edge lock (Down arrow + edge under cursor).
    if let (Some((ea, eb)), Some(mode), Some(start), Some(proj)) =
        (q.reference_edge, q.reference_mode, q.start_point, project)
    {
        if let Some(mut direction) = direction_from_edge(ea, eb, mode, q.work_plane_normal) {
            let locked = proj(start, direction);
            if (locked - start).dot(direction) < 0.0 {
                direction = -direction;
            }
            if let Some(hit) = lock_line_snaps(
                edges,
                start,
                direction,
                &cur,
                q.acquired_point,
                q.chain_first_point,
                project,
            ) {
                return hit;
            }
            return SnapResult::new(locked, SnapKind::Reference, COLOR_REFERENCE);
        }
    }

    // 3. Shift held + auto axis inference → lock to that axis.
    if allow_axis && q.shift_held {
        if let (Some(start), Some(proj)) = (q.start_point, project) {
            if let Some(inferred) =
                detect_axis_alignment(start, q.candidate_world, q.inference_angle_deg)
            {
                let mut axis_dir = axes::axis(inferred);
                let locked = proj(start, axis_dir);
                if (locked - start).dot(axis_dir) < 0.0 {
                    axis_dir = -axis_dir;
                }
                if let Some(hit) = lock_line_snaps(
                    edges,
                    start,
                    axis_dir,
                    &cur,
                    q.acquired_point,
                    q.chain_first_point,
                    project,
                ) {
                    return hit;
                }
                return SnapResult::new(locked, SnapKind::Axis, inferred.color())
                    .with_axis(inferred);
            }
        }
    }

    let et = q.edge_threshold_px.unwrap_or(thr);
    // Edge inferences measure against the edge threshold.
    let edge_cur = Cursor {
        threshold_px: et,
        ..cur
    };
    let mut pending: Vec<Pending> = Vec::new();
    let consider = |pending: &mut Vec<Pending>,
                    world: DVec3,
                    kind: SnapKind,
                    color: Rgb,
                    occludable: bool,
                    context: Option<SnapContext>| {
        let Some(px) = w2p(world) else { return };
        let d = hyp(px, cx, cy);
        if d > thr {
            return;
        }
        pending.push(Pending {
            d,
            world,
            kind,
            color,
            context,
            occludable,
        });
    };
    // the nearest visible candidate; near-ties go to the drawing context
    let resolve = |pending: &mut Vec<Pending>| -> Option<Pending> {
        pending.sort_by_key(|c| ((c.d / TIE_PX).floor() as i64, c.context.is_some() as u8));
        let mut chosen = None;
        for c in pending.drain(..) {
            if c.occludable && occluded(occ, c.world) {
                continue;
            }
            chosen = Some(c);
            break;
        }
        pending.clear();
        chosen
    };

    // 4. Vertex snaps (close, endpoint).
    let mut best: Option<Pending> = None;
    if let (Some(first), Some(start)) = (q.chain_first_point, q.start_point) {
        // A chain of one point passes the same point as both
        if first != start {
            consider(
                &mut pending,
                first,
                SnapKind::Close,
                COLOR_CLOSE,
                false,
                None,
            );
            best = resolve(&mut pending);
        }
    }
    if best.as_ref().is_none_or(|b| b.kind != SnapKind::Close) {
        let mut plain = Vec::new();
        for e in edges {
            if e.center {
                consider(
                    &mut pending,
                    e.a,
                    SnapKind::Center,
                    COLOR_ENDPOINT,
                    true,
                    None,
                );
            } else if e.component_origin {
                consider(
                    &mut pending,
                    e.a,
                    SnapKind::ComponentOrigin,
                    COLOR_ORIGIN,
                    true,
                    None,
                );
            } else if e.arc_midpoint {
                consider(
                    &mut pending,
                    e.a,
                    SnapKind::ArcMidpoint,
                    COLOR_MIDPOINT,
                    true,
                    None,
                );
            } else if e.guide {
                continue;
            } else {
                plain.push(e);
            }
        }
        consider(
            &mut pending,
            DVec3::ZERO,
            SnapKind::Origin,
            COLOR_ORIGIN,
            true,
            None,
        );
        for e in plain {
            consider(
                &mut pending,
                e.a,
                SnapKind::Endpoint,
                COLOR_ENDPOINT,
                true,
                e.context,
            );
            consider(
                &mut pending,
                e.b,
                SnapKind::Endpoint,
                COLOR_ENDPOINT,
                true,
                e.context,
            );
        }
        if best.is_none() {
            best = resolve(&mut pending);
        }
    }
    if let Some(b) = best {
        return SnapResult::new(b.world, b.kind, b.color).with_context(b.context);
    }

    // 4b. Perpendicular to a face you started on.
    if allow_parperp {
        if let (Some(start), Some(proj)) = (q.start_point, project) {
            let draw = q.candidate_world - start;
            if draw.length() > 1e-6 {
                let draw_u = draw.unit_or_zero();
                let cos_tol = 8f64.to_radians().cos();
                for e in edges {
                    if !point_on_segment_world(start, e.a, e.b, 1e-3) {
                        continue;
                    }
                    let Some(perp) =
                        direction_from_edge(e.a, e.b, RefMode::Perpendicular, q.work_plane_normal)
                    else {
                        continue;
                    };
                    if draw_u.dot(perp).abs() < cos_tol {
                        continue;
                    }
                    let mut best_hit: Option<(f64, DVec3)> = None;
                    for other in edges {
                        if point_on_segment_world(start, other.a, other.b, 1e-3) {
                            continue;
                        }
                        let Some(hit) =
                            line_segment_intersection(start, perp, other.a, other.b, 1e-3)
                        else {
                            continue;
                        };
                        if (hit - start).dot(draw_u) <= 0.0 {
                            continue;
                        }
                        let Some(hp) = w2p(hit) else { continue };
                        let dh = hyp(hp, cx, cy);
                        if dh > et || occluded(occ, hit) {
                            continue;
                        }
                        if best_hit.is_none_or(|b| dh < b.0) {
                            best_hit = Some((dh, hit));
                        }
                    }
                    if let Some((_, hit)) = best_hit {
                        return SnapResult::new(hit, SnapKind::Intersection, COLOR_ENDPOINT)
                            .with_guide(start, hit);
                    }
                    if line_inferences {
                        if let Some(fp) = from_point_snap(
                            edges,
                            Some(start),
                            FromLine::NearAxis(q.candidate_world - start),
                            &cur,
                            q.acquired_point,
                            None,
                        ) {
                            return fp;
                        }
                    }
                    return SnapResult::new(
                        proj(start, perp),
                        SnapKind::Reference,
                        COLOR_REFERENCE,
                    );
                }
            }
        }
    }

    // 4e. 'Through point' — but not when the cursor is ON that point.
    if line_inferences {
        if let Some(start) = q.start_point {
            let on_the_point = q
                .acquired_point
                .and_then(w2p)
                .is_some_and(|ap| hyp(ap, cx, cy) <= thr);
            if !on_the_point {
                if let Some(tp) = through_point_snap(
                    Some(start),
                    q.acquired_point,
                    q.candidate_world - start,
                    project,
                    q.inference_angle_deg,
                ) {
                    return tp;
                }
            }
        }
    }

    // 5. Extension along an edge's continuation.
    if line_inferences {
        if let Some(ext) =
            extension_snap(q.candidate_world, &edge_cur, edges, q.start_point, project)
        {
            return ext;
        }
    }

    // 5b. 'From point' along an axis-aligned draw.
    if line_inferences {
        if let Some(start) = q.start_point {
            if let Some(fp) = from_point_snap(
                edges,
                Some(start),
                FromLine::NearAxis(q.candidate_world - start),
                &cur,
                q.acquired_point,
                None,
            ) {
                return fp;
            }
        }
    }

    // 5c. Edge / guide intersection (the green X).
    if let Some(inter) = intersection_snap(&edge_cur, edges) {
        return inter;
    }

    // 6. Midpoint + origin.
    for e in edges {
        if e.guide || (e.a - e.b).length() < 1e-9 {
            continue;
        }
        consider(
            &mut pending,
            (e.a + e.b) * 0.5,
            SnapKind::Midpoint,
            COLOR_MIDPOINT,
            true,
            e.context,
        );
    }
    consider(
        &mut pending,
        DVec3::ZERO,
        SnapKind::Origin,
        COLOR_ORIGIN,
        true,
        None,
    );
    if let Some(b) = resolve(&mut pending) {
        return SnapResult::new(b.world, b.kind, b.color).with_context(b.context);
    }

    // 7. On-edge, with a more generous radius.
    let mut best_edge: Option<(f64, DVec3, &SnapEdge)> = None;
    for e in edges {
        let (Some(pa), Some(pb)) = (w2p(e.a), w2p(e.b)) else {
            continue;
        };
        let (d, t) = closest_on_segment_2d((cx, cy), pa, pb);
        if d > et {
            continue;
        }
        let ab = e.b - e.a;
        let on_pt = match project {
            Some(proj) if ab.length() > 1e-9 => {
                let p = proj(e.a, ab);
                let tt = (p - e.a).dot(ab) / ab.dot(ab);
                e.a + ab * tt.clamp(0.0, 1.0)
            }
            _ => e.a + ab * t,
        };
        if occluded(occ, on_pt) {
            continue;
        }
        if best_edge.is_none_or(|b| d < b.0) {
            best_edge = Some((d, on_pt, e));
        }
    }
    if let Some((_, on_pt, e)) = best_edge {
        if line_inferences {
            if let Some(ap) = q.acquired_point {
                if let Some(cross) = edge_from_point_crossing(e, ap, &cur) {
                    return cross;
                }
            }
        }
        if e.guide {
            return SnapResult::new(on_pt, SnapKind::OnLine, COLOR_ON_EDGE);
        }
        return SnapResult::new(on_pt, SnapKind::OnEdge, COLOR_ON_EDGE).with_context(e.context);
    }

    // 8b. Acquired-edge parallel inference (off-axis edges only).
    if allow_parperp {
        if let (Some((ea, eb)), Some(start), Some(proj)) = (q.acquired_edge, q.start_point, project)
        {
            let ev = eb - ea;
            let draw = q.candidate_world - start;
            if ev.length() > 1e-9
                && draw.length() > 1e-6
                && detect_axis_alignment(ea, eb, q.inference_angle_deg).is_none()
            {
                let ev_u = ev.unit_or_zero();
                if draw.unit_or_zero().dot(ev_u).abs() >= q.inference_angle_deg.to_radians().cos() {
                    return SnapResult::new(
                        proj(start, ev_u),
                        SnapKind::Reference,
                        COLOR_REFERENCE,
                    );
                }
            }
        }
    }

    // 8c. Perpendicular to an encouraged face.
    if allow_parperp {
        if let Some(start) = q.start_point {
            if let Some(pf) = perpendicular_face_snap(
                Some(start),
                q.acquired_face_normal,
                q.candidate_world - start,
                project,
                q.inference_angle_deg,
            ) {
                return pf;
            }
        }
    }

    // 8d. 'From point' from an encouraged point (and two-point crossings).
    if line_inferences {
        if !q.acquired_points.is_empty() {
            if let Some(tp) = two_point_snap(&q.acquired_points, &cur) {
                return tp;
            }
        }
        if let Some(ap) = q.acquired_point {
            let on_plane = if q.face_under_cursor {
                q.work_plane_normal
            } else {
                None
            };
            if let Some(fp) = first_point_from_point(ap, q.candidate_world, &cur, on_plane) {
                return fp;
            }
            // 8e. …and level with / in line with it ACROSS this face.
            if on_plane.is_some() {
                if let Some(ip) = in_plane_with_point_snap(ap, q.candidate_world, on_plane, &cur) {
                    return ip;
                }
            }
        }
    }

    // 9. Axis inference (magnetic for Move; on-screen axes when asked).
    if let Some(start) = q.start_point {
        if allow_axis {
            if let (Some(mag), Some(proj)) = (q.magnetic_axis_deg, project) {
                if let Some(inferred) = detect_axis_alignment(start, q.candidate_world, mag) {
                    let locked = proj(start, axes::axis(inferred));
                    return SnapResult::new(locked, SnapKind::Axis, inferred.color())
                        .with_axis(inferred);
                }
            }
            if let (Some(px), Some(proj)) = (q.screen_axis_px, project) {
                if let Some(inferred) =
                    detect_axis_on_screen(start, q.candidate_world, q.candidate_pixel, w2p, px)
                {
                    let locked = proj(start, axes::axis(inferred));
                    let reach = (q.candidate_world - start).length();
                    if (locked - start).length() <= max_axis_reach * reach {
                        return SnapResult::new(locked, SnapKind::Axis, inferred.color())
                            .with_axis(inferred);
                    }
                }
            }
            if let Some(inferred) =
                detect_axis_alignment(start, q.candidate_world, q.inference_angle_deg)
            {
                return SnapResult::new(
                    q.candidate_world,
                    SnapKind::AxisInference,
                    inferred.color(),
                )
                .with_axis(inferred);
            }
        }
    }

    // 10. On-face.
    if q.face_under_cursor {
        return SnapResult::new(q.candidate_world, SnapKind::OnFace, COLOR_ON_FACE);
    }
    SnapResult::new(q.candidate_world, SnapKind::None, COLOR_NONE)
}

/// A face's plane as `(point, normal)` in WORLD coordinates. A component
/// instance's face lives in its prototype's local frame, so the normal is
/// rebuilt from the mapped polygon (exact for any affine map).
pub fn face_plane_world(
    mesh: &crate::Mesh,
    face: crate::FaceId,
    xform: Option<&glam::DMat4>,
) -> (DVec3, DVec3) {
    use crate::math::MatExt;
    let Some(m) = xform else {
        return (mesh.face_centroid(face), mesh.face_normal(face));
    };
    let pts: Vec<DVec3> = mesh
        .face_positions(face)
        .into_iter()
        .map(|p| m.project_point(p))
        .collect();
    let n = pts.len();
    if n == 0 {
        return (
            DVec3::ZERO,
            m.transform_vector(mesh.face_normal(face)).unit_or_zero(),
        );
    }
    let c = pts.iter().fold(DVec3::ZERO, |s, p| s + *p) / n as f64;
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        nx += (a.y - b.y) * (a.z + b.z);
        ny += (a.z - b.z) * (a.x + b.x);
        nz += (a.x - b.x) * (a.y + b.y);
    }
    let normal = DVec3::new(nx, ny, nz);
    if normal.length() < 1e-12 {
        return (c, m.transform_vector(mesh.face_normal(face)).unit_or_zero());
    }
    (c, normal.unit_or_zero())
}
