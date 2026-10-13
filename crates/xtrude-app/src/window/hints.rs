// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! One status-bar hint at a time, for the tool and the step it is in — the
//! classic status bar, not a strip of every shortcut at once.

use xtrude_core::snap::LinearMode;
use xtrude_model::i18n::tr;

use crate::tools::Tool;
use crate::viewport::{NavMode, NavStyle};

/// A tool's hint per phase.
enum Hint {
    /// The same hint always.
    Always(&'static str),
    /// `(idle, started)`.
    Two(&'static str, &'static str),
    /// Named phases (`idle` is the fallback).
    Phases(&'static [(&'static str, &'static str)]),
}

/// Whether `key` has a hint of its own (the rest get «Name: Esc cancels.»).
pub fn has_hint(key: &str) -> bool {
    hint_entry(key).is_some()
}

/// The texts `key` shows past its idle step (translated), for the fit
/// check: each may carry the Alt clause.
pub fn started_texts(key: &str) -> Vec<String> {
    match hint_entry(key) {
        None => Vec::new(),
        Some(Hint::Always(s)) => vec![tr(s, &[])],
        Some(Hint::Two(_, s)) => vec![tr(s, &[])],
        Some(Hint::Phases(list)) => list
            .iter()
            .filter(|(p, _)| *p != "idle")
            .map(|(_, t)| tr(t, &[]))
            .collect(),
    }
}

fn hint_entry(key: &str) -> Option<Hint> {
    use Hint::*;
    Some(match key {
        "select" => Always("Shift = add/remove, Ctrl = add. Double-click a group to edit it."),
        "change_axes" => Two(
            "Click the new origin.",
            "Click the red direction, then the green one.",
        ),
        "merge_solids" | "solid_union" => Two(
            "Click a solid group or component (1).",
            "Click another solid (2) — and more to add them.",
        ),
        "solid_subtract" => Two(
            "Click the solid that cuts (1).",
            "Click the solid to cut (2). The first one goes away.",
        ),
        "solid_trim" => Two(
            "Click the solid that cuts (1).",
            "Click the solid to cut (2). The first one stays.",
        ),
        "solid_intersect" => Two(
            "Click a solid group or component (1).",
            "Click another solid (2): only what they share stays.",
        ),
        "solid_split" => Two(
            "Click a solid group or component (1).",
            "Click another solid (2): three pieces come out.",
        ),
        "eraser" => Always("Drag over edges to erase them. Shift = hide instead."),
        "paint" => Always("Click a face to paint it. Alt = sample the color under the cursor."),
        "line" => Two(
            "Click the start point. Arrows lock an axis, Shift locks the inference.",
            "Click the end point, or type the length and Enter.",
        ),
        "freehand" => Always("Press and drag to draw a freehand line."),
        "rectangle" => Two(
            "Click the first corner. Arrows pick the plane; Ctrl = centre.",
            "Click the opposite corner, or type width;height and Enter.",
        ),
        "rectangle_center" => Two(
            "Click the centre. Arrows pick the plane; Ctrl = from a corner.",
            "Click a corner, or type width;height and Enter.",
        ),
        "rotated_rect" => Two(
            "Click the first corner of the base edge.",
            "Edge end, then width (or type them). Shift holds the direction.",
        ),
        "circle" | "polygon" => Two(
            "Click the centre. Arrows pick the plane, Down = perpendicular to an edge. Type the sides first.",
            "Click the radius, or type it and Enter.",
        ),
        "slot" => Phases(&[
            ("idle", "Click the first centre. Arrows pick the plane. Type the sides first."),
            ("started", "Click the second centre, or type length;width and Enter."),
            ("width", "Click to set the width, or type it and Enter."),
        ]),
        "arc" => Phases(&[
            ("idle", "Click the start point — on an edge to draw a tangent arc."),
            ("end", "End point. Magenta = same distance; double-click rounds."),
            (
                "bulge",
                "Click the bulge, or type it. 'Nr' = radius, 'Ns' = segments. Alt keeps the corner.",
            ),
        ]),
        "arc3" => Two(
            "Click the start point.",
            "Click a point the arc passes through, then the end.",
        ),
        "center_arc" => Two(
            "Click the centre.",
            "Click the start of the arc, then its end; or type the angle.",
        ),
        "sector" => Two(
            "Click the centre.",
            "Click the start of the wedge, then its end; or type the angle.",
        ),
        "pushpull" => Two(
            "Click a face and move. Ctrl keeps the starting face.",
            "Move, or type the distance and Enter. Double-click repeats the last.",
        ),
        "move" => Two(
            "Click what to move (or select it first). Red + on a group = rotate it.",
            "Click the destination, or type the distance and Enter.",
        ),
        "rotate" => Two(
            "Click the centre of rotation — on a face, the protractor takes its plane.",
            "Click the start of the angle, then the end; or type the degrees.",
        ),
        "scale" => Two(
            "Select something, then drag a grip: corners uniform, edges along an axis.",
            "Drag, or type a factor (2) or a size (2m). Ctrl = centre, Shift = uniform.",
        ),
        "pattern_linear" => Two(
            "Click what to repeat (or select it first), at the start of the direction.",
            "Click the spacing, or type count;spacing or count/span and Enter.",
        ),
        "pattern_circular" => Two(
            "Click the centre (or select first); on a face, the protractor takes its plane.",
            "Type count;angle (count alone: full turn), or click the angle.",
        ),
        "flip" => Always("Click the plane to mirror the selection across."),
        "sweep" => Two(
            "Select the path, then click the profile face. Alt = sweep along the selection.",
            "Move along the path; click to finish.",
        ),
        "offset" => Two(
            "Click a face, or connected edges, to offset.",
            "Move to set the offset, or type it and Enter.",
        ),
        "fillet" => Phases(&[
            (
                "idle",
                "Click an edge to round it (or select edges first); type the radius and Enter.",
            ),
            (
                "sizing",
                "Move to set the radius, or type it and Enter; click to round. 'Ns' = segments.",
            ),
        ]),
        "chamfer" => Phases(&[
            (
                "idle",
                "Click an edge to chamfer it (or select edges first); type the distance and Enter.",
            ),
            (
                "sizing",
                "Move to set the distance along the faces, or type it and Enter; click to chamfer.",
            ),
        ]),
        "hole" => Always("Click a face to drill. Type M3, M3c, Ø or Ø;depth; Tab = head."),
        "shell" => Phases(&[
            ("idle", "Click a solid to hollow it (or select its open faces first)."),
            (
                "faces",
                "Click the faces to leave open; Enter, or type the wall thickness and Enter.",
            ),
        ]),
        "measure" => Two(
            "Click a point or an edge to measure. Arrows lock an axis.",
            "Click the second point, or type the distance and Enter.",
        ),
        "protractor" => Two(
            "Click the vertex of the angle.",
            "Click the start of the angle, then the end; or type the degrees.",
        ),
        "dimension" => Two(
            "Click the first point of the dimension.",
            "Click the 2nd point, then place it; past an end = linear.",
        ),
        "text" => Two(
            "Click a point for a label, or empty space for a screen note.",
            "Click where the label goes; the text is asked for next.",
        ),
        "section" => Always("Click a face to place the section plane. Shift keeps the orientation."),
        "paste" => Always("Click where the copy goes. Arrows lock an axis."),
        "place_group" => Always("Click where the component goes."),
        _ => return None,
    })
}

/// A camera mode's hint; Orbit's names how `style` orbits from any tool.
pub fn nav_hint(mode: NavMode, style: NavStyle) -> String {
    match mode {
        NavMode::Orbit => format!(
            "{} {}",
            tr("Drag to orbit. Shift = pan. Wheel = zoom.", &[]),
            style.orbit_reminder()
        ),
        NavMode::Pan => tr("Drag to pan. Wheel = zoom.", &[]),
        NavMode::Zoom => tr(
            "Drag up to zoom in, down to zoom out. Wheel zooms at the cursor.",
            &[],
        ),
        NavMode::ZoomWindow => tr("Drag a box to zoom into it.", &[]),
    }
}

/// The «Alt = …» clause for a linear-inference mode.
pub fn alt_inference_hint(mode: LinearMode) -> String {
    let label = match mode {
        LinearMode::All => "all",
        LinearMode::Off => "none",
        LinearMode::ParallelPerp => "parallel / perpendicular",
    };
    tr("Alt = inferences ({state})", &[("state", &tr(label, &[]))])
}

/// The one line the status bar shows now. With `linear_mode` and a tool
/// mid-operation, the Alt offer is appended, the classic way.
pub fn hint_for(
    key: Option<&str>,
    tool: Option<&dyn Tool>,
    nav: Option<(NavMode, NavStyle)>,
    linear_mode: Option<LinearMode>,
) -> String {
    if let Some((n, style)) = nav {
        return nav_hint(n, style);
    }
    let Some(tool) = tool else {
        return String::new();
    };
    let key = key.unwrap_or("");
    let Some(entry) = hint_entry(key) else {
        let name = tool.info().name;
        return tr("{name}: Esc cancels.", &[("name", &tr(name, &[]))]);
    };
    let phase = tool.phase();
    let text = match entry {
        Hint::Always(s) => tr(s, &[]),
        Hint::Two(idle, started) => tr(if phase != "idle" { started } else { idle }, &[]),
        Hint::Phases(list) => {
            let pick = list
                .iter()
                .find(|(p, _)| *p == phase)
                .or_else(|| list.iter().find(|(p, _)| *p == "idle"))
                .map_or("", |(_, t)| t);
            tr(pick, &[])
        }
    };
    with_alt(text, tool, linear_mode)
}

/// The tool's own clause before the first click; the Alt one mid-operation,
/// and only for a tool that snaps.
fn with_alt(text: String, tool: &dyn Tool, linear_mode: Option<LinearMode>) -> String {
    if tool.phase() == "idle" {
        let own = tool.status_clause();
        return if own.is_empty() {
            text
        } else {
            format!("{text} | {own}")
        };
    }
    let Some(mode) = linear_mode else {
        return text;
    };
    if !tool.snap_prefs().uses_snap {
        return text;
    }
    format!("{text} | {}", alt_inference_hint(mode))
}
