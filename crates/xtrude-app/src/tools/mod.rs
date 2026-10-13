// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Tools.
//!
//! A tool is anything the user activates from the toolbar to interact with
//! the viewport: draw, modify, select. The [`Tool`] trait has the events a
//! tool handles and what the viewport asks of it, each with its default; a
//! tool overrides what it has. Related answers come grouped: [`ToolInfo`]
//! (what the tool is), [`ValueSpec`] (what the value box hands it),
//! [`SnapPrefs`] and [`WorkPlane`].

use crate::input::ScreenPt;
use crate::value_box::{ArrayKind, PatternEntry, ValueSpec};
use std::any::Any;
use std::collections::HashSet;
use xtrude_core::Axis;

use glam::DVec3;
use xtrude_core::axes;
use xtrude_core::math::VecExt;
use xtrude_core::snap::{self, SnapResult};
use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;
use xtrude_model::scene::SelectMode;

use crate::input::{Key, Modifiers};
use crate::viewport::overlay::Paint;
use crate::viewport::{Cursor, Viewport};

pub mod arc;
pub mod chamfer;
pub mod change_axes;
pub mod circle;
pub mod dimension;
pub mod eraser;
pub mod fillet;
pub mod flip;
pub mod freehand;
pub mod hole;
pub mod line;
pub mod measure;
pub mod move_tool;
pub mod offset;
pub mod paint;
pub mod paste;
pub mod pattern;
pub mod place_group;
pub mod protractor;
pub mod pushpull;
pub mod rectangle;
pub mod rotate;
pub mod rotated_rectangle;
pub mod scale;
pub mod section;
pub mod select;
pub mod shell;
pub mod slot;
pub mod solids;
pub mod sweep;
pub mod text;
pub mod transform;

/// `(point, normal)` of a plane.
pub type Plane = (DVec3, DVec3);
/// RGBA colour.
pub type Rgba = [f64; 4];

/// What a tool receives for a pointer event: the (snapped) world point, the
/// raw screen position, the modifiers and the snap that produced the point.
#[derive(Clone, Debug)]
pub struct ToolContext {
    pub world: DVec3,
    pub screen: (f64, f64),
    pub modifiers: Modifiers,
    /// `None` from callers that drive a tool without the snap engine.
    pub snap: Option<SnapResult>,
}

impl ToolContext {
    /// A context at `world` with no snap (headless tests and scripts).
    pub fn at(world: DVec3) -> ToolContext {
        ToolContext {
            world,
            screen: (0.0, 0.0),
            modifiers: Modifiers::NONE,
            snap: None,
        }
    }

    pub fn with_modifiers(mut self, m: Modifiers) -> ToolContext {
        self.modifiers = m;
        self
    }

    /// The snap kind, `"none"` without a snap.
    pub fn snap_kind(&self) -> &str {
        self.snap.as_ref().map_or("none", |s| s.kind.as_str())
    }
}

/// A typed value the value box hands a tool (`on_value`).
#[derive(Clone, Debug, PartialEq)]
pub enum ToolValue {
    Num(f64),
    Pair(f64, f64),
    Triple(f64, f64, f64),
    /// `("abs_len", value)`: typed with a unit, for a tool that reads a bare
    /// number as a factor (Scale).
    AbsLen(Box<ToolValue>),
    /// `12s`: a segment / side count ([`ValueSpec::segments`]).
    Segments(u32),
    /// `3x`, `/3`, `5x10m`: an array after a copy ([`ValueSpec::array`]).
    Array {
        count: u32,
        kind: ArrayKind,
        step: Option<f64>,
    },
    /// `2r`: a radius ([`ValueSpec::radius`]).
    Radius(f64),
    /// `M3`, `M3c`: a screw size, the nominal diameter in mm
    /// ([`ValueSpec::screw_sizes`]).
    Screw {
        nominal: f64,
        close: bool,
    },
    /// `4;15`, `4/45`: a pattern's count and spacing ([`ValueSpec::pattern`]).
    Pattern(PatternEntry),
}

impl ToolValue {
    pub fn from_typed(t: &crate::value_box::Typed) -> Option<ToolValue> {
        use crate::value_box::Typed;
        Some(match *t {
            Typed::Value(v) => ToolValue::Num(v),
            Typed::Pair(a, b) => ToolValue::Pair(a, b),
            Typed::Triple(a, b, c) => ToolValue::Triple(a, b, c),
            _ => return None,
        })
    }

    pub fn as_num(&self) -> Option<f64> {
        match *self {
            ToolValue::Num(v) => Some(v),
            _ => None,
        }
    }
}

/// A shaded preview face: an outer loop, its holes and its look (world
/// coordinates).
pub type PreviewFace = xtrude_core::geometry::PolyFace;

/// What a transform tool is dragging: the loose edges (by their ends) and
/// the groups in motion, kept out of the snap scene.
#[derive(Clone, Debug, Default)]
pub struct SnapExcluded {
    pub edges: HashSet<xtrude_core::EdgeId>,
    pub groups: HashSet<GroupId>,
}

impl SnapExcluded {
    /// The loose edges at `verts` (active mesh) and `groups`; `None` when
    /// nothing is in motion.
    pub fn of(
        vp: &Viewport,
        verts: &[xtrude_core::VertexId],
        groups: &[GroupId],
    ) -> Option<SnapExcluded> {
        let mesh = vp.scene.mesh();
        let mut ex = SnapExcluded::default();
        for &v in verts {
            if mesh.has_vertex(v) {
                ex.edges.extend(mesh.verts[v].edges.iter().copied());
            }
        }
        ex.groups.extend(groups.iter().copied());
        if ex.edges.is_empty() && ex.groups.is_empty() {
            return None;
        }
        Some(ex)
    }
}

/// Where a tool's own overlay paint goes among the document annotations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayLayer {
    /// Beneath the section planes (Scale's box and grips).
    UnderSectionPlanes,
    /// Over the section planes, beneath the dimensions and text labels
    /// (Flip's planes, Move's rotation grips).
    OverSectionPlanes,
}

/// The Line tool's axis magnet for the other drawing tools: within 3° of
/// an axis from the start point the point lands ON the axis, and the axis
/// the work plane cannot hold is found in pixels.
pub const MAGNET_DEG: f64 = 3.0;
pub const MAGNET_PX: f64 = 9.0;

/// What a tool is, fixed for the tool (or for each of its variants).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolInfo {
    /// Registry key ("line", "pushpull", …).
    pub key: &'static str,
    /// Display name ("Line").
    pub name: &'static str,
    pub icon: Option<&'static str>,
    pub shortcut: Option<&'static str>,
    pub shortcut_alt: Option<&'static str>,
    pub description: Option<&'static str>,
    /// Click-drag rubber-band box (Select).
    pub box_select: bool,
    /// The tool's wireframe preview hides behind the model.
    pub wireframe_depth_tested: bool,
}

impl ToolInfo {
    /// A tool with only a key and a name; the other fields are filled in
    /// with struct update syntax.
    pub const fn new(key: &'static str, name: &'static str) -> ToolInfo {
        ToolInfo {
            key,
            name,
            icon: None,
            shortcut: None,
            shortcut_alt: None,
            description: None,
            box_select: false,
            wireframe_depth_tested: false,
        }
    }
}

/// How the snap engine treats a tool's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapPrefs {
    /// Drawing tools snap; picking tools (Select, Push/Pull) do not.
    pub uses_snap: bool,
    /// Within this many degrees of an axis from the start point, the point
    /// lands ON the axis.
    pub magnetic_axis_deg: Option<f64>,
    /// The axis the work plane cannot hold is found within this many
    /// pixels on screen.
    pub screen_axis_px: Option<f64>,
    /// The second point is an arm from the vertex, a direction rather than
    /// a line drawn: no line inferences (Protractor, Rotate).
    pub radial_arm: bool,
    /// Away from a face, the drag is read on an upright plane facing the
    /// camera, so it can go up and down (Move).
    pub prefers_vertical_drag: bool,
    /// Edges inside groups are hovered although the tool does not snap
    /// (Push/Pull).
    pub hover_group_edges: bool,
    /// The model axes are a source the tool can start from (Measure).
    pub axis_source: bool,
}

impl Default for SnapPrefs {
    fn default() -> SnapPrefs {
        SnapPrefs {
            uses_snap: true,
            magnetic_axis_deg: None,
            screen_axis_px: None,
            radial_arm: false,
            prefers_vertical_drag: false,
            hover_group_edges: false,
            axis_source: false,
        }
    }
}

impl SnapPrefs {
    /// A snapping tool with the Line tool's axis magnet when `on`.
    pub fn axis_magnet(on: bool) -> SnapPrefs {
        SnapPrefs {
            magnetic_axis_deg: on.then_some(MAGNET_DEG),
            screen_axis_px: on.then_some(MAGNET_PX),
            ..SnapPrefs::default()
        }
    }
}

/// Arrow keys → the axis a locked drawing plane is NORMAL to.
pub fn plane_lock_axis(key: Key) -> Option<Axis> {
    match key {
        Key::Right => Some(Axis::X),
        Key::Left => Some(Axis::Y),
        Key::Up => Some(Axis::Z),
        _ => None,
    }
}

/// The plane's everyday name.
pub fn plane_lock_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "YZ",
        Axis::Y => "XZ",
        _ => "XY",
    }
}

/// Which way a down-arrow reference lock reads the hovered entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    /// Perpendicular to the hovered edge.
    Edge,
    /// Parallel to the hovered face.
    Face,
}

/// The arrow-key drawing-plane lock of the planar tools (circle, polygon,
/// rectangle, the arcs): Right / Left / Up lock the plane normal to X / Y / Z
/// before the first click, Down locks it to the hovered edge or face.
#[derive(Clone, Debug, Default)]
pub struct PlaneLock {
    pub plane_lock: Option<Axis>,
    /// Down-arrow reference lock: `(normal, kind)`.
    pub plane_ref: Option<(DVec3, RefKind)>,
    /// The plane the viewport used for the last cursor hit.
    pub hover_plane: Option<Plane>,
}

/// A tool's drawing plane, as the viewport reads it.
#[derive(Clone, Copy, Debug)]
pub struct WorkPlane<'a> {
    /// The captured plane, `None` until the tool captures one.
    pub plane: Option<Plane>,
    /// The arrow-key plane lock, for a tool that has one.
    pub lock: Option<&'a PlaneLock>,
}

/// The protractor disc's radius on screen, px.
pub const DISC_PX: f64 = 60.0;
/// The protractor's tick pitch, degrees.
pub const TICK_DEG: f64 = 15.0;

/// How far off the drawing plane a point must be to count as elsewhere.
pub const OFF_PLANE_TOL: f64 = 1e-3;
/// Radius / half-side of the cursor preview, px.
pub const PREVIEW_PX: f64 = 22.0;

impl PlaneLock {
    /// Remember the plane the viewport just hit.
    pub fn note_plane(&mut self, vp: &Viewport) {
        if let Some(p) = vp.inference.last_work_plane {
            self.hover_plane = Some(p);
        }
    }

    fn own_plane(&self, work_plane: Option<Plane>) -> Plane {
        if let Some(p) = work_plane {
            return p;
        }
        if let Some(p) = self.hover_plane {
            return p;
        }
        (axes::origin(), axes::axis(Axis::Z))
    }

    /// `(point, normal)` the shape is laid out on: the captured or locked
    /// plane — unless a snapped point says otherwise — else the plane of the
    /// last hit, else the ground.
    pub fn drawing_plane(&self, work_plane: Option<Plane>, points: &[DVec3]) -> Plane {
        let plane = self.own_plane(work_plane);
        self.snapped_plane(plane, points).unwrap_or(plane)
    }

    /// The plane the shape's points ask for when a SNAPPED one lies off
    /// `plane`, or `None` to keep `plane`. An arrow-key lock never yields.
    pub fn snapped_plane(&self, plane: Plane, pts: &[DVec3]) -> Option<Plane> {
        if self.plane_lock.is_some() || self.plane_ref.is_some() {
            return None;
        }
        if pts.len() < 2 {
            return None;
        }
        let (origin, normal) = plane;
        let n = normal.unit_or_zero();
        let tol = OFF_PLANE_TOL;
        if pts.iter().all(|p| (*p - origin).dot(n).abs() < tol) {
            return None;
        }
        let a = pts[0];
        if pts.len() >= 3 {
            let through = (pts[1] - a).cross(pts[2] - a);
            if through.length() > tol * tol {
                return Some((a, through.unit_or_zero()));
            }
        }
        let span = pts[pts.len() - 1] - a;
        let flat: Vec<DVec3> = Axis::ALL
            .iter()
            .map(|&k| axes::axis(k))
            .filter(|ax| span.dot(*ax).abs() < tol)
            .collect();
        if flat.len() != 1 {
            return None;
        }
        Some((a, flat[0]))
    }

    /// Make the snapped plane the captured one (a point that is not the last
    /// was clicked).
    pub fn adopt_snapped_plane(&self, work_plane: &mut Option<Plane>, points: &[DVec3]) {
        if let Some(s) = self.snapped_plane(self.own_plane(*work_plane), points) {
            *work_plane = Some(s);
        }
    }

    /// The plane the shape WOULD take at `point` before the first click.
    pub fn preview_plane(&self, vp: &Viewport, point: DVec3) -> Plane {
        if let Some(locked) = self.locked_work_plane(point) {
            return locked;
        }
        if let Some((_, n)) = vp.inference.last_work_plane {
            if n.unit_or_zero().dot(axes::axis(Axis::Z)).abs() > 0.99 {
                if let Some(v) = vp.near_horizon_vertical(point) {
                    return v;
                }
            }
            return (point, n);
        }
        (point, axes::axis(Axis::Z))
    }

    /// RGBA of the axis the plane is normal to, or `None` off the axes.
    pub fn plane_color(normal: DVec3) -> Option<Rgba> {
        let mut n = normal.unit_or_zero();
        if !axes::is_world() {
            n = axes::to_local(n);
        }
        for (axis, comp) in [(Axis::X, n.x), (Axis::Y, n.y), (Axis::Z, n.z)] {
            if comp.abs() > 0.99 {
                let [r, g, b] = axis.color();
                return Some([r, g, b, 1.0]);
            }
        }
        None
    }

    /// The rubber band's colour while an arrow-key lock is on.
    pub fn lock_color(&self) -> Option<Rgba> {
        if self.plane_ref.is_some() {
            let [r, g, b] = snap::COLOR_REFERENCE;
            return Some([r, g, b, 1.0]);
        }
        let axis = self.plane_lock?;
        Self::plane_color(axes::axis(axis))
    }

    /// An arrow key before the first click: lock / free the drawing plane.
    pub fn plane_lock_key(&mut self, vp: &mut Viewport, key: Key, started: bool) -> bool {
        if started {
            return false;
        }
        if key == Key::Down {
            return self.reference_lock_key(vp);
        }
        let Some(axis) = plane_lock_axis(key) else {
            return false;
        };
        self.plane_ref = None;
        self.plane_lock = if self.plane_lock == Some(axis) {
            None
        } else {
            Some(axis)
        };
        let msg = match self.plane_lock {
            Some(a) => tr(
                "Drawing plane locked: {plane}",
                &[("plane", &plane_lock_name(a))],
            ),
            None => tr("Drawing plane free", &[]),
        };
        vp.flash_status(&msg, 2500);
        vp.update();
        true
    }

    fn reference_lock_key(&mut self, vp: &mut Viewport) -> bool {
        if self.plane_ref.is_some() {
            self.plane_ref = None;
            vp.flash_status(&tr("Drawing plane free", &[]), 2500);
        } else {
            let Some(r) = Self::hovered_reference(vp) else {
                return true;
            };
            self.plane_ref = Some(r);
            self.plane_lock = None;
            vp.flash_status(
                &tr(
                    match r.1 {
                        RefKind::Edge => "Drawing plane locked: perpendicular to the edge",
                        RefKind::Face => "Drawing plane locked: parallel to the face",
                    },
                    &[],
                ),
                2500,
            );
        }
        vp.update();
        true
    }

    /// `(normal, kind)` of the reference under the cursor: the edge beats
    /// the face; `None` over nothing.
    pub fn hovered_reference(vp: &mut Viewport) -> Option<(DVec3, RefKind)> {
        if let Some(e) = &vp.inference.hover_edge {
            let d = e.b - e.a;
            if d.length() > 1e-9 {
                return Some((d.unit_or_zero(), RefKind::Edge));
            }
        }
        let ScreenPt { x, y } = vp.input.last_mouse_pos?;
        let hit = vp.pick_face_placement(x, y)?;
        let (_p, n) = vp.face_plane_world(&hit);
        if n.length() > 1e-9 {
            return Some((n.unit_or_zero(), RefKind::Face));
        }
        None
    }

    /// Both locks off.
    pub fn clear_plane_lock(&mut self) {
        self.plane_lock = None;
        self.plane_ref = None;
    }

    /// `(point, normal)` of the locked plane through `point`, or `None`.
    pub fn locked_work_plane(&self, point: DVec3) -> Option<Plane> {
        if let Some(a) = self.plane_lock {
            return Some((point, axes::axis(a)));
        }
        self.plane_ref.map(|(n, _)| (point, n))
    }
}

/// Metres per screen pixel at `point` along `along`, or `None` when the
/// viewport cannot project.
pub fn world_per_pixel(vp: &Viewport, point: DVec3, along: DVec3) -> Option<f64> {
    let a = vp.world_to_pixel(point)?;
    let b = vp.world_to_pixel(point + along.unit_or_zero() * 0.25)?;
    let d = (b.x - a.x).hypot(b.y - a.y);
    (d > 1e-6).then(|| 0.25 / d)
}

/// Newell normal of a closed loop of points.
pub fn loop_normal(corners: &[DVec3]) -> DVec3 {
    let n = corners.len();
    let mut s = DVec3::ZERO;
    for i in 0..n {
        s += corners[i].cross(corners[(i + 1) % n]);
    }
    s
}

/// Wind `corners` so the face looks the way its plane does (a loop square
/// to the plane is left as it came).
pub fn face_the_plane(corners: &[DVec3], plane_normal: Option<DVec3>) -> Vec<DVec3> {
    let Some(n) = plane_normal else {
        return corners.to_vec();
    };
    if corners.len() < 3 {
        return corners.to_vec();
    }
    if loop_normal(corners).dot(n) < -1e-12 {
        let mut out = vec![corners[0]];
        out.extend(corners[1..].iter().rev());
        return out;
    }
    corners.to_vec()
}

thread_local! {
    static PAINT_SAMPLE_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Paint's eyedropper, armed by an Alt tap.
pub fn paint_sample_armed() -> bool {
    PAINT_SAMPLE_ARMED.with(|c| c.get())
}

pub fn set_paint_sample_armed(on: bool) {
    PAINT_SAMPLE_ARMED.with(|c| c.set(on));
}

/// The selection mode the keyboard modifiers ask for (Select tool): Shift
/// toggles, Ctrl adds, Shift+Ctrl removes.
pub fn selection_mode(m: Modifiers) -> SelectMode {
    match (m.shift, m.ctrl) {
        (true, true) => SelectMode::Remove,
        (true, false) => SelectMode::Toggle,
        (false, true) => SelectMode::Add,
        (false, false) => SelectMode::Replace,
    }
}

/// A tool. Every method but [`Self::info`] has a default; tools override
/// what they have. `vp` is the viewport the tool acts on.
pub trait Tool: Any {
    /// What the tool is: its key, name, icon, shortcuts.
    fn info(&self) -> &'static ToolInfo;
    fn wireframe_color(&self) -> Option<Rgba> {
        None
    }
    /// Caption of the value box, which may change with the tool's
    /// state (Circle: "Sides" before the centre, "Radius" after); `None`
    /// hides the box.
    fn value_box_caption(&self) -> Option<String> {
        None
    }

    /// The tool as `Any`, to downcast it in tests (`tool_any!`
    /// implements both).
    #[doc(hidden)]
    fn as_any(&self) -> &dyn Any;
    #[doc(hidden)]
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ---- lifecycle -----------------------------------------------------------
    fn on_activate(&mut self, _vp: &mut Viewport) {}
    fn on_deactivate(&mut self, _vp: &mut Viewport) {}

    // ---- spatial input -----------------------------------------------------------
    fn on_click(&mut self, _vp: &mut Viewport, _ctx: &ToolContext) {}
    fn on_hover(&mut self, _vp: &mut Viewport, _ctx: &ToolContext) {}
    fn on_double_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.on_click(vp, ctx);
    }
    fn on_triple_click(&mut self, vp: &mut Viewport, ctx: &ToolContext) {
        self.on_click(vp, ctx);
    }
    /// Rubber-band box released (`rect` normalised, pixels).
    fn on_box_select(
        &mut self,
        _vp: &mut Viewport,
        _rect: (f64, f64, f64, f64),
        _crossing: bool,
        _mode: SelectMode,
    ) {
    }
    /// The plane the cursor is read on mid-operation, or `None`.
    fn drag_plane(&self, _vp: &Viewport) -> Option<Plane> {
        None
    }
    fn on_cancel(&mut self, _vp: &mut Viewport) {}
    fn on_release(&mut self, _vp: &mut Viewport) {}

    // ---- keys -----------------------------------------------------------------------
    fn on_key(&mut self, _vp: &mut Viewport, _key: Key, _modifiers: Modifiers) -> bool {
        false
    }
    /// `None`: no release handler; `Some(true)`: consumed.
    fn on_key_release(&mut self, _vp: &mut Viewport, _key: Key) -> Option<bool> {
        None
    }

    // ---- typed values ----------------------------------------------------------------
    /// What the tool takes in the value box.
    fn value_spec(&self) -> ValueSpec {
        ValueSpec::default()
    }
    /// A value typed in the value box, of a kind `value_spec`
    /// accepts: whether the tool took it.
    fn on_value(&mut self, _vp: &mut Viewport, _value: &ToolValue) -> bool {
        false
    }
    /// The live measurement text and where to float it, `None` when the
    /// tool has nothing to say.
    fn value_label(&self, _vp: &Viewport) -> Option<(String, Option<DVec3>)> {
        None
    }
    fn status_clause(&self) -> String {
        String::new()
    }

    // ---- state the viewport reads ------------------------------------------------------
    fn start_point(&self) -> Option<DVec3> {
        None
    }
    fn chain_first_point(&self) -> Option<DVec3> {
        None
    }
    /// The tool's drawing plane, `None` for a tool without one.
    fn work_plane(&self) -> Option<WorkPlane<'_>> {
        None
    }
    /// Capture `plane` as the drawing plane, for a tool with a
    /// [`Self::work_plane`].
    fn set_work_plane(&mut self, _plane: Option<Plane>) {}
    /// The pointer over the viewport, `None` = the arrow; by default the
    /// tool's icon.
    fn cursor(&self, _vp: &Viewport) -> Option<Cursor> {
        self.info()
            .icon
            .map(|key| Cursor::Tool { key, plus: false })
    }
    /// The step the tool is in, for the status bar's hint: `idle`,
    /// `started`, or a step of the tool's own (the arc's `end` / `bulge`,
    /// the fillet's `sizing`).
    fn phase(&self) -> &'static str {
        if self.start_point().is_some() {
            "started"
        } else {
            "idle"
        }
    }
    /// How the snap engine treats the tool's points.
    fn snap_prefs(&self) -> SnapPrefs {
        SnapPrefs::default()
    }
    /// What the tool is dragging, kept out of the snap scene.
    fn snap_excluded(&self, _vp: &Viewport) -> Option<SnapExcluded> {
        None
    }
    /// An operation in progress that Esc cancels.
    fn busy(&self) -> bool {
        self.start_point().is_some()
    }
    /// `busy` plus state that holds geometry.
    fn busy_for_delete(&self) -> bool {
        self.busy()
    }
    /// The tool's own undo (a step inside its operation), `None` to let the
    /// history undo.
    fn on_undo(&mut self, _vp: &mut Viewport) -> Option<bool> {
        None
    }

    // ---- previews --------------------------------------------------------------------------
    fn rubber_band_lines(&self, _vp: &Viewport) -> Vec<(DVec3, DVec3)> {
        Vec::new()
    }
    fn guide_preview_lines(&self) -> Vec<(DVec3, DVec3)> {
        Vec::new()
    }
    fn preview_faces(&self) -> Vec<PreviewFace> {
        Vec::new()
    }
    /// A point the tool marks with the inference square, and what it is.
    fn inference_marker(&self) -> Option<(DVec3, pushpull::InferenceKind)> {
        None
    }
    /// The tool's own 2D paint at `layer` of the overlay (grips, planes).
    fn overlay(&self, _vp: &Viewport, _layer: OverlayLayer, _out: &mut Vec<Paint>) {}
}

/// Implements `as_any` / `as_any_mut` for a tool.
#[macro_export]
macro_rules! tool_any {
    () => {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    };
}
