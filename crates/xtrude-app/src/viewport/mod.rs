// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport controller.
//!
//! Everything the viewport decides — camera navigation,
//! picking, the work plane, snapping, the value box, the Esc
//! cascade, tool dispatch, group editing, copy / paste — lives here,
//! independent of any window toolkit or GPU, so it runs headless in tests.
//! The egui shell feeds it [`crate::input`] events and the renderer reads
//! its state.
//!
//! [`Viewport`] holds the camera, the scene and its history, and one
//! sub-state per concern, each a type of its own with its invariant:
//! [`Navigation`] (camera drags, the Zoom Window box, the frame's depth),
//! [`InputState`] (keys held, the pointer's last place), [`Inference`] (the
//! snapping memory), [`ViewPrefs`], [`LivePreview`] (what a tool shows
//! while it works), the tool slot (the active tool, lent to its
//! callbacks) and [`Outbox`] (the [`ViewportEvent`]s, the pointer and the
//! repaints for the window). The `impl Viewport` blocks are split by
//! subject across the sub-modules: input in `events`, picking in `pick`,
//! snapping in `snapping`, the frame in [`render`], the 2D paint in
//! [`overlay`].

mod annotations;
mod clipboard;
mod depth;
mod events;
mod inference;
mod input_state;
mod nav_style;
mod navigation;
mod note;
mod outbox;
pub mod overlay;
mod pick;
mod plane;
mod prefs;
mod preview;
pub mod probe;
pub mod render;
mod snapping;
mod tool_slot;

use std::collections::HashSet;
use std::rc::Rc;
use xtrude_model::settings::{self, keys};

use glam::DVec3;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::snap::{LinearMode, SnapContext};
use xtrude_core::{EdgeId, FaceId};
use xtrude_model::entity::EntityRef;
use xtrude_model::history::{Command, History};
use xtrude_model::i18n::tr;
use xtrude_model::ids::{GroupId, MeshId, ObjId};
use xtrude_model::scene::Scene;
use xtrude_model::style::Style;
use xtrude_model::units;

use crate::tools::Tool;
use crate::value_box::ValueBox;

pub use depth::{DepthSeen, DepthSnap};
pub use inference::Inference;
pub use input_state::InputState;
pub use nav_style::{Binding, NavAction, NavStyle, Trigger};
pub use navigation::Navigation;
use navigation::RubberBand;
pub use note::{CursorNote, Note};
pub use outbox::Outbox;
pub use pick::{FaceHit, PickIndex, Placement, Projection};
pub use prefs::ViewPrefs;
pub use preview::LivePreview;
pub use probe::{CameraProbe, StandInProbe, ViewProbe};
pub use snapping::{CenterKey, CenterRef, InMotion, ENCOURAGE_MS};
use tool_slot::{ToolSlot, ToolSwitch};

/// What the viewport tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewportEvent {
    ValueBufferChanged(String),
    SceneVersionChanged(u64),
    MeasurementChanged(String),
    /// A brief status-bar message.
    Flash(String, u32),
    /// Ask the window to activate a tool by key ("select").
    ActivateTool(String),
    /// Paint's eyedropper took a color: the tray shows it, the
    /// eyedropper button releases.
    PaintSampled,
    /// A section plane was just placed: the window asks for its name and
    /// symbol (`prompt_section_name`).
    PromptSectionName(ObjId),
    /// The Hole tool's Tab: the window asks for the whole hole
    /// (`prompt_hole_spec`).
    PromptHoleSpec,
    /// The model clipboard was refilled (the window publishes it on the
    /// system clipboard).
    ClipboardChanged,
    /// A right click: the window opens the context menu.
    ContextMenu,
}

/// A camera navigation mode for a left drag (trackpads).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Orbit,
    Pan,
    Zoom,
    ZoomWindow,
}

/// The pointer over the viewport; `None` in [`Outbox::cursor`] is the
/// arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cursor {
    Orbit,
    Pan,
    Zoom,
    ZoomWindow,
    /// The system's "not allowed" pointer: the tool refuses what it shows.
    NotAllowed,
    /// A tool's pointer, drawn from the icon `key`, with the "+" badge when
    /// `plus`.
    Tool {
        key: &'static str,
        plus: bool,
    },
}

impl Cursor {
    /// The icon the shell draws the pointer from.
    pub fn key(self) -> &'static str {
        match self {
            Cursor::Orbit => "orbit",
            Cursor::Pan => "pan",
            Cursor::Zoom => "zoom",
            Cursor::ZoomWindow => "zoom_window",
            Cursor::NotAllowed => "not_allowed",
            Cursor::Tool { key, .. } => key,
        }
    }

    /// Whether the "+" badge goes on the icon.
    pub fn plus(self) -> bool {
        matches!(self, Cursor::Tool { plus: true, .. })
    }
}

/// How the model outside the open group reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestMode {
    Normal,
    Fade,
    Hide,
}

impl RestMode {
    pub const ALL: [RestMode; 3] = [RestMode::Normal, RestMode::Fade, RestMode::Hide];

    /// The spelling in the settings (`display/edit_rest_mode`) and in the
    /// `rest:` action ids.
    pub fn key(self) -> &'static str {
        match self {
            RestMode::Normal => "normal",
            RestMode::Fade => "fade",
            RestMode::Hide => "hide",
        }
    }

    pub fn from_key(key: &str) -> Option<RestMode> {
        RestMode::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// An edge under the cursor: a loose edge of the active mesh, or a group's
/// hard edge as a world pseudo-edge.
#[derive(Clone, Debug, PartialEq)]
pub struct PickedEdge {
    pub a: DVec3,
    pub b: DVec3,
    /// The loose edge (active mesh), `None` for a group's edge.
    pub edge: Option<(MeshId, xtrude_core::EdgeId)>,
    pub in_group: bool,
    pub context: Option<SnapContext>,
    /// The top-level owner of a group's edge.
    pub group: Option<GroupId>,
    pub soft: bool,
    pub curve: Option<i64>,
}

/// A modal text prompt: `(title, label, current, multiline)` → the text,
/// `None` when cancelled.
pub type Prompt = Box<dyn FnMut(&str, &str, &str, bool) -> Option<String>>;

pub struct Viewport {
    pub camera: OrbitCamera,
    pub scene: Scene,
    pub history: History,
    width: f64,
    height: f64,
    /// The active tool.
    pub(crate) tool_slot: ToolSlot,
    /// The snapping memory between hovers.
    pub inference: Inference,
    pub clipboard: Option<xtrude_io::clip::ClipData>,
    /// The keys held and the pointer's last place.
    pub input: InputState,
    /// Thresholds, navigation directions, anti-aliasing.
    pub prefs: ViewPrefs,
    hover_entity: Option<EntityRef>,
    pub(crate) render_cache: render::RenderCache,
    /// Camera navigation and the frame on screen.
    pub nav: Navigation,
    /// The box select under way.
    select_box: Option<RubberBand>,
    pub value_box: ValueBox,
    /// Events, the pointer and repaints for the window.
    pub outbox: Outbox,
    /// The note beside the pointer.
    pub note: CursorNote,
    /// A window is listening (it handles `ActivateTool` requests).
    pub(crate) has_window: bool,
    /// What a tool shows live while it works.
    pub preview: LivePreview,
    pick: pick::PickCache,
    /// Modal text prompt: `(title, label, current, multiline)` → the text,
    /// `None` when cancelled. The window
    /// installs one; tests install their own answers.
    pub prompt: Option<Prompt>,
    /// How pixels become rays and what is under the cursor: the camera,
    /// or a stand-in's answers (see [`probe`]).
    probe: Rc<dyn ViewProbe>,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport::new()
    }
}

impl Viewport {
    pub fn new() -> Viewport {
        let scene = Scene::new();
        scene.bind_units();
        Viewport {
            camera: OrbitCamera::new(),
            scene,
            history: History::new(),
            width: 640.0,
            height: 480.0,
            tool_slot: ToolSlot::default(),
            inference: Inference::default(),
            clipboard: None,
            input: InputState::default(),
            prefs: ViewPrefs::load(),
            hover_entity: None,
            render_cache: Default::default(),
            nav: Navigation::default(),
            select_box: None,
            value_box: ValueBox::default(),
            outbox: Outbox::default(),
            note: CursorNote::default(),
            has_window: false,
            preview: LivePreview::default(),
            pick: pick::PickCache::default(),
            prompt: None,
            probe: Rc::new(CameraProbe),
        }
    }

    /// A stand-in without projection ([`StandInProbe`]): tools driven
    /// through it see world points only.
    pub fn stub() -> Viewport {
        let mut vp = Viewport::new();
        vp.set_probe(Rc::new(StandInProbe));
        vp
    }

    /// Replace how the viewport projects and picks (tests).
    pub fn set_probe(&mut self, probe: Rc<dyn ViewProbe>) {
        self.probe = probe;
    }

    /// The viewport's probe, to call with the viewport itself.
    pub fn probe(&self) -> Rc<dyn ViewProbe> {
        Rc::clone(&self.probe)
    }

    /// Ask the user for a text (a modal dialog). `None`: cancelled, or no
    /// window to ask in.
    pub fn prompt_text(
        &mut self,
        title: &str,
        label: &str,
        current: &str,
        multiline: bool,
    ) -> Option<String> {
        let f = self.prompt.as_mut()?;
        f(title, label, current, multiline)
    }

    // ---- size ------------------------------------------------------------------------
    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    /// The widget's size (logical pixels). This does not touch the
    /// camera's aspect — only [`Self::resize_gl`] does, once the viewport
    /// is on screen.
    pub fn resize(&mut self, w: f64, h: f64) {
        self.width = w.max(1.0);
        self.height = h.max(1.0);
    }

    /// The drawable surface changed size: the camera follows.
    pub fn resize_gl(&mut self, w: f64, h: f64) {
        self.resize(w, h);
        self.camera.set_aspect(w.round() as i32, h.round() as i32);
    }

    // ---- signals ----------------------------------------------------------------------
    /// Ask for a repaint.
    pub fn update(&mut self) {
        self.outbox.redraws += 1;
    }

    /// Briefly show `text` in the status bar.
    pub fn flash_status(&mut self, text: &str, msec: u32) {
        self.outbox.flash(text, msec);
    }

    /// Drain the queued signals.
    pub fn take_events(&mut self) -> Vec<ViewportEvent> {
        std::mem::take(&mut self.outbox.events)
    }

    /// The scene changed from outside (load, undo, redo, leaving a
    /// group): the drawing axes follow its context at once.
    pub fn notify_scene_changed(&mut self) {
        self.sync_axes();
        self.outbox
            .events
            .push(ViewportEvent::SceneVersionChanged(self.scene.version));
        self.update();
    }

    fn emit_measurement(&mut self) {
        let text = self.measurement_text();
        self.outbox
            .events
            .push(ViewportEvent::MeasurementChanged(text));
    }

    /// Make the drawing axes the current context's. Pointer events, the
    /// context changes and every frame call it, so the axes drawn and
    /// snapped to never wait for the mouse to move.
    pub fn sync_axes(&self) {
        xtrude_core::axes::sync(self.scene.drawing_frame().as_ref());
    }

    /// The display style this frame draws with.
    pub fn effective_style(&self) -> &Style {
        self.preview
            .style_override
            .as_ref()
            .unwrap_or(&self.scene.display_style)
    }

    // ---- commands ---------------------------------------------------------------------
    /// Run a command through the history (one undo step).
    pub fn execute(&mut self, cmd: &mut dyn Command) -> bool {
        self.history.execute(&mut self.scene, cmd)
    }

    /// Run a boxed command (the builders return them).
    pub fn execute_boxed(&mut self, mut cmd: Box<dyn Command>) -> bool {
        self.history.execute(&mut self.scene, cmd.as_mut())
    }

    /// Run a command by value.
    pub fn run<C: Command>(&mut self, cmd: C) -> bool {
        self.history.run(&mut self.scene, cmd)
    }

    pub fn undo(&mut self) -> bool {
        let ok = self.history.undo(&mut self.scene);
        self.notify_scene_changed();
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self.history.redo(&mut self.scene);
        self.notify_scene_changed();
        ok
    }

    // ---- the active tool ------------------------------------------------------------
    pub fn active_tool(&self) -> Option<&dyn Tool> {
        self.tool_slot.active.as_deref()
    }

    pub fn active_tool_mut(&mut self) -> Option<&mut (dyn Tool + 'static)> {
        self.tool_slot.active.as_deref_mut()
    }

    /// The active tool as a concrete type, for tests that inspect its
    /// state; the app goes through the [`Tool`] trait (`clippy.toml`).
    #[doc(hidden)]
    #[allow(clippy::disallowed_methods)]
    pub fn tool<T: Tool>(&self) -> Option<&T> {
        self.tool_slot.active.as_ref()?.as_any().downcast_ref::<T>()
    }

    /// [`Self::tool`], mutable; for tests.
    #[doc(hidden)]
    #[allow(clippy::disallowed_methods)]
    pub fn tool_mut<T: Tool>(&mut self) -> Option<&mut T> {
        self.tool_slot
            .active
            .as_mut()?
            .as_any_mut()
            .downcast_mut::<T>()
    }

    /// The active tool's registry key.
    pub fn active_tool_key(&self) -> Option<&'static str> {
        self.tool_slot.active.as_ref().map(|t| t.info().key)
    }

    /// Call `f` with the active tool taken out of its slot, so the tool can
    /// act on the viewport. A tool switch it asks for meanwhile happens
    /// after it returns.
    pub fn with_tool<R>(&mut self, f: impl FnOnce(&mut dyn Tool, &mut Viewport) -> R) -> Option<R> {
        let mut tool = self.tool_slot.active.take()?;
        let was_out = self.tool_slot.out;
        self.tool_slot.out = true;
        let r = f(tool.as_mut(), self);
        self.tool_slot.out = was_out;
        self.tool_slot.active = Some(tool);
        match std::mem::take(&mut self.tool_slot.pending) {
            ToolSwitch::NotRequested => {
                if std::mem::take(&mut self.tool_slot.cursor_dirty) {
                    self.apply_tool_cursor();
                }
            }
            // The tool switched itself: it is the one that leaves.
            ToolSwitch::ToNoTool => self.set_active_tool(None),
            ToolSwitch::To(next) => self.set_active_tool(Some(next)),
        }
        Some(r)
    }

    fn tool_attr<R>(&self, f: impl FnOnce(&dyn Tool) -> R) -> Option<R> {
        self.tool_slot.active.as_deref().map(f)
    }

    /// The active tool's start point.
    pub fn tool_start_point(&self) -> Option<DVec3> {
        self.tool_attr(|t| t.start_point()).flatten()
    }

    /// Activate `tool` (`None` = no tool). Picking the tool you already hold
    /// starts it over — see [`Self::restart_tool`].
    pub fn set_active_tool(&mut self, tool: Option<Box<dyn Tool>>) {
        if self.tool_slot.out {
            self.tool_slot.pending = ToolSwitch::to(tool);
            return;
        }
        self.nav.mode = None;
        self.nav.end_drag();
        self.outbox.cursor = None;
        if self.tool_slot.active.is_some() {
            self.with_tool(|t, vp| t.on_deactivate(vp));
        }
        self.tool_slot.active = tool;
        self.inference.tool_changed();
        self.hover_entity = None;
        if self.tool_slot.active.is_some() {
            self.with_tool(|t, vp| t.on_activate(vp));
        }
        self.apply_tool_cursor();
        self.emit_measurement();
        self.update();
    }

    /// Picking the tool already held: the first point is released and the
    /// locks let go.
    pub fn restart_tool(&mut self) {
        if self.nav.mode.is_some() {
            return;
        }
        self.with_tool(|t, vp| t.on_cancel(vp));
        self.inference.axis_lock = None;
        self.inference.shift_lock = None;
        self.inference.clear_reference();
        self.inference.linear_inference_mode = LinearMode::All;
        self.inference.last_snap = None;
        self.refresh_snap();
    }

    /// `(front_dot, back_dot)`: the selection dot colour for each side of a
    /// face, each picked against the paint that side shows — its own colour
    /// (or the open group's), the two-sided back paint, or the style's
    /// back tint.
    pub fn selection_dot_colors(&self, mesh: MeshId, face: FaceId) -> ([f64; 3], [f64; 3]) {
        use xtrude_core::attrs::Back;
        use xtrude_render::selection::selection_dot_color;
        let owner = self
            .scene
            .edit_group()
            .and_then(|g| self.scene.doc.find_group(g))
            .and_then(|g| g.material.as_ref());
        let attrs = xtrude_model::materials::effective_attrs(
            &self.scene.doc.mesh(mesh).face(face).attrs,
            xtrude_model::group::effective_material(owner, None),
        );
        let front = attrs
            .paint
            .color
            .unwrap_or(crate::tools::paint::DEFAULT_FACE_COLOR);
        let back = match &attrs.back {
            Some(Back::Mirror) => Some(front),
            Some(Back::Own(p)) => p.color,
            None => Some(xtrude_model::style::effective_back_color(
                &self.scene.display_style,
                self.scene.back_face_color,
            )),
        };
        (selection_dot_color(Some(front)), selection_dot_color(back))
    }

    /// Whether a camera drag (orbit / pan / zoom) is under way.
    pub fn camera_dragging(&self) -> bool {
        self.nav.camera_drag.is_some()
    }

    /// Enter a camera navigation mode for the left drag (`None` clears it).
    pub fn set_nav_mode(&mut self, mode: Option<NavMode>) {
        if self.tool_slot.active.is_some() {
            self.with_tool(|t, vp| t.on_deactivate(vp));
            self.tool_slot.active = None;
        }
        self.hover_entity = None;
        self.inference.last_snap = None;
        self.nav.mode = mode;
        self.nav.end_drag();
        self.outbox.cursor = mode.map(|m| match m {
            NavMode::Orbit => Cursor::Orbit,
            NavMode::Pan => Cursor::Pan,
            NavMode::Zoom => Cursor::Zoom,
            NavMode::ZoomWindow => Cursor::ZoomWindow,
        });
        self.update();
    }

    /// The tool's pointer changed (a mode toggled): re-read it.
    pub fn refresh_tool_cursor(&mut self) {
        if self.tool_slot.out {
            self.tool_slot.cursor_dirty = true;
        } else {
            self.apply_tool_cursor();
        }
    }

    pub(crate) fn apply_tool_cursor(&mut self) {
        let Some(tool) = self.tool_slot.active.as_deref() else {
            self.outbox.cursor = None;
            return;
        };
        self.outbox.cursor = tool.cursor(self);
    }

    // ---- hover / suppression ---------------------------------------------------------
    pub fn hover_entity(&self) -> Option<EntityRef> {
        self.hover_entity
    }

    /// The entity highlighted under the cursor (`None` clears it).
    pub fn set_hover(&mut self, entity: Option<EntityRef>) {
        if entity == self.hover_entity {
            return;
        }
        self.hover_entity = entity;
        self.update();
    }

    pub fn suppressed_faces(&self) -> &HashSet<(MeshId, FaceId)> {
        &self.preview.suppressed_faces
    }

    pub fn suppressed_edges(&self) -> &HashSet<(MeshId, EdgeId)> {
        &self.preview.suppressed_edges
    }

    /// Hide faces and edges from the normal pass while a tool previews;
    /// empty sets draw everything again.
    pub fn set_suppressed(
        &mut self,
        faces: impl IntoIterator<Item = (MeshId, FaceId)>,
        edges: impl IntoIterator<Item = (MeshId, EdgeId)>,
    ) {
        let faces: HashSet<_> = faces.into_iter().collect();
        let edges: HashSet<_> = edges.into_iter().collect();
        let p = &mut self.preview;
        if faces == p.suppressed_faces && edges == p.suppressed_edges {
            return;
        }
        p.suppressed_faces = faces;
        p.suppressed_edges = edges;
        p.suppressed_serial += 1;
        self.update();
    }

    // ---- groups preview (Move / Rotate / Paste drags) ------------------------------------
    pub fn begin_groups_preview(&mut self, groups: &[GroupId]) {
        self.preview.begin(groups, false);
        self.update();
    }

    pub fn set_groups_preview_offset(&mut self, delta: DVec3) {
        self.preview.matrix = Some(glam::DMat4::from_translation(delta));
        self.update();
    }

    pub fn set_groups_preview_matrix(&mut self, m: glam::DMat4) {
        self.preview.matrix = Some(m);
        self.update();
    }

    /// Preview the clipboard's group templates (not in the scene).
    pub fn begin_external_groups_preview(&mut self, groups: &[GroupId]) {
        self.preview.begin(groups, true);
        self.update();
    }

    pub fn end_groups_preview(&mut self) {
        self.preview.end();
        self.update();
    }

    // ---- the value box ------------------------------------------------------------------

    pub fn set_value_buffer(&mut self, text: &str) {
        if text == self.value_box.buffer {
            return;
        }
        self.value_box.buffer = text.to_string();
        self.outbox
            .events
            .push(ViewportEvent::ValueBufferChanged(text.to_string()));
        self.update();
    }

    /// The live measurement for the value box: the tool's `value_label`,
    /// or the single segment a line is drawing.
    pub fn measurement_text(&self) -> String {
        let Some(tool) = self.tool_slot.active.as_deref() else {
            return String::new();
        };
        if let Some((s, _)) = tool.value_label(self) {
            return s;
        }
        let segs = tool.rubber_band_lines(self);
        if segs.len() == 1 {
            let (a, b) = segs[0];
            return units::fmt_len((b - a).length());
        }
        String::new()
    }

    // ---- group editing ---------------------------------------------------------------------
    /// Enter a group for editing (double-click into it).
    pub fn begin_group_edit(&mut self, group: GroupId) {
        let parent = self.scene.edit_group();
        let Some(g) = self.scene.doc.find_group(group) else {
            return;
        };
        let in_definition = parent
            .and_then(|p| self.scene.doc.find_group(p))
            .is_some_and(|p| p.children.iter().any(|c| c.id == group) && p.component);
        let (has_xform, component, has_children, name) = (
            g.xform.is_some(),
            g.component,
            !g.children.is_empty(),
            g.name.clone(),
        );
        if has_xform
            && !component
            && !has_children
            && !in_definition
            && self.mesh_shared_elsewhere(group)
        {
            // A copied GROUP shares its geometry only until it is edited:
            // opening it makes it its own, one undo step.
            self.run(xtrude_model::commands::MakeIndependentCommand { group });
        }
        let was_instance = has_xform && (!has_children || component);
        self.scene.begin_group_edit(group);
        self.sync_axes();
        let mark = self.history.undo_len();
        if let Some(level) = self.scene.edit_stack.last_mut() {
            level.undo_mark = mark;
        }
        self.hover_entity = None;
        let share = self
            .scene
            .edit_stack
            .last()
            .and_then(|l| l.share.as_ref())
            .map(|s| s.proto);
        match share {
            Some(proto) if was_instance => {
                // The open copy edits a mesh of its own until it leaves.
                let copies = self.copies_of(proto) + 1;
                let msg = tr(
                    "Editing component '{name}' — the change reaches its {n} copies when you leave (Esc); Make Independent first to edit one copy only",
                    &[("name", &name), ("n", &copies)],
                );
                self.flash_status(&msg, 6000);
            }
            _ => {
                let msg = tr(
                    "Editing {path} — Esc or click outside goes up one level",
                    &[("path", &self.edit_path_text())],
                );
                self.flash_status(&msg, 5000);
            }
        }
        self.update();
    }

    /// Whether another placement draws `group`'s mesh.
    fn mesh_shared_elsewhere(&self, group: GroupId) -> bool {
        let Some(mesh) = self.scene.doc.find_group(group).map(|g| g.mesh) else {
            return false;
        };
        self.scene.doc.groups.iter().any(|top| {
            xtrude_model::group::iter_placements(top, None)
                .iter()
                .any(|(g, _)| g.id != group && g.mesh == mesh)
        })
    }

    /// Where you are, as a path: `Assembly › Housing › Lid`.
    pub fn edit_path_text(&self) -> String {
        let names: Vec<String> = self
            .scene
            .edit_stack
            .iter()
            .filter_map(|l| self.scene.doc.find_group(l.group).map(|g| g.name.clone()))
            .collect();
        names.join(" › ")
    }

    /// Step out ONE level (Esc).
    pub fn end_one_group_edit(&mut self) {
        self.leave_group_edit(false);
    }

    /// Leave every open group (menus, save, export).
    pub fn end_group_edit(&mut self) {
        self.leave_group_edit(true);
    }

    fn leave_group_edit(&mut self, all: bool) {
        if self.scene.edit_group().is_none() {
            return;
        }
        let mut status = None;
        loop {
            if let Some(s) = self.leave_one_level() {
                status = Some(s);
            }
            if !all || self.scene.edit_group().is_none() {
                break;
            }
        }
        self.hover_entity = None;
        match status {
            Some((msg, ms)) => self.flash_status(&msg, ms),
            None if self.scene.edit_group().is_some() => {
                let msg = tr(
                    "Up one level — now in {path}",
                    &[("path", &self.edit_path_text())],
                );
                self.flash_status(&msg, 4000);
            }
            None => self.flash_status(&tr("Left the group", &[]), 2000),
        }
        self.notify_scene_changed();
    }

    /// Leave the innermost level, sharing an edited copy back to its
    /// definition as ONE undo step with the session's edits. Returns the
    /// status it has to show, if any.
    fn leave_one_level(&mut self) -> Option<(String, u32)> {
        let mark = self
            .scene
            .edit_stack
            .last()
            .map_or(0, |l| l.undo_mark)
            .min(self.history.undo_len());
        let share = self.scene.take_edit_share();
        self.scene.end_one_group_edit();
        let share = share?;
        let inner = self.history.undo_len() - mark;
        let edited = self.scene.doc.find_group(share.group).map(|g| g.mesh);
        let changed = edited.is_some() && self.scene.edit_changed(&share);
        let name = self
            .scene
            .doc
            .find_group(share.group)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        if let Some(contents) = &share.contents {
            if !changed && inner == 0 {
                self.scene.share_back_contents(&share);
                return None;
            }
            // Even unchanged, the session's steps saw the copy open: they
            // fold into the step that puts it back.
            self.run(xtrude_model::commands::ShareContentsCommand {
                share: share.clone(),
            });
            self.history
                .squash_last_with(inner + 1, "ShareContentsCommand", |before| {
                    share.restore_entry(before)
                });
            // A group inside a component reaches the copies through it.
            if !changed || !contents.component {
                return None;
            }
        } else {
            let Some(m) = edited.filter(|_| changed) else {
                self.scene
                    .restore_sharing(share.group, share.proto, share.xform);
                return Some((tr("Left the group", &[]), 2000));
            };
            self.run(xtrude_model::commands::ReshareInstanceCommand {
                group: share.group,
                proto: share.proto,
                xform: share.xform,
                edited: m,
            });
            // The session's edits folded into the share-back: ONE undo
            // step, whose undo lands back on the shared definition.
            self.history
                .squash_last_with(inner + 1, "ReshareInstanceCommand", |before| {
                    share.restore_entry(before)
                });
        }
        let msg = tr(
            "Component '{name}' updated on its {n} copies",
            &[("name", &name), ("n", &self.copies_of(share.proto))],
        );
        Some((msg, 4000))
    }

    /// How many placements, at every depth, draw `mesh`.
    fn copies_of(&self, mesh: MeshId) -> usize {
        self.scene
            .doc
            .groups
            .iter()
            .flat_map(|top| xtrude_model::group::iter_placements(top, None))
            .filter(|(g, _)| g.mesh == mesh)
            .count()
    }

    pub fn edit_rest_mode(&self) -> RestMode {
        self.prefs.edit_rest_mode
    }

    pub fn set_edit_rest_mode(&mut self, mode: RestMode) {
        if mode == self.prefs.edit_rest_mode {
            return;
        }
        self.prefs.edit_rest_mode = mode;
        settings::write(&keys::EDIT_REST_MODE, &mode.key().to_string());
        self.update();
    }

    /// Whether the model outside the edited group is out of this frame.
    pub fn rest_is_hidden(&self) -> bool {
        self.scene.edit_group().is_some() && self.prefs.edit_rest_mode == RestMode::Hide
    }

    /// A document boundary: the old drawing's caches go.
    pub fn reset_document_caches(&mut self) {
        self.pick = pick::PickCache::default();
        self.hover_entity = None;
        self.inference.hover_edge = None;
        self.inference.last_snap = None;
        self.update();
    }

    /// The eye the view is seen from; the orbit camera's when the probe
    /// has none.
    pub fn eye(&self) -> DVec3 {
        self.probe.eye(self).unwrap_or_else(|| self.camera.eye())
    }

    /// Alt is held down (Paint's eyedropper).
    pub fn alt_down(&self) -> bool {
        self.input.alt_down
    }

    /// Where the current orbit gesture turns (`None` between gestures).
    pub fn orbit_pivot(&self) -> Option<DVec3> {
        self.nav.orbit_pivot
    }

    pub fn toggle_projection(&mut self) {
        self.camera.toggle_projection();
        self.update();
    }

    /// An operation in progress Esc should cancel first.
    pub fn tool_busy(&self) -> bool {
        self.tool_attr(|t| t.busy()).unwrap_or(false)
    }
}

/// A length formatted per the dimension style.
pub fn format_dim_value(metres: f64, style: &xtrude_model::dimension::DimensionStyle) -> String {
    units::format_length(metres, &style.units, style.decimals)
}
