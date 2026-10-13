// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The main window without its widgets: the tools and the actions that pick
//! them, the menus, the commands the menus run, the document's file lifecycle,
//! the status bar's text. The egui shell draws all of it and routes the user's
//! input here; tests drive it directly.

pub mod actions;
pub mod clipboard;
mod commands;
mod context;
pub mod dialogs;
pub mod extensions;
pub mod filedialogs;
pub mod files;
pub mod form;
mod gpu;
pub mod hints;
pub mod model_info;
pub mod preferences;
pub mod progress;
pub mod solid_inspector;
pub mod status_bar;
pub mod toolbar_layout;
pub mod toolbar_stacks;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use xtrude_model::settings::{self, keys};

use xtrude_model::i18n::tr;
use xtrude_model::intersect::IntersectMode;
use xtrude_model::style::{builtin_styles, Style};

use crate::input::{KeyEvent, MouseEvent};
use crate::tools::{self, Tool};
use crate::tray::{Tray, SECTIONS};
use crate::viewport::{NavMode, RestMode, Viewport, ViewportEvent};
pub use actions::{Action, ActionList, KeySeq, MenuItem};
pub use context::CtxItem;
pub use dialogs::{Asker, Dialogs, NoDialogs, SaveChoice, ScriptedDialogs};
pub use toolbar_layout::{ToolbarArea, ToolbarLayout};
pub use toolbar_stacks::ToolbarStacks;

/// A fresh tool for a toolbar key.
pub fn make_tool(key: &str) -> Option<Box<dyn Tool>> {
    use tools::*;
    use xtrude_model::solids::SolidOp;
    Some(match key {
        "select" => Box::new(select::SelectTool::new()),
        "line" => Box::new(line::LineTool::new()),
        "freehand" => Box::new(freehand::FreehandTool::new()),
        "rectangle" => Box::new(rectangle::RectangleTool::new()),
        "rotated_rect" => Box::new(rotated_rectangle::RotatedRectangleTool::new()),
        "circle" => Box::new(circle::RadialTool::circle()),
        "polygon" => Box::new(circle::RadialTool::polygon()),
        "arc" => Box::new(arc::ArcTool::new()),
        "arc3" => Box::new(arc::ThreePointArcTool::new()),
        "center_arc" => Box::new(arc::CenterArcTool::new()),
        "sector" => Box::new(arc::CenterArcTool::sector()),
        "pushpull" => Box::new(pushpull::PushPullTool::new()),
        "offset" => Box::new(offset::OffsetTool::new()),
        "move" => Box::new(move_tool::MoveTool::new()),
        "rotate" => Box::new(rotate::RotateTool::new()),
        "scale" => Box::new(scale::ScaleTool::new()),
        "flip" => Box::new(flip::FlipTool::new()),
        "sweep" => Box::new(sweep::SweepTool::new()),
        "fillet" => Box::new(fillet::FilletTool::new()),
        "chamfer" => Box::new(chamfer::ChamferTool::new()),
        "shell" => Box::new(shell::ShellTool::new()),
        "paint" => Box::new(paint::PaintTool::new()),
        "dimension" => Box::new(dimension::DimensionTool::new()),
        "eraser" => Box::new(eraser::EraserTool::new()),
        "measure" => Box::new(measure::MeasureTool::new()),
        "protractor" => Box::new(protractor::ProtractorTool::new()),
        "text" => Box::new(text::TextTool::new()),
        "section" => Box::new(section::SectionPlaneTool::new()),
        "change_axes" => Box::new(change_axes::ChangeAxesTool::new()),
        k => {
            let op = solids::SOLID_TOOLS
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, op)| *op)?;
            let _: SolidOp = op;
            Box::new(solids::SolidTool::new(op))
        }
    })
}

/// The side trays: `(object name, title)` — the Properties tray.
pub const TRAYS: [(&str, &str); 1] = [(PROPERTIES_TRAY, "Properties")];

/// The tray that holds the tray sections ([`SECTIONS`]).
pub const PROPERTIES_TRAY: &str = "tray_properties";

/// The toolbars in the order of the work: `(object name, title, action
/// ids)`. The members of a stack ([`toolbar_stacks::TOOLBAR_STACKS`]) fold
/// into one stacked button where the first of them stands; a
/// [`PANEL_PREFIX`] entry is a drop-down panel ([`TOOLBAR_PANELS`]).
pub const TOOLBAR_LAYOUT: [(&str, &str, &[&str]); 7] = [
    (
        "main",
        "Main",
        &[
            "save_tb",
            "undo",
            "redo",
            "select",
            "eraser",
            "paint",
            "eyedropper",
        ],
    ),
    (
        "draw",
        "Draw",
        &[
            "line",
            "freehand",
            "rectangle",
            "rotated_rect",
            "circle",
            "polygon",
            "arc",
            "arc3",
            "center_arc",
            "sector",
        ],
    ),
    (
        "model",
        "Model",
        &[
            "pushpull",
            "sweep",
            "offset",
            "fillet",
            "chamfer",
            "shell",
            "solid_union",
            "solid_subtract",
            "solid_trim",
            "solid_intersect",
            "solid_split",
            "merge_solids",
            "text3d",
        ],
    ),
    (
        "transform",
        "Transform",
        &["move", "rotate", "scale", "flip"],
    ),
    (
        "inspect",
        "Inspect",
        &[
            "measure",
            "protractor",
            "section",
            "solid_inspector",
            "model_info",
        ],
    ),
    (
        "view",
        "View",
        &[
            "nav:orbit",
            "nav:pan",
            "nav:zoom",
            "nav:zoom_window",
            "zoom_extents",
            "zoom_selection",
            "view:iso",
            "view:top",
            "view:front",
            "view:right",
            "view:left",
            "view:back",
            "view:bottom",
            "toggle_projection",
            "panel:styles",
        ],
    ),
    (
        "annotate",
        "Annotate",
        &["dimension", "text", "panel:dimension_style"],
    ),
];

/// The prefix of a drop-down panel in a bar's action ids.
pub const PANEL_PREFIX: &str = "panel:";

/// The drop-down panels a bar can hold: `(key, title)`; the key names the
/// icon too.
pub const TOOLBAR_PANELS: [(&str, &str); 2] =
    [("styles", "Styles"), ("dimension_style", "Dimension style")];

/// Every tool, in the order of the bars, then the context-menu ones.
const TOOL_KEYS: &[&str] = &[
    "select",
    "eraser",
    "paint",
    "line",
    "freehand",
    "rectangle",
    "rotated_rect",
    "circle",
    "polygon",
    "arc",
    "arc3",
    "center_arc",
    "sector",
    "pushpull",
    "sweep",
    "offset",
    "fillet",
    "chamfer",
    "shell",
    "solid_union",
    "solid_subtract",
    "solid_trim",
    "solid_intersect",
    "solid_split",
    "merge_solids",
    "move",
    "rotate",
    "scale",
    "flip",
    "measure",
    "protractor",
    "section",
    "dimension",
    "text",
    "change_axes",
];

/// Every tool key, in the order of the bars, then the context-menu ones.
pub fn tool_keys() -> Vec<&'static str> {
    TOOL_KEYS.to_vec()
}

const VIEW_TIPS: [(&str, &str, &str); 7] = [
    ("top", "Top", "Look straight down on the model, in plan."),
    (
        "bottom",
        "Bottom",
        "Look straight up at the model from below.",
    ),
    ("front", "Front", "Look at the model from the front."),
    ("back", "Back", "Look at the model from the back."),
    ("left", "Left", "Look at the model from the left."),
    ("right", "Right", "Look at the model from the right."),
    (
        "iso",
        "Isometric",
        "Look at the model from a corner above, at the same angle to the three axes.",
    ),
];

fn style_tip(name: &str) -> &'static str {
    match name {
        "Default" => "Faces in their colours, under the sky.",
        "Hidden line" => "White faces that hide what lies behind them — a clean line drawing.",
        "Monochrome" => "Every face in the front or back colour, without their colors.",
        "Wireframe" => "Only the edges: the faces are not drawn.",
        "X-ray" => "See-through faces, so the edges behind them show.",
        _ => "",
    }
}

pub const SAVE_TIP: &str = "Save the document to its .xtrude file.";
pub const TEXT3D_TIP: &str =
    "Build a text as an extruded solid, in the font and height you choose.";

/// What Repeat (Shift+R) replays: a tool, or a one-shot command with the
/// argument it took.
#[derive(Clone, Debug, PartialEq)]
pub enum Repeat {
    Tool(String),
    /// An action id ([`MainWindow::trigger`]).
    Command(String),
}

/// A timed status-bar message over the base hint.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusMessage {
    pub text: String,
    pub msec: u32,
}

/// A toolbar: `(object name, title, action ids)`.
pub type ToolbarDef = (String, String, Vec<String>);

pub struct MainWindow {
    pub viewport: Viewport,
    pub actions: ActionList,
    /// The menu bar: `(title, entries)`.
    pub menus: Vec<(String, Vec<MenuItem>)>,
    pub toolbars: Vec<ToolbarDef>,
    /// Where each toolbar sits, saved on every change.
    pub toolbar_layout: ToolbarLayout,
    /// The member each stacked button shows, saved on every change.
    pub toolbar_stacks: ToolbarStacks,
    active_key: Option<String>,
    /// Tool keys a workspace allows (`None` = every tool).
    pub tool_filter: Option<HashSet<String>>,
    last_command: Option<(String, Repeat)>,
    /// A right click asked for the context menu; the shell opens it
    /// ([`Self::take_context_menu_request`]).
    context_menu_asked: bool,
    pub current_path: Option<PathBuf>,
    /// The name of an IMPORTED file shown in the title until saved.
    pub import_name: Option<String>,
    /// A `.skp` or STEP file from the command line, imported after the
    /// first paint.
    pub pending_import: Option<PathBuf>,
    /// The STEP helper program to run; `None` finds it as
    /// `HelperBackend::locate` does (next to the executable). Tests point
    /// it at a stand-in.
    pub step_helper: Option<PathBuf>,
    /// The progress dialog of the running open or import (one at most).
    pub progress: Option<progress::ImportProgress>,
    /// The import parsing on a worker thread.
    pub import_job: Option<progress::ImportJob>,
    /// Leave threaded imports running for the shell to poll; otherwise
    /// (tests, scripts) the window waits for them.
    pub background_imports: bool,
    /// Model Info was asked for: the shell opens the dialog.
    pub model_info_requested: bool,
    /// The open Solid Inspector (one instance; modeless, the shell draws it).
    pub solid_inspector: Option<solid_inspector::SolidInspector>,
    saved_content: i64,
    /// The latest timed message (the shell shows it, then the base hint).
    pub status_message: Option<StatusMessage>,
    pub value_box_buffer: String,
    pub value_box_live: String,
    pub dialogs: Box<dyn Dialogs>,
    /// The system clipboard Copy offers the model clipboard on.
    pub system_clipboard: Box<dyn clipboard::SystemClipboard>,
    /// The style left for X-ray, by document (`scene_serial`).
    style_before_xray: Option<(u64, Style)>,
    scene_serial: u64,
    pub title: String,
    /// The window asked to close (File ▸ Quit, confirmed).
    pub quit_requested: bool,
    /// Every viewport signal the window has handled, for the shell and the
    /// trays to read (and drain) after each event.
    pub seen_events: Vec<ViewportEvent>,
    /// The Lua extensions and their menu.
    pub extensions: extensions::Extensions,
    /// A folder to show in the file manager (Open Extensions Folder): the
    /// shell opens it.
    pub open_folder_requested: Option<PathBuf>,
    /// The device drawing the viewport, once the shell has one
    /// ([`MainWindow::set_gpu_info`]).
    pub gpu: Option<xtrude_render::gpuinfo::GpuInfo>,
}

impl Default for MainWindow {
    fn default() -> Self {
        MainWindow::new()
    }
}

impl MainWindow {
    pub fn new() -> MainWindow {
        let mut viewport = Viewport::new();
        viewport.history.max_steps = settings::read(&keys::UNDO_STEPS).max(0) as usize;
        let mut w = MainWindow {
            viewport,
            actions: ActionList::default(),
            menus: Vec::new(),
            toolbars: Vec::new(),
            toolbar_layout: ToolbarLayout::factory(),
            toolbar_stacks: ToolbarStacks::load(),
            active_key: None,
            tool_filter: None,
            last_command: None,
            context_menu_asked: false,
            current_path: None,
            import_name: None,
            gpu: None,
            pending_import: None,
            step_helper: None,
            progress: None,
            import_job: None,
            background_imports: false,
            model_info_requested: false,
            solid_inspector: None,
            saved_content: 0,
            status_message: None,
            value_box_buffer: String::new(),
            value_box_live: String::new(),
            dialogs: Box::new(NoDialogs),
            system_clipboard: Box::new(clipboard::MemoryClipboard::default()),
            style_before_xray: None,
            scene_serial: 0,
            title: String::new(),
            quit_requested: false,
            seen_events: Vec::new(),
            extensions: extensions::Extensions::default(),
            open_folder_requested: None,
        };
        w.build_actions();
        w.reload_extensions();
        let known: Vec<String> = w.toolbars.iter().map(|(id, _, _)| id.clone()).collect();
        w.toolbar_layout = ToolbarLayout::load(&known);
        w.build_menus();
        w.saved_content = w.viewport.scene.content_version() as i64;
        w.activate_tool("select");
        w.apply_new_document_units();
        w.frame_new_document();
        w.sync_style_menu();
        w.update_title();
        w
    }

    // ---- actions -----------------------------------------------------------------------
    fn build_actions(&mut self) {
        let a = &mut self.actions;
        // the tools, one checkable action each
        for key in tool_keys() {
            let Some(tool) = make_tool(key) else { continue };
            let mut keys: Vec<&str> = Vec::new();
            if let Some(s) = tool.info().shortcut.filter(|s| !s.is_empty()) {
                keys.push(s);
            }
            if let Some(s) = tool.info().shortcut_alt {
                keys.push(s);
            }
            if key == "select" {
                keys = vec!["Space"]; // Spacebar returns to Select
            }
            let mut act = Action::new(key, &tr(tool.info().name, &[]))
                .keys(&keys)
                .checkable(false)
                .icon(key);
            if let Some(d) = tool.info().description {
                act.status_tip = tr(d, &[]);
            }
            a.add(act);
        }
        a.add(
            Action::new("save_tb", &tr("Save", &[]))
                .tip(&tr(SAVE_TIP, &[]))
                .icon("save"),
        );
        a.add(
            Action::new("eyedropper", &tr("Sample Color", &[]))
                .tip(&tr(
                    "Sample Color — click a face to pick up its color (or hold Alt with the Paint tool)",
                    &[],
                ))
                .checkable(false)
                .icon("eyedropper"),
        );
        a.add(
            Action::new("show_section_planes", &tr("Section Planes", &[]))
                .tip(&tr(
                    "Show the section planes themselves; the cut they make stays either way.",
                    &[],
                ))
                .checkable(true)
                .icon("section_planes"),
        );
        a.add(
            Action::new("show_section_cuts", &tr("Section Cuts", &[]))
                .tip(&tr(
                    "Cut the model open at its active section planes; off, the model shows whole.",
                    &[],
                ))
                .checkable(true)
                .icon("section_cuts"),
        );
        a.add(
            Action::new("section_fill", &tr("Section Fill", &[]))
                .tip(&tr(
                    "Fill the cut faces of a section with a solid colour.",
                    &[],
                ))
                .checkable(true)
                .icon("section_fill"),
        );
        a.add(
            Action::new("text3d", &tr("3D Text…", &[]))
                .tip(&tr(TEXT3D_TIP, &[]))
                .icon("text3d"),
        );
        for (key, label, short, tip) in [
            (
                "orbit",
                "Orbit",
                "O",
                "Orbit (O) — left-drag to rotate the view",
            ),
            ("pan", "Pan", "H", "Pan (H) — left-drag to slide the view"),
            (
                "zoom",
                "Zoom",
                "Z",
                "Zoom (Z) — drag up/down to zoom in/out",
            ),
            (
                "zoom_window",
                "Zoom Window",
                "",
                "Zoom Window — drag a box to zoom to that region",
            ),
        ] {
            a.add(
                Action::new(&format!("nav:{key}"), &tr(label, &[]))
                    .keys(&[short])
                    .tip(&tr(tip, &[]))
                    .checkable(false)
                    .icon(key),
            );
        }
        a.add(
            Action::new("zoom_extents", &tr("Zoom Extents", &[]))
                .keys(&["Shift+Z", "F2"])
                .tip(&tr("Frame the whole model in the view.", &[]))
                .icon("zoom_extents"),
        );
        a.add(
            Action::new("zoom_selection", &tr("Zoom Selection", &[]))
                .keys(&["Ctrl+Alt+Z"])
                .tip(&tr("Frame the selection in the view.", &[]))
                .icon("zoom_selection"),
        );
        for (key, label, tip) in VIEW_TIPS {
            a.add(
                Action::new(&format!("view:{key}"), &tr(label, &[]))
                    .tip(&tr(tip, &[]))
                    .icon(&format!("view_{key}")),
            );
        }
        // File
        use actions::std_keys::*;
        for (id, text, keys, tip) in [
            ("new", "New", NEW, "Start a new, empty model in this window."),
            (
                "new_window",
                "New Window",
                "Ctrl+Shift+N",
                "Open another Xtrude CAD window with its own document; copy and paste work between them.",
            ),
            (
                "open",
                "Open…",
                OPEN,
                "Open an Xtrude CAD document (.xtrude) in place of this one; an IngeTrazo document (.igz), or a file of another format Xtrude CAD reads, opens as a new, unsaved one.",
            ),
            ("save", "Save", SAVE, SAVE_TIP),
            (
                "save_as",
                "Save As…",
                SAVE_AS,
                "Save the document under another name or in another folder.",
            ),
            (
                "quit",
                "Quit",
                QUIT,
                "Close Xtrude CAD, offering to save what has not been saved.",
            ),
        ] {
            a.add(Action::new(id, &tr(text, &[])).keys(&[keys]).tip(&tr(tip, &[])));
        }
        for (id, text, tip) in [
            (
                "import",
                "Import…",
                "Bring a file into this model: another document as one component, or a STEP, STL, OBJ, glTF, COLLADA, DXF or .skp model.",
            ),
            (
                "export",
                "Export…",
                "Save the model in another format: STL in millimetres for 3D printing, 3MF for slicers, OBJ, glTF or COLLADA.",
            ),
        ] {
            a.add(Action::new(id, &tr(text, &[])).tip(&tr(tip, &[])));
        }
        // Edit
        for (id, text, keys, tip) in [
            ("undo", "Undo", vec![UNDO], "Take back the last change to the model."),
            (
                "redo",
                "Redo",
                vec!["Ctrl+Y", "Ctrl+Shift+Z"],
                "Bring back the change that was just undone.",
            ),
            (
                "repeat_last_command",
                "Repeat last command",
                vec!["Shift+R"],
                "Run the last command again, on what is selected now.",
            ),
            (
                "cut",
                "Cut",
                vec![CUT],
                "Take the selection out of the model and keep it to paste elsewhere.",
            ),
            (
                "copy",
                "Copy",
                vec![COPY],
                "Keep a copy of the selection to paste here or in another Xtrude CAD window.",
            ),
            (
                "paste",
                "Paste",
                vec![PASTE],
                "Place what was cut or copied, with a click where it goes.",
            ),
            (
                "paste_in_place",
                "Paste in Place",
                vec!["Ctrl+Alt+V"],
                "Paste the copy exactly where the original was, in the group that is open — to move things into or out of groups without shifting them.",
            ),
            (
                "select_all",
                "Select All",
                vec![SELECT_ALL],
                "Select everything: edges, faces, groups and dimensions.",
            ),
            (
                "invert_selection",
                "Invert Selection",
                vec!["Ctrl+Shift+I"],
                "Select what is not selected, and drop what is.",
            ),
            (
                "make_group",
                "Make Group",
                vec!["Ctrl+G"],
                "Wrap the selection in a group that moves as one and keeps apart from the geometry around it.",
            ),
            (
                "make_component",
                "Make Component…",
                vec!["G"],
                "Turn the selection into a component: every copy shares it, so changing one changes them all.",
            ),
            (
                "explode_group",
                "Explode Group",
                vec!["Ctrl+Shift+G"],
                "Break the selected groups apart, back into the geometry around them.",
            ),
            (
                "split_into_pieces",
                "Split into Pieces",
                vec![],
                "Regroup the selected group by the pieces that do not touch; explode it afterwards for each piece on its own.",
            ),
            (
                "simplify_mesh",
                "Simplify Mesh…",
                vec![],
                "Simplify Mesh — merge coplanar and near-coplanar faces",
            ),
            (
                "delete_guides",
                "Delete Guides",
                vec![],
                "Remove every guide line and guide point from the model.",
            ),
            (
                "hide",
                "Hide",
                vec![],
                "Stop showing the selected objects, faces and edges; they stay in the document until unhidden.",
            ),
            (
                "unhide_selected",
                "Selected",
                vec![],
                "Show again the hidden things that are selected — Hidden Groups or Hidden Edges and Faces lets you select them.",
            ),
            ("unhide_last", "Last", vec![], "Show again what the last hiding put away."),
            ("unhide_all", "All", vec![], "Show again everything that is hidden."),
            (
                "reverse_faces",
                "Reverse Faces",
                vec![],
                "Swap the front and back sides of the selected faces.",
            ),
            (
                "orient_faces",
                "Orient Faces",
                vec![],
                "Turn every face connected to the selected one so its front side matches.",
            ),
            (
                "heal_overlaps",
                "Heal Overlapping Faces",
                vec![],
                "Remove the faces left lying over their own subdivisions after drawing or erasing.",
            ),
            (
                "rebuild_planar",
                "Rebuild Faces (Planar)",
                vec![],
                "Work the faces out again from the edges: on the plane of the selected faces, or over the whole drawing when it is flat.",
            ),
            (
                "intersect:model",
                "With Everything",
                vec![],
                "Add edges wherever the selected faces cross the rest of the model, groups included.",
            ),
            (
                "intersect:selection",
                "Within Selection",
                vec![],
                "Add edges where the selected faces cross one another.",
            ),
            (
                "intersect:context",
                "Within Open Group",
                vec![],
                "Add edges where the selected faces cross the rest of the group being edited, or of the model.",
            ),
        ] {
            a.add(Action::new(id, &tr(text, &[])).keys(&keys).tip(&tr(tip, &[])));
        }
        if let Some(r) = a.get_mut("repeat_last_command") {
            r.enabled = false;
        }
        for (id, key) in [("undo", "undo"), ("redo", "redo")] {
            if let Some(x) = a.get_mut(id) {
                x.icon = Some(key.into());
            }
        }
        // Camera
        a.add(
            Action::new("toggle_projection", &tr("Toggle Perspective / Parallel", &[]))
                .keys(&["Shift+P"])
                .tip(&tr(
                    "Switch between perspective and a parallel projection, where sizes do not shrink with distance.",
                    &[],
                ))
                .icon("projection"),
        );
        for st in builtin_styles() {
            a.add(
                Action::new(&format!("style:{}", st.name), &tr(&st.name, &[]))
                    .tip(&tr(style_tip(&st.name), &[]))
                    .checkable(false),
            );
        }
        a.add(
            Action::new("toggle_xray", &tr("Toggle X-ray", &[]))
                .keys(&["Alt+X"])
                .tip(&tr(
                    "Switch to X-ray, or back to the style you were in.",
                    &[],
                )),
        );
        a.add(
            Action::new("style_edges", &tr("Edges", &[]))
                .tip(&tr("Draw the edges of the model.", &[]))
                .checkable(true),
        );
        a.add(
            Action::new("style_silhouettes", &tr("Silhouette Edges", &[]))
                .tip(&tr(
                    "Draw the outline of each shape with a thicker line.",
                    &[],
                ))
                .checkable(false),
        );
        a.add(
            Action::new("style_hidden_lines", &tr("Hidden Lines", &[]))
                .keys(&["K"])
                .tip(&tr(
                    "Draw the edges hidden behind faces as dashed lines.",
                    &[],
                ))
                .checkable(false),
        );
        for (mode, label, tip) in [
            (RestMode::Normal, "Show normally", "While a group is edited, the rest of the model shows as usual."),
            (
                RestMode::Fade,
                "Fade",
                "While a group is edited, the rest of the model shows pale, so the group stands out.",
            ),
            (
                RestMode::Hide,
                "Hide (fastest)",
                "While a group is edited, the rest of the model is not drawn — the quickest on a heavy model.",
            ),
        ] {
            a.add(
                Action::new(&format!("rest:{}", mode.key()), &tr(label, &[]))
                    .tip(&tr(tip, &[]))
                    .checkable(false),
            );
        }
        a.add(
            Action::new("show_hidden_groups", &tr("Hidden Groups", &[]))
                .tip(&tr(
                    "Show hidden groups and components as a see-through grid, so they can be selected again.",
                    &[],
                ))
                .checkable(false),
        );
        a.add(
            Action::new(
                "show_hidden_edges_faces",
                &tr("Hidden Edges and Faces", &[]),
            )
            .tip(&tr(
                "Show hidden faces and edges as a see-through grid, so they can be selected again.",
                &[],
            ))
            .checkable(false),
        );
        a.add(
            Action::new("cancel_tool", &tr("Cancel current tool", &[]))
                .keys(&["Esc"])
                .tip(&tr(
                    "Stop what the tool is doing; with nothing in progress, clear the selection.",
                    &[],
                )),
        );
        // Window
        a.add(
            Action::new("clean_screen", &tr("Clean screen", &[]))
                .keys(&["Ctrl+0"])
                .tip(&tr(
                    "Fold away every toolbar, panel and bar so only the model shows; once more brings them all back.",
                    &[],
                ))
                .checkable(false),
        );
        a.add(
            Action::new("sidebar", &tr("Sidebar", &[]))
                .keys(&["Ctrl+F5"])
                .tip(&tr("Show or hide the trays on the right.", &[]))
                .checkable(true),
        );
        a.add(
            Action::new("reset_toolbar_layout", &tr("Reset toolbar layout", &[])).tip(&tr(
                "Put every toolbar back in its first place and show them all; the side panels stay as they are.",
                &[],
            )),
        );
        a.add(
            Action::new("reset_panel_layout", &tr("Reset panel layout", &[])).tip(&tr(
                "Put every panel back in its first place and show them all; the toolbars stay as they are.",
                &[],
            )),
        );
        a.add(
            Action::new("preferences", &tr("Preferences…", &[])).tip(&tr(
                "Change the units, icons and the rest of the settings.",
                &[],
            )),
        );
        a.add(
            Action::new("model_info", &tr(model_info::NAME, &[]))
                .tip(&tr(model_info::DESCRIPTION, &[]))
                .icon("model_info"),
        );
        a.add(
            Action::new("solid_inspector", &tr(solid_inspector::NAME, &[]))
                .tip(&tr(solid_inspector::DESCRIPTION, &[]))
                .icon("solid_inspector"),
        );
        a.add(Action::new("about", &tr("About Xtrude CAD", &[])).tip(&tr(
            "The version, the authors and the licence of Xtrude CAD.",
            &[],
        )));
        // toolbars, their stacks folded
        self.toolbars = TOOLBAR_LAYOUT
            .iter()
            .map(|(o, t, ids)| {
                let ids = ids.iter().map(|k| k.to_string()).collect();
                (o.to_string(), tr(t, &[]), toolbar_stacks::fold(ids))
            })
            .collect();
    }

    fn build_menus(&mut self) {
        use MenuItem::{Action as A, Menu as M, Separator as S};
        let act = |id: &str| A(id.to_string());
        let file = vec![
            act("new"),
            act("new_window"),
            act("open"),
            M(tr("Open Recent", &[]), vec![MenuItem::Dynamic("recent")]),
            act("save"),
            act("save_as"),
            S,
            act("import"),
            act("export"),
            S,
            act("quit"),
        ];
        let edit = vec![
            act("undo"),
            act("redo"),
            act("repeat_last_command"),
            S,
            act("cut"),
            act("copy"),
            act("paste"),
            act("paste_in_place"),
            S,
            act("select"),
            act("select_all"),
            act("invert_selection"),
            S,
            act("make_group"),
            act("make_component"),
            act("explode_group"),
            S,
            act("hide"),
            M(
                tr("Unhide", &[]),
                vec![
                    act("unhide_selected"),
                    act("unhide_last"),
                    act("unhide_all"),
                ],
            ),
            act("delete_guides"),
            S,
            act("cancel_tool"),
        ];
        let mut style_items: Vec<MenuItem> = builtin_styles()
            .iter()
            .map(|s| act(&format!("style:{}", s.name)))
            .collect();
        style_items.extend([
            S,
            act("toggle_xray"),
            S,
            act("style_edges"),
            act("style_silhouettes"),
            act("style_hidden_lines"),
        ]);
        let view = vec![
            M(
                tr("Standard Views", &[]),
                ["top", "bottom", "front", "back", "left", "right", "iso"]
                    .map(|k| act(&format!("view:{k}")))
                    .to_vec(),
            ),
            act("zoom_extents"),
            act("zoom_selection"),
            S,
            act("toggle_projection"),
            S,
            M(tr("Style", &[]), style_items),
            M(
                tr("Rest of model while editing", &[]),
                vec![act("rest:normal"), act("rest:fade"), act("rest:hide")],
            ),
            S,
            act("show_section_planes"),
            act("show_section_cuts"),
            act("section_fill"),
            S,
            act("show_hidden_groups"),
            act("show_hidden_edges_faces"),
            S,
            act("nav:orbit"),
            act("nav:pan"),
            act("nav:zoom"),
            act("nav:zoom_window"),
        ];
        let draw = vec![
            act("line"),
            act("freehand"),
            M(
                tr("Arcs", &[]),
                ["arc", "arc3", "center_arc", "sector"].map(act).to_vec(),
            ),
            M(
                tr("Shapes", &[]),
                ["rectangle", "rotated_rect", "circle", "polygon"]
                    .map(act)
                    .to_vec(),
            ),
            S,
            act("eraser"),
        ];
        let model = vec![
            act("pushpull"),
            act("sweep"),
            act("offset"),
            act("fillet"),
            act("chamfer"),
            act("shell"),
            act("text3d"),
            S,
            act("move"),
            act("rotate"),
            act("scale"),
            act("flip"),
            S,
            M(
                tr("Solids", &[]),
                [
                    "solid_union",
                    "solid_subtract",
                    "solid_trim",
                    "solid_intersect",
                    "solid_split",
                    "merge_solids",
                ]
                .map(act)
                .to_vec(),
            ),
            M(
                tr("Add Crossing Edges", &[]),
                vec![
                    act("intersect:model"),
                    act("intersect:selection"),
                    act("intersect:context"),
                ],
            ),
            act("split_into_pieces"),
            S,
            M(
                tr("Mesh repair", &[]),
                [
                    "reverse_faces",
                    "orient_faces",
                    "heal_overlaps",
                    "rebuild_planar",
                    "simplify_mesh",
                ]
                .map(act)
                .to_vec(),
            ),
            S,
            act("paint"),
            act("eyedropper"),
        ];
        let inspect = vec![
            act("measure"),
            act("protractor"),
            act("section"),
            S,
            act("solid_inspector"),
            act("model_info"),
            S,
            M(tr("Annotations", &[]), vec![act("dimension"), act("text")]),
        ];
        let window = vec![
            M(tr("Panels", &[]), vec![MenuItem::Dynamic("panels")]),
            M(tr("Toolbars", &[]), vec![MenuItem::Dynamic("toolbars")]),
            S,
            act("clean_screen"),
            act("sidebar"),
            S,
            act("preferences"),
        ];
        self.menus = vec![
            (tr("File", &[]), file),
            (tr("Edit", &[]), edit),
            (tr("View", &[]), view),
            (tr("Draw", &[]), draw),
            (tr("Model", &[]), model),
            (tr("Inspect", &[]), inspect),
            (extensions::menu_title(), self.extensions_menu()),
            (tr("Window", &[]), window),
            (tr("Help", &[]), vec![act("about")]),
        ];
    }

    // ---- tools -------------------------------------------------------------------------
    /// The key of the tool in hand (`None` for a camera mode or a
    /// transient tool like Paste).
    pub fn active_tool_key(&self) -> Option<&str> {
        self.active_key.as_deref()
    }

    /// Pick a tool by its toolbar key.
    pub fn activate_tool(&mut self, key: &str) {
        let Some(tool) = make_tool(key) else {
            return;
        };
        if let Some(filter) = &self.tool_filter {
            if !filter.contains(key) {
                self.show_message(
                    &tr(
                        "{name} is not available here.",
                        &[("name", &tr(tool.info().name, &[]))],
                    ),
                    3000,
                );
                return;
            }
        }
        let same = self.active_key.as_deref() == Some(key)
            && self.viewport.nav.mode.is_none()
            && self
                .viewport
                .active_tool()
                .is_some_and(|t| t.info().key == key);
        if same {
            // Picking the tool you already hold starts it over.
            self.viewport.restart_tool();
        } else {
            self.viewport.set_active_tool(Some(tool));
        }
        self.active_key = Some(key.to_string());
        self.check_tool_action(Some(key));
        self.toolbar_stacks.follow(key);
        // Select is where Repeat is pressed FROM, and the tool entered from
        // a right-click needs the group that click was on.
        if key != "select" && key != "change_axes" {
            let name = make_tool(key).map(|t| t.info().name).unwrap_or(key);
            self.remember_command(name, Repeat::Tool(key.to_string()));
        }
        self.pump_events();
    }

    /// Hand the viewport a transient tool (Paste, Place Group, a Change
    /// Axes with its target) that no toolbar button stands for.
    pub fn activate_transient(&mut self, tool: Box<dyn Tool>) {
        let key = tool.info().key.to_string();
        self.viewport.set_active_tool(Some(tool));
        self.active_key = (key == "change_axes").then_some(key);
        self.check_tool_action(self.active_key.clone().as_deref());
        self.pump_events();
    }

    fn check_tool_action(&mut self, key: Option<&str>) {
        let keys: Vec<String> = tool_keys().iter().map(|k| k.to_string()).collect();
        for k in keys.iter().map(String::as_str).chain([
            "nav:orbit",
            "nav:pan",
            "nav:zoom",
            "nav:zoom_window",
        ]) {
            if let Some(a) = self.actions.get_mut(k) {
                a.checked = Some(k) == key;
            }
        }
    }

    /// Whether the action of a tool or camera mode shows checked.
    pub fn is_checked(&self, id: &str) -> bool {
        self.actions.get(id).is_some_and(|a| a.checked)
    }

    /// The member action stacked button `id` shows (the last member run,
    /// or its first).
    pub fn stack_current(&self, id: &str) -> Option<&str> {
        self.toolbar_stacks.current(id)
    }

    /// A member picked from a stacked button's list: run it, as its own
    /// button would; the stack shows it once it has run. Returns whether
    /// `action` is a member of `id`.
    pub fn pick_in_stack(&mut self, id: &str, action: &str) -> bool {
        if !toolbar_stacks::members(id).is_some_and(|ms| ms.contains(&action)) {
            return false;
        }
        self.trigger(action);
        true
    }

    pub fn activate_nav(&mut self, key: &str) {
        let mode = match key {
            "orbit" => NavMode::Orbit,
            "pan" => NavMode::Pan,
            "zoom" => NavMode::Zoom,
            "zoom_window" => NavMode::ZoomWindow,
            _ => return,
        };
        self.viewport.set_nav_mode(Some(mode));
        self.active_key = None;
        let id = format!("nav:{key}");
        self.check_tool_action(Some(&id));
        self.toolbar_stacks.follow(&id);
        self.pump_events();
    }

    // ---- repeat last command -------------------------------------------------------------
    pub fn remember_command(&mut self, label: &str, run: Repeat) {
        let text = tr("Repeat {name}", &[("name", &tr(label, &[]))]);
        self.last_command = Some((label.to_string(), run));
        if let Some(a) = self.actions.get_mut("repeat_last_command") {
            a.text = text;
            a.enabled = true;
        }
    }

    /// What Repeat would run: `(label, how)`.
    pub fn last_command(&self) -> Option<&(String, Repeat)> {
        self.last_command.as_ref()
    }

    /// Run again the last tool picked or one-shot command applied — a
    /// command on the CURRENT selection.
    pub fn repeat_last_command(&mut self) -> bool {
        let Some((label, run)) = self.last_command.clone() else {
            self.viewport
                .flash_status(&tr("Nothing to repeat yet", &[]), 3000);
            self.pump_events();
            return false;
        };
        match run {
            Repeat::Tool(k) => self.activate_tool(&k),
            Repeat::Command(id) => {
                self.trigger(&id);
            }
        }
        self.viewport
            .flash_status(&tr("Repeat: {name}", &[("name", &tr(&label, &[]))]), 2000);
        self.pump_events();
        true
    }

    /// «Shift+R: repeat Line», on the right while Select is up.
    pub fn repeat_hint(&self) -> Option<String> {
        let (label, _) = self.last_command.as_ref()?;
        if self.active_key.as_deref() != Some("select") {
            return None;
        }
        let keys = self
            .actions
            .get("repeat_last_command")
            .map(Action::shortcut_text)
            .unwrap_or_default();
        Some(if keys.is_empty() {
            tr("Repeat {name}", &[("name", &tr(label, &[]))])
        } else {
            tr(
                "{keys}: repeat {name}",
                &[("keys", &keys), ("name", &tr(label, &[]))],
            )
        })
    }

    // ---- triggering ----------------------------------------------------------------------
    /// Run the action `id` (a menu entry, a button, a shortcut).
    /// Returns whether an action by that name exists.
    pub fn trigger(&mut self, id: &str) -> bool {
        let handled = self.ask_as(Asker::Trigger(id.to_string()), |w| w.dispatch(id));
        self.pump_events();
        handled
    }

    /// Run `f` as `asker`: a question `f` leaves pending is answered by
    /// running `asker` again ([`Asker`]). The innermost asker is the one
    /// that asked: an action a shortcut fires, a section plane a click
    /// placed.
    pub fn ask_as<R>(&mut self, asker: Asker, f: impl FnOnce(&mut MainWindow) -> R) -> R {
        let outer = self.dialogs.set_asker(Some(asker));
        let r = f(self);
        self.dialogs.set_asker(outer);
        r
    }

    fn remembered(&mut self, label: &str, id: &str) {
        self.remember_command(label, Repeat::Command(id.to_string()));
    }

    /// Run the one-shot command `id` and remember it for Repeat if it did
    /// something: ran a command on the history. A refusal, a failed command
    /// or a cancelled dialog leaves Repeat offering what it offered.
    fn run_repeatable(&mut self, label: &str, id: &str, run: impl FnOnce(&mut MainWindow)) {
        let before = self.viewport.history.commands_run();
        run(self);
        if self.viewport.history.commands_run() != before {
            self.remembered(label, id);
        }
    }

    fn dispatch(&mut self, id: &str) -> bool {
        if let Some(act) = self.actions.get(id) {
            if !act.enabled {
                return true;
            }
        }
        if tool_keys().contains(&id) {
            self.activate_tool(id);
            return true;
        }
        if let Some(p) = id.strip_prefix("open_recent:") {
            self.open_recent(Path::new(p));
            return true;
        }
        if id == "clear_recent" {
            files::clear_recent();
            return true;
        }
        if let Some(key) = id.strip_prefix("tray_section:") {
            let shown = self.tray_section_visible(key);
            self.set_tray_section_shown(key, !shown);
            return true;
        }
        if let Some(bar) = id.strip_prefix("toolbar:") {
            self.toggle_toolbar(bar);
            return true;
        }
        if let Some(k) = id.strip_prefix("nav:") {
            self.activate_nav(k);
            return true;
        }
        if let Some(k) = id.strip_prefix("view:") {
            self.on_standard_view(k);
            self.toolbar_stacks.follow(id);
            return true;
        }
        if let Some(name) = id.strip_prefix("style:") {
            if let Some(st) = xtrude_model::style::style_by_name(name) {
                self.apply_display_style(st);
            }
            return true;
        }
        if let Some(k) = id.strip_prefix("rest:") {
            if let Some(mode) = RestMode::from_key(k) {
                self.viewport.set_edit_rest_mode(mode);
            }
            self.sync_rest_menu();
            return true;
        }
        if let Some(k) = id.strip_prefix("intersect:") {
            let mode = match k {
                "model" => IntersectMode::Model,
                "selection" => IntersectMode::Selection,
                _ => IntersectMode::Context,
            };
            self.run_repeatable("Add Crossing Edges", id, |w| w.on_add_crossing_edges(mode));
            return true;
        }
        if self.dispatch_context(id) || self.dispatch_extensions(id) {
            return true;
        }
        match id {
            "save" | "save_tb" => self.on_save(),
            "save_as" => self.on_save_as(),
            "import" => self.on_import(),
            "export" => self.on_export(),
            "new" => self.on_new(),
            "open" => self.on_open(),
            "quit" => self.request_quit(),
            "new_window" => self.on_new_window(),
            "undo" => self.on_undo(),
            "redo" => self.on_redo(),
            "repeat_last_command" => {
                self.repeat_last_command();
            }
            "cut" => {
                self.viewport.cut_selection();
            }
            "copy" => {
                self.viewport.copy_selection();
            }
            "paste" => self.on_paste(),
            "paste_in_place" => self.on_paste_in_place(),
            "select_all" => self.on_select_all(),
            "invert_selection" => self.on_invert_selection(),
            "make_group" => self.run_repeatable("Make Group", id, |w| w.on_make_group()),
            "make_component" => {
                self.run_repeatable("Make Component", id, |w| w.on_make_component())
            }
            "make_independent" => {
                self.run_repeatable("Make Independent", id, |w| w.on_make_independent())
            }
            "merge_groups" => self.run_repeatable("Merge Groups", id, |w| w.on_merge_groups()),
            "explode_group" => self.run_repeatable("Explode Group", id, |w| w.on_explode_group()),
            "split_into_pieces" => {
                self.run_repeatable("Split into Pieces", id, |w| w.on_split_into_pieces())
            }
            "simplify_mesh" => self.on_simplify_mesh(),
            "delete_guides" => self.run_repeatable("Delete Guides", id, |w| w.on_delete_guides()),
            "hide" => self.run_repeatable("Hide", id, |w| w.on_hide()),
            "unhide_selected" => self.on_unhide_selected(),
            "unhide_last" => self.on_unhide_last(),
            "unhide_all" => self.on_unhide_all(),
            "divide" => self.run_repeatable("Divide", id, |w| w.on_divide()),
            "reverse_faces" => self.run_repeatable("Reverse Faces", id, |w| w.on_reverse_faces()),
            "orient_faces" => self.run_repeatable("Orient Faces", id, |w| w.on_orient_faces()),
            "heal_overlaps" => {
                self.run_repeatable("Heal Overlapping Faces", id, |w| w.on_heal_overlaps())
            }
            "rebuild_planar" => {
                self.run_repeatable("Rebuild Faces (Planar)", id, |w| w.on_rebuild_planar())
            }
            "delete_selection" => self.on_delete_selection(),
            "zoom_extents" => self.on_zoom_extents(),
            "zoom_selection" => self.on_zoom_selection(),
            "toggle_projection" => self.viewport.toggle_projection(),
            "toggle_xray" => self.toggle_xray(),
            "style_edges" => self.toggle_style_field("edges"),
            "style_silhouettes" => self.toggle_style_field("profiles"),
            "style_hidden_lines" => self.toggle_style_field("back_edges"),
            "section_fill" => self.toggle_style_field("section_fill"),
            "show_section_planes" => {
                self.viewport.scene.show_section_planes = !self.viewport.scene.show_section_planes;
                self.sync_section_menu();
                self.viewport.update();
            }
            "show_section_cuts" => {
                self.viewport.scene.show_section_cuts = !self.viewport.scene.show_section_cuts;
                self.sync_section_menu();
                self.viewport.update();
            }
            "show_hidden_groups" => {
                let on = !self.viewport.scene.show_hidden_objects;
                self.set_hidden_view(true, on);
            }
            "show_hidden_edges_faces" => {
                let on = !self.viewport.scene.show_hidden_geometry;
                self.set_hidden_view(false, on);
            }
            "eyedropper" => {
                let on = !self.is_checked("eyedropper");
                self.on_toggle_eyedropper(on);
            }
            "cancel_tool" => self.viewport.escape(),
            "text3d" => {
                // no undo step until the text is placed: it ran when the
                // placing tool is in hand
                if self.on_insert_3d_text() {
                    self.remembered("3D Text", id);
                }
            }
            "clean_screen" | "sidebar" => {
                if let Some(a) = self.actions.get_mut(id) {
                    a.checked = !a.checked;
                }
            }
            "model_info" => {
                // one-shot: the tool in hand stays
                self.remembered(model_info::NAME, id);
                self.model_info_requested = true;
            }
            "solid_inspector" => {
                // one-shot like Model Info; a second call refreshes the open
                // inspector instead of opening another (`on_activate`)
                self.remembered(solid_inspector::NAME, id);
                let scene = &self.viewport.scene;
                match self.solid_inspector.as_mut() {
                    Some(dlg) => dlg.refresh(scene),
                    None => {
                        self.solid_inspector = Some(solid_inspector::SolidInspector::new(scene))
                    }
                }
            }
            "reset_toolbar_layout" => self.reset_toolbar_layout(),
            "reset_panel_layout" => self.reset_panel_layout(),
            "preferences" | "about" => {}
            _ => return false,
        }
        true
    }

    // ---- the side trays ----------------------------------------------------------------
    /// The side trays the sidebar holds: `(name, title)`.
    pub fn trays(&self) -> Vec<(String, String)> {
        TRAYS
            .iter()
            .map(|(n, t)| (n.to_string(), tr(t, &[])))
            .collect()
    }

    fn hidden_trays() -> Vec<String> {
        match settings::read(&keys::HIDDEN_TRAYS) {
            serde_json::Value::Array(a) => a
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            serde_json::Value::String(s) => vec![s],
            _ => Vec::new(),
        }
    }

    /// Whether the tray `name` is shown (its tab, not the fold).
    pub fn tray_shown(&self, name: &str) -> bool {
        !Self::hidden_trays().iter().any(|n| n == name)
    }

    /// Show or hide one tray and remember it; asking for a tray unfolds a
    /// folded sidebar (`set_tray_shown`).
    pub fn set_tray_shown(&mut self, name: &str, shown: bool) {
        if shown && !self.sidebar_visible() {
            self.set_sidebar_visible(true);
        }
        let mut hidden = Self::hidden_trays();
        hidden.retain(|n| n != name);
        if !shown {
            hidden.push(name.to_string());
        }
        hidden.sort();
        settings::write(&keys::HIDDEN_TRAYS, &serde_json::json!(hidden));
    }

    pub fn sidebar_visible(&self) -> bool {
        self.actions.get("sidebar").is_some_and(|a| a.checked)
    }

    /// Fold the trays away or bring them back; a folded sidebar opens
    /// unfolded at the next start (the fold is not remembered).
    pub fn set_sidebar_visible(&mut self, on: bool) {
        if let Some(a) = self.actions.get_mut("sidebar") {
            a.checked = on;
        }
    }

    /// Window ▸ Panels: one checkable entry per tray section, top to
    /// bottom, ticked when it is on screen, then Reset panel layout.
    pub fn panels_menu(&self) -> Vec<context::CtxItem> {
        let mut items: Vec<context::CtxItem> = Tray::section_order()
            .into_iter()
            .filter_map(|key| SECTIONS.iter().find(|(k, _)| *k == key))
            .map(|(key, title)| context::CtxItem::Entry {
                id: format!("tray_section:{key}"),
                text: tr(title, &[]),
                enabled: true,
                checked: Some(self.tray_section_visible(key)),
            })
            .collect();
        items.push(context::CtxItem::Separator);
        if let Some(a) = self.actions.get("reset_panel_layout") {
            items.push(context::CtxItem::Entry {
                id: a.id.clone(),
                text: a.text.clone(),
                enabled: true,
                checked: None,
            });
        }
        items
    }

    /// Every tray section back in its factory place and shown, and the
    /// tray shown in an unfolded sidebar. The folds stay as they are. Not
    /// undoable, like the toolbar layout.
    pub fn reset_panel_layout(&mut self) {
        Tray::reset_layout();
        self.set_tray_shown(PROPERTIES_TRAY, true);
    }

    /// Whether the tray section `key` is on screen: shown, in a shown
    /// tray.
    pub fn tray_section_visible(&self, key: &str) -> bool {
        self.tray_shown(PROPERTIES_TRAY) && Tray::section_shown(key)
    }

    /// Show or hide one tray section and remember it; showing one also
    /// shows its tray and unfolds a folded sidebar (`set_tray_shown`).
    pub fn set_tray_section_shown(&mut self, key: &str, shown: bool) {
        Tray::set_section_shown(key, shown);
        if shown {
            self.set_tray_shown(PROPERTIES_TRAY, true);
        }
    }

    // ---- keyboard and mouse ---------------------------------------------------------------
    /// A key press reaching the window: the viewport claims what it is
    /// typing (shortcut override), then an action's shortcut fires — none
    /// when two actions share it — else the viewport has it.
    pub fn key_press(&mut self, ev: &KeyEvent) {
        self.viewport.app_key_event(ev, true);
        if !self.viewport.shortcut_override(ev) {
            let owners: Vec<String> = self
                .actions
                .for_key(ev)
                .iter()
                .map(|a| a.id.clone())
                .collect();
            if owners.len() == 1 && !ev.auto_repeat {
                self.trigger(&owners[0]);
                return;
            }
            if owners.len() > 1 {
                // «Ambiguous shortcut overload»: neither fires.
                return;
            }
        }
        self.viewport.key_press(ev);
        self.pump_events();
    }

    pub fn key_release(&mut self, ev: &KeyEvent) {
        self.viewport.app_key_event(ev, false);
        self.viewport.key_release(ev);
        self.pump_events();
    }

    pub fn mouse_press(&mut self, ev: &MouseEvent) {
        self.viewport.app_mouse_press();
        self.viewport.mouse_press(ev);
        self.pump_events();
    }

    pub fn mouse_move(&mut self, ev: &MouseEvent) {
        self.viewport.mouse_move(ev);
        self.pump_events();
    }

    pub fn mouse_release(&mut self, ev: &MouseEvent) {
        self.viewport.mouse_release(ev);
        self.pump_events();
    }

    pub fn mouse_double_click(&mut self, ev: &MouseEvent) {
        self.viewport.mouse_double_click(ev);
        self.pump_events();
    }

    pub fn wheel(&mut self, x: f64, y: f64, delta: f64) {
        self.viewport.wheel(x, y, delta);
        self.pump_events();
    }

    /// The start of a frame, before its input: fire the timers that came
    /// due by `now` (the snap dwell, the note beside the pointer).
    pub fn tick(&mut self, now: Instant) {
        self.viewport.tick(now);
        self.pump_events();
    }

    /// How long from `now` until `tick` has something to fire, `None`
    /// when nothing waits: the shell asks for a frame then.
    pub fn next_tick(&self, now: Instant) -> Option<Duration> {
        self.viewport.next_tick_in(now)
    }

    /// Whether a right click asked for the context menu since the last
    /// call: the viewport decides what is a click (not a camera drag).
    pub fn take_context_menu_request(&mut self) -> bool {
        std::mem::take(&mut self.context_menu_asked)
    }

    /// Act on what the viewport asked for since the last pump.
    pub fn pump_events(&mut self) {
        for ev in self.viewport.take_events() {
            self.seen_events.push(ev.clone());
            match ev {
                ViewportEvent::ValueBufferChanged(t) => self.value_box_buffer = t,
                ViewportEvent::MeasurementChanged(t) => self.value_box_live = t,
                ViewportEvent::Flash(text, msec) => {
                    self.status_message = Some(StatusMessage { text, msec })
                }
                ViewportEvent::ActivateTool(k) => self.activate_tool(&k),
                ViewportEvent::PaintSampled => self.release_eyedropper(),
                ViewportEvent::PromptSectionName(id) => self.name_section_plane(id),
                ViewportEvent::SceneVersionChanged(_) => {
                    self.sync_style_menu();
                    self.update_title();
                }
                ViewportEvent::ContextMenu => self.context_menu_asked = true,
                ViewportEvent::ClipboardChanged => {
                    // also on the system clipboard, so another Xtrude CAD
                    // window can paste it; encoded only if asked
                    if let Some(c) = self.viewport.clipboard.clone() {
                        clipboard::publish(self.system_clipboard.as_mut(), c);
                    }
                }
            }
        }
        // a tool may have switched itself (Paste → Select)
        let key = self
            .viewport
            .active_tool()
            .map(|t| t.info().key.to_string());
        if key.is_some() && key.as_deref() != self.active_key.as_deref() {
            if let Some(k) = key.filter(|k| tool_keys().contains(&k.as_str())) {
                self.active_key = Some(k.clone());
                self.check_tool_action(Some(&k));
            }
        }
        self.update_title();
    }

    // ---- status bar ------------------------------------------------------------------------
    pub fn show_message(&mut self, text: &str, msec: u32) {
        self.status_message = Some(StatusMessage {
            text: text.to_string(),
            msec,
        });
    }

    /// The shown toolbars docked in `area`, line by line:
    /// `(id, title, action ids)`.
    pub fn toolbars_in(&self, area: ToolbarArea) -> Vec<ToolbarDef> {
        self.toolbar_lines(area).into_iter().flatten().collect()
    }

    /// The lines of `area`, outermost first, each with its shown toolbars
    /// in order (a line whose bars are all hidden comes back empty, so the
    /// positions stay the layout's line indices).
    pub fn toolbar_lines(&self, area: ToolbarArea) -> Vec<Vec<ToolbarDef>> {
        self.toolbar_layout.lines[area.index()]
            .iter()
            .map(|line| {
                line.iter()
                    .filter(|id| self.toolbar_layout.is_shown(id))
                    .filter_map(|id| self.toolbar(id))
                    .collect()
            })
            .collect()
    }

    /// The shown floating toolbars, each with its top-left.
    pub fn floating_toolbars(&self) -> Vec<(ToolbarDef, (f32, f32))> {
        self.toolbar_layout
            .floating
            .iter()
            .filter(|f| self.toolbar_layout.is_shown(&f.id))
            .filter_map(|f| self.toolbar(&f.id).map(|t| (t, f.pos)))
            .collect()
    }

    fn toolbar(&self, id: &str) -> Option<ToolbarDef> {
        self.toolbars.iter().find(|(t, _, _)| t == id).cloned()
    }

    /// The area `id` is docked in, `None` when it floats.
    pub fn toolbar_area(&self, id: &str) -> Option<ToolbarArea> {
        self.toolbar_layout.spot(id).map(|s| s.area)
    }

    /// Move a toolbar where it was dropped, and remember it.
    pub fn place_toolbar(&mut self, id: &str, target: toolbar_layout::DropTarget) {
        self.toolbar_layout.place(id, target);
        self.toolbar_layout.save();
    }

    /// Window ▸ Toolbars: one checkable entry per toolbar in creation
    /// order, ticked when shown, then Reset toolbar layout.
    pub fn toolbars_menu(&self) -> Vec<context::CtxItem> {
        let mut items: Vec<context::CtxItem> = self
            .toolbars
            .iter()
            .map(|(id, title, _)| context::CtxItem::Entry {
                id: format!("toolbar:{id}"),
                text: title.clone(),
                enabled: true,
                checked: Some(self.toolbar_layout.is_shown(id)),
            })
            .collect();
        items.push(context::CtxItem::Separator);
        if let Some(a) = self.actions.get("reset_toolbar_layout") {
            items.push(context::CtxItem::Entry {
                id: a.id.clone(),
                text: a.text.clone(),
                enabled: true,
                checked: None,
            });
        }
        items
    }

    /// Every toolbar back in its factory place and shown, saved at once.
    /// The trays keep their state: only the toolbars are reset. Not
    /// undoable, like any other toolbar change.
    pub fn reset_toolbar_layout(&mut self) {
        let known: Vec<String> = self.toolbars.iter().map(|(id, _, _)| id.clone()).collect();
        self.toolbar_layout = ToolbarLayout::factory_for(&known);
        self.toolbar_layout.save();
    }

    /// Untick or tick a toolbar in Window ▸ Toolbars, and remember it.
    pub fn toggle_toolbar(&mut self, id: &str) {
        let shown = self.toolbar_layout.is_shown(id);
        self.toolbar_layout.set_shown(id, !shown);
        self.toolbar_layout.save();
    }

    /// The status bar's base message: the tool's name and its hint.
    pub fn status_hint(&self) -> String {
        let vp = &self.viewport;
        let nav = vp.nav.mode;
        let tool = vp.active_tool();
        let key = tool.map(|t| t.info().key);
        let text = hints::hint_for(
            key,
            tool,
            nav.map(|n| (n, vp.prefs.nav_style)),
            Some(vp.inference.linear_inference_mode),
        );
        let name = match (nav, tool) {
            (Some(n), _) => tr(
                match n {
                    NavMode::Orbit => "Orbit",
                    NavMode::Pan => "Pan",
                    NavMode::Zoom => "Zoom",
                    NavMode::ZoomWindow => "Zoom window",
                },
                &[],
            ),
            (None, Some(t)) => tr(t.info().name, &[]),
            _ => String::new(),
        };
        match (name.is_empty(), text.is_empty()) {
            (false, false) => format!("{name} — {text}"),
            (false, true) => name,
            _ => text,
        }
    }

    /// The value box: `(caption, text, typing)`, `None` when the
    /// tool takes no value.
    pub fn value_box(&self) -> Option<(String, String, bool)> {
        let tool = self.viewport.active_tool()?;
        let caption = tool.value_box_caption()?;
        let typing = !self.value_box_buffer.is_empty();
        let text = if typing {
            self.value_box_buffer.clone()
        } else {
            self.value_box_live.clone()
        };
        Some((tr(&caption, &[]), text, typing))
    }

    // ---- title and dirt -------------------------------------------------------------------
    pub fn is_dirty(&self) -> bool {
        self.viewport.scene.content_version() as i64 != self.saved_content
    }

    /// Record the document as saved now.
    pub fn mark_saved(&mut self) {
        self.saved_content = self.viewport.scene.content_version() as i64;
        self.update_title();
    }

    /// Record a version as the saved one; -1 = never saved.
    pub fn set_saved_version(&mut self, version: i64) {
        let view = if version >= 0 {
            self.viewport.scene.view_version as i64
        } else {
            0
        };
        self.saved_content = version - view;
        self.update_title();
    }

    pub fn update_title(&mut self) {
        let name = if let Some(p) = &self.current_path {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else if let Some(n) = &self.import_name {
            n.clone()
        } else {
            tr("Untitled", &[])
        };
        let marker = if self.is_dirty() { " *" } else { "" };
        self.title = format!("Xtrude CAD — {name}{marker}");
    }

    // ---- menus that mirror state -------------------------------------------------------------
    pub fn sync_style_menu(&mut self) {
        let st = self.viewport.scene.display_style.clone();
        for s in builtin_styles() {
            if let Some(a) = self.actions.get_mut(&format!("style:{}", s.name)) {
                a.checked = s.name == st.name;
            }
        }
        for (id, v) in [
            ("style_edges", st.edges),
            ("style_silhouettes", st.profiles),
            ("style_hidden_lines", st.back_edges),
            ("section_fill", st.section_fill),
        ] {
            if let Some(a) = self.actions.get_mut(id) {
                a.checked = v;
            }
        }
        self.sync_section_menu();
        self.sync_rest_menu();
    }

    pub fn sync_section_menu(&mut self) {
        let s = &self.viewport.scene;
        let vals = [
            ("show_section_planes", s.show_section_planes),
            ("show_section_cuts", s.show_section_cuts),
            ("show_hidden_groups", s.show_hidden_objects),
            ("show_hidden_edges_faces", s.show_hidden_geometry),
        ];
        for (id, v) in vals {
            if let Some(a) = self.actions.get_mut(id) {
                a.checked = v;
            }
        }
    }

    fn sync_rest_menu(&mut self) {
        let mode = self.viewport.edit_rest_mode();
        for m in RestMode::ALL {
            if let Some(a) = self.actions.get_mut(&format!("rest:{}", m.key())) {
                a.checked = m == mode;
            }
        }
    }
}
