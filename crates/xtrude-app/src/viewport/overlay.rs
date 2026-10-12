// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The 2D overlay over the 3D frame: the screen-space rubber band, the guide
//! preview, the snap marker and its ScreenTip, the inference marker, guide
//! points, the length label, the lock labels and the selection and zoom boxes
//! — as paint commands the shell executes.

use crate::input::ScreenPt;
use crate::tools::pushpull::InferenceKind;
use glam::DVec3;
use xtrude_core::snap::{
    SnapContext, SnapKind, SnapResult, COLOR_ENDPOINT, COLOR_ON_EDGE, COLOR_ON_FACE,
};
use xtrude_core::Axis;
use xtrude_model::entity::EntityRef;
use xtrude_model::i18n::tr;
use xtrude_model::units::typed_value_text;

use super::events::BOX_DRAG_THRESHOLD_PX;
use super::Viewport;

pub type Rgba = [f32; 4];
pub type Pt = (f64, f64);

/// A stroke: colour, width (px), dash pattern (on, off) px or solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub color: Rgba,
    pub width: f32,
    pub dash: Option<(f32, f32)>,
}

impl Pen {
    pub fn solid(color: Rgba, width: f32) -> Pen {
        Pen {
            color,
            width,
            dash: None,
        }
    }

    pub fn dashed(color: Rgba, width: f32, on: f32, off: f32) -> Pen {
        Pen {
            color,
            width,
            dash: Some((on, off)),
        }
    }
}

/// One thing to paint, in viewport pixels (logical, top-left origin).
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Line {
        a: Pt,
        b: Pt,
        pen: Pen,
    },
    Rect {
        min: Pt,
        max: Pt,
        pen: Option<Pen>,
        fill: Option<Rgba>,
    },
    Ellipse {
        center: Pt,
        r: f64,
        pen: Option<Pen>,
        fill: Option<Rgba>,
    },
    Polygon {
        points: Vec<Pt>,
        pen: Option<Pen>,
        fill: Option<Rgba>,
    },
    /// Text whose baseline starts at `at`.
    Text {
        at: Pt,
        text: String,
        color: Rgba,
        size: f32,
        bold: bool,
        italic: bool,
    },
}

/// An 8-bit RGBA colour as paint colour.
pub fn qc(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}

fn rgbf(c: [f64; 3], a: f32) -> Rgba {
    [c[0] as f32, c[1] as f32, c[2] as f32, a]
}

/// The ScreenTip of each snap kind.
pub fn snap_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "endpoint" => "Endpoint",
        "midpoint" => "Midpoint",
        "arc_midpoint" => "Arc midpoint",
        "component_origin" => "Component origin",
        "on_line" => "On line",
        "on_axis" => "On axis",
        "tangent" => "Tangent at vertex",
        "on_edge" => "On edge",
        "on_face" => "On face",
        "origin" => "Origin",
        "extension" => "Extension",
        "intersection" => "Intersection",
        "from_point" => "From point",
        "aligned" => "In line with point",
        "through_point" => "Through point",
        "perp_face" => "Perpendicular to face",
        "center" => "Center",
        "guide_point" => "Guide point",
        _ => return None,
    })
}

const WHITE_HALO: Rgba = [1.0, 1.0, 1.0, 230.0 / 255.0];

/// The tip text with its white shadow, 1 px down-right (`drawText` twice).
fn shadowed(
    out: &mut Vec<Paint>,
    at: Pt,
    text: &str,
    color: Rgba,
    size: f32,
    bold: bool,
    shadow: Rgba,
) {
    out.push(Paint::Text {
        at: (at.0 + 1.0, at.1 + 1.0),
        text: text.to_string(),
        color: shadow,
        size,
        bold,
        italic: false,
    });
    out.push(Paint::Text {
        at,
        text: text.to_string(),
        color,
        size,
        bold,
        italic: false,
    });
}

impl Viewport {
    /// The overlay to paint over this frame.
    pub fn overlay(&mut self) -> Vec<Paint> {
        let mut out = Vec::new();
        self.overlay_rubber_band(&mut out);
        self.overlay_guide_preview(&mut out);
        self.overlay_center_hint(&mut out);
        if self.active_tool().is_some() {
            if let Some(s) = self.inference.last_snap.clone() {
                if s.kind != SnapKind::None {
                    self.overlay_snap_indicator(&mut out, &s);
                }
            }
        }
        self.overlay_inference_marker(&mut out);
        self.overlay_guides(&mut out);
        self.overlay_annotations(&mut out);
        self.overlay_length_label(&mut out);
        if self.inference.reference_mode.is_some() {
            self.overlay_reference_label(&mut out);
        } else if let Some(a) = self.inference.axis_lock {
            let (name, c) = axis_name_color(a);
            out.push(Paint::Text {
                at: (14.0, 24.0),
                text: tr("{axis} axis locked", &[("axis", &name)]),
                color: c,
                size: 11.0,
                bold: true,
                italic: false,
            });
        } else if let Some(a) = self
            .active_tool()
            .and_then(|t| t.work_plane())
            .and_then(|w| w.lock)
            .and_then(|l| l.plane_lock)
        {
            let (_, c) = axis_name_color(a);
            out.push(Paint::Text {
                at: (14.0, 24.0),
                text: tr(
                    "Drawing plane locked: {plane}",
                    &[("plane", &crate::tools::plane_lock_name(a))],
                ),
                color: c,
                size: 11.0,
                bold: true,
                italic: false,
            });
        } else {
            self.overlay_inference_label(&mut out);
        }
        self.overlay_linear_mode_label(&mut out);
        self.overlay_boxes(&mut out);
        out
    }

    fn overlay_rubber_band(&self, out: &mut Vec<Paint>) {
        let Some(t) = self.active_tool() else { return };
        if t.info().wireframe_depth_tested {
            return; // drawn in the 3D pass
        }
        let segs = t.rubber_band_lines(self);
        if segs.is_empty() {
            return;
        }
        let color = t
            .wireframe_color()
            .map(|c| c.map(|x| x as f32))
            .unwrap_or_else(|| self.rubber_band_rgba());
        let width = if self.rubber_band_inference() {
            2.5
        } else {
            2.0
        };
        for (a, b) in segs {
            if let Some((p, q)) = self.segment_to_pixels(a, b) {
                out.push(Paint::Line {
                    a: p,
                    b: q,
                    pen: Pen::solid(color, width),
                });
            }
        }
    }

    fn overlay_guide_preview(&self, out: &mut Vec<Paint>) {
        let Some(t) = self.active_tool() else { return };
        for (a, b) in t.guide_preview_lines() {
            if let Some((p, q)) = self.segment_to_pixels(a, b) {
                out.push(Paint::Line {
                    a: p,
                    b: q,
                    pen: Pen::dashed(qc(70, 90, 120, 255), 1.5, 15.0, 10.5),
                });
            }
        }
    }

    fn overlay_center_hint(&mut self, out: &mut Vec<Paint>) {
        let uses_snap = self.active_tool().is_some_and(|t| t.snap_prefs().uses_snap);
        let on_it = self
            .inference
            .last_snap
            .as_ref()
            .is_some_and(|s| s.kind == SnapKind::Center);
        if !uses_snap || on_it {
            return;
        }
        let Some(ScreenPt { x: mx, y: my }) = self.input.last_mouse_pos else {
            return;
        };
        let Some(r) = self.valid_center_ref() else {
            return;
        };
        let Some(pc) = self.world_to_pixel(r.center) else {
            return;
        };
        let hint = self.center_hint_px(&r, pc.tuple());
        if hint <= 0.0 || (pc.x - mx).hypot(pc.y - my) > hint {
            return;
        }
        let c = rgbf(COLOR_ENDPOINT, 1.0);
        out.push(Paint::Ellipse {
            center: pc.tuple(),
            r: 4.0,
            pen: Some(Pen::solid(WHITE_HALO, 3.0)),
            fill: Some(c),
        });
        out.push(Paint::Ellipse {
            center: pc.tuple(),
            r: 4.0,
            pen: Some(Pen::solid(c, 1.5)),
            fill: None,
        });
    }

    fn overlay_snap_indicator(&self, out: &mut Vec<Paint>, snap: &SnapResult) {
        if matches!(snap.kind, SnapKind::AxisInference | SnapKind::Axis) {
            return;
        }
        let Some(ScreenPt { x: px, y: py }) = self.world_to_pixel(snap.point) else {
            return;
        };
        let color = rgbf(snap.color, 1.0);
        let guide = |a: DVec3, b: DVec3, c: [f64; 3], out: &mut Vec<Paint>| {
            if let (Some(p), Some(q)) = (self.world_to_pixel(a), self.world_to_pixel(b)) {
                if let Some((p, q)) = self.clip_pixel_line(p.tuple(), q.tuple()) {
                    out.push(Paint::Line {
                        a: p,
                        b: q,
                        pen: Pen::dashed(rgbf(c, 0.9), 2.0, 8.0, 4.0),
                    });
                }
            }
        };
        if let Some((a, b)) = snap.guide {
            guide(a, b, snap.guide_color.unwrap_or(snap.color), out);
        }
        for (a, b, c) in snap.guides.clone().unwrap_or_default() {
            guide(a, b, c, out);
        }
        let halo = Pen::solid(WHITE_HALO, 4.5);
        let mark = Pen::solid(color, 2.6);
        let fill = rgbf(snap.color, 0.30);
        match snap.kind.as_str() {
            "intersection" => {
                for pen in [halo, mark] {
                    out.push(Paint::Line {
                        a: (px - 8.0, py - 8.0),
                        b: (px + 8.0, py + 8.0),
                        pen,
                    });
                    out.push(Paint::Line {
                        a: (px - 8.0, py + 8.0),
                        b: (px + 8.0, py - 8.0),
                        pen,
                    });
                }
            }
            "center" => {
                out.push(Paint::Ellipse {
                    center: (px, py),
                    r: 6.5,
                    pen: Some(halo),
                    fill: None,
                });
                out.push(Paint::Ellipse {
                    center: (px, py),
                    r: 6.5,
                    pen: Some(mark),
                    fill: Some(rgbf(snap.color, 0.85)),
                });
            }
            "endpoint" | "origin" | "component_origin" | "on_edge" | "on_axis" | "on_line"
            | "extension" | "from_point" | "aligned" | "tangent" => {
                let (min, max) = ((px - 7.0, py - 7.0), (px + 7.0, py + 7.0));
                out.push(Paint::Rect {
                    min,
                    max,
                    pen: Some(halo),
                    fill: None,
                });
                out.push(Paint::Rect {
                    min,
                    max,
                    pen: Some(mark),
                    fill: Some(fill),
                });
            }
            "midpoint" | "arc_midpoint" => {
                let d = vec![
                    (px, py - 9.0),
                    (px + 9.0, py),
                    (px, py + 9.0),
                    (px - 9.0, py),
                ];
                out.push(Paint::Polygon {
                    points: d.clone(),
                    pen: Some(halo),
                    fill: None,
                });
                out.push(Paint::Polygon {
                    points: d,
                    pen: Some(mark),
                    fill: Some(fill),
                });
            }
            k => {
                let r = match k {
                    "on_face" => 5.5,
                    "close" => 9.0,
                    "reference" | "through_point" | "perp_face" => 6.5,
                    _ => 0.0,
                };
                if r > 0.0 {
                    // the round markers keep the same 30 % fill as the others
                    out.push(Paint::Ellipse {
                        center: (px, py),
                        r,
                        pen: Some(halo),
                        fill: Some(fill),
                    });
                    out.push(Paint::Ellipse {
                        center: (px, py),
                        r,
                        pen: Some(mark),
                        fill: Some(fill),
                    });
                }
            }
        }
        let mut label = snap_label(snap.kind.as_str()).map(|l| {
            if snap.kind == SnapKind::Aligned && snap.axis == Some(Axis::Z) {
                tr("Level with point", &[])
            } else {
                tr(l, &[])
            }
        });
        if let Some(own) = &snap.label {
            label = Some(own.clone());
        }
        if let Some(mut text) = label {
            match snap.context {
                Some(SnapContext::Component) => {
                    text = format!("{text} {}", tr("in component", &[]))
                }
                Some(SnapContext::Group) => text = format!("{text} {}", tr("in group", &[])),
                None => {}
            }
            shadowed(
                out,
                (px + 10.0, py + 16.0),
                &text,
                color,
                9.0,
                false,
                qc(255, 255, 255, 220),
            );
        }
    }

    fn overlay_inference_marker(&self, out: &mut Vec<Paint>) {
        let Some((world, kind)) = self.active_tool().and_then(|t| t.inference_marker()) else {
            return;
        };
        let Some(ScreenPt { x: px, y: py }) = self.world_to_pixel(world) else {
            return;
        };
        let (rgb, label) = match kind {
            InferenceKind::Edge => (COLOR_ON_EDGE, "on_edge"),
            InferenceKind::Face => (COLOR_ON_FACE, "on_face"),
            InferenceKind::GuideLine => (COLOR_ON_EDGE, "on_line"),
            InferenceKind::GuidePoint => (COLOR_ENDPOINT, "guide_point"),
            InferenceKind::Vertex => (COLOR_ENDPOINT, "endpoint"),
        };
        let color = rgbf(rgb, 1.0);
        let (min, max) = ((px - 6.0, py - 6.0), (px + 6.0, py + 6.0));
        out.push(Paint::Rect {
            min,
            max,
            pen: Some(Pen::solid(WHITE_HALO, 4.0)),
            fill: None,
        });
        out.push(Paint::Rect {
            min,
            max,
            pen: Some(Pen::solid(color, 2.4)),
            fill: Some(rgbf(rgb, 0.30)),
        });
        if let Some(t) = snap_label(label) {
            shadowed(
                out,
                (px + 10.0, py + 16.0),
                &tr(t, &[]),
                color,
                9.0,
                false,
                qc(255, 255, 255, 220),
            );
        }
    }

    fn overlay_guides(&self, out: &mut Vec<Paint>) {
        for g in &self.scene.doc.guides {
            if g.direction.is_some() {
                continue;
            }
            let selected = self.scene.selection.contains(&EntityRef::Guide(g.id));
            let pen = if selected {
                Pen::dashed(qc(243, 115, 41, 255), 2.0, 28.0, 20.0)
            } else {
                Pen::dashed(qc(70, 90, 120, 255), 1.0, 14.0, 10.0)
            };
            if let Some(ScreenPt { x, y }) = self.world_to_pixel(g.point) {
                out.push(Paint::Line {
                    a: (x - 5.0, y),
                    b: (x + 5.0, y),
                    pen,
                });
                out.push(Paint::Line {
                    a: (x, y - 5.0),
                    b: (x, y + 5.0),
                    pen,
                });
            }
        }
    }

    fn overlay_length_label(&self, out: &mut Vec<Paint>) {
        let Some(t) = self.active_tool() else { return };
        let Some((mut text, Some(mid))) = t.value_label(self) else {
            return;
        };
        let Some(ScreenPt { x, y }) = self.world_to_pixel(mid) else {
            return;
        };
        let buffer = &self.value_box.buffer;
        let shadow = if buffer.is_empty() {
            qc(255, 255, 255, 220)
        } else {
            text = if t.value_spec().unitless {
                buffer.clone()
            } else {
                typed_value_text(buffer)
            };
            qc(255, 220, 130, 235)
        };
        shadowed(
            out,
            (x + 11.0, y - 8.0),
            &text,
            qc(0x0F, 0x14, 0x1B, 255),
            10.0,
            true,
            shadow,
        );
    }

    fn overlay_reference_label(&self, out: &mut Vec<Paint>) {
        let Some(mode) = self.inference.reference_mode else {
            return;
        };
        if self.inference.reference_edge.is_none() {
            return;
        }
        let word = if mode == xtrude_core::snap::RefMode::Parallel {
            "Parallel"
        } else {
            "Perpendicular"
        };
        out.push(Paint::Text {
            at: (14.0, 24.0),
            text: format!("{word} to reference edge"),
            color: rgbf([0.85, 0.30, 0.80], 1.0),
            size: 11.0,
            bold: true,
            italic: false,
        });
    }

    fn overlay_inference_label(&self, out: &mut Vec<Paint>) {
        let Some(s) = &self.inference.last_snap else {
            return;
        };
        if s.kind != SnapKind::AxisInference {
            return;
        }
        let name = match s.axis {
            Some(Axis::X) => "Red",
            Some(Axis::Y) => "Green",
            Some(Axis::Z) => "Blue",
            _ => "?",
        };
        out.push(Paint::Text {
            at: (14.0, 44.0),
            text: format!("On {name} Axis (hold Shift to lock)"),
            color: rgbf(s.color, 0.95),
            size: 10.0,
            bold: false,
            italic: true,
        });
    }

    fn overlay_linear_mode_label(&self, out: &mut Vec<Paint>) {
        use xtrude_core::snap::LinearMode;
        let text = match self.inference.linear_inference_mode {
            LinearMode::Off => tr("Linear inferences: OFF (Alt)", &[]),
            LinearMode::ParallelPerp => tr("Inferences: parallel / perpendicular only (Alt)", &[]),
            LinearMode::All => return,
        };
        out.push(Paint::Text {
            at: (14.0, self.height() - 16.0),
            text,
            color: qc(210, 150, 40, 255),
            size: 9.0,
            bold: false,
            italic: false,
        });
    }

    fn overlay_boxes(&self, out: &mut Vec<Paint>) {
        if let Some(super::RubberBand { start: s, cur: c }) = self.select_box {
            if (c.x - s.x).hypot(c.y - s.y) >= BOX_DRAG_THRESHOLD_PX {
                let crossing = c.x - s.x < 0.0;
                let (pen, fill) = if crossing {
                    (
                        Pen::dashed(qc(40, 158, 92, 255), 1.5, 6.0, 3.0),
                        qc(40, 158, 92, 28),
                    )
                } else {
                    (Pen::solid(qc(51, 102, 199, 255), 1.5), qc(51, 102, 199, 28))
                };
                out.push(Paint::Rect {
                    min: (s.x.min(c.x), s.y.min(c.y)),
                    max: (s.x.max(c.x), s.y.max(c.y)),
                    pen: Some(pen),
                    fill: Some(fill),
                });
            }
        }
        if let Some(super::RubberBand { start: s, cur: c }) = self.nav.zoom_box {
            out.push(Paint::Rect {
                min: (s.x.min(c.x), s.y.min(c.y)),
                max: (s.x.max(c.x), s.y.max(c.y)),
                pen: Some(Pen::dashed(qc(243, 115, 41, 255), 1.5, 6.0, 3.0)),
                fill: Some(qc(243, 115, 41, 26)),
            });
        }
    }

    /// A pixel segment cut to a margin around the view.
    pub fn clip_pixel_line(&self, p: Pt, q: Pt) -> Option<(Pt, Pt)> {
        let (w, h) = (self.width(), self.height());
        let m = 2000.0;
        let (x0, y0, x1, y1) = (-m, -m, w + m, h + m);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        for (pp, qq) in [
            (-dx, p.0 - x0),
            (dx, x1 - p.0),
            (-dy, p.1 - y0),
            (dy, y1 - p.1),
        ] {
            if pp == 0.0 {
                if qq < 0.0 {
                    return None;
                }
            } else {
                let r = qq / pp;
                if pp < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        (t0 <= t1).then_some((
            (p.0 + dx * t0, p.1 + dy * t0),
            (p.0 + dx * t1, p.1 + dy * t1),
        ))
    }

    fn rubber_band_rgba(&self) -> Rgba {
        let Some(s) = &self.inference.last_snap else {
            return [0.95, 0.45, 0.16, 0.85];
        };
        match s.kind {
            SnapKind::Axis | SnapKind::Reference | SnapKind::ThroughPoint | SnapKind::PerpFace => {
                rgbf(s.color, 1.0)
            }
            SnapKind::AxisInference => rgbf(s.color, 0.5),
            SnapKind::Close => [0.20, 0.40, 0.78, 0.95],
            _ => [0.95, 0.45, 0.16, 0.85],
        }
    }

    fn rubber_band_inference(&self) -> bool {
        self.inference.last_snap.as_ref().is_some_and(|s| {
            matches!(
                s.kind,
                SnapKind::Axis
                    | SnapKind::AxisInference
                    | SnapKind::Reference
                    | SnapKind::Extension
                    | SnapKind::ThroughPoint
                    | SnapKind::PerpFace
            )
        })
    }
}

fn axis_name_color(a: Axis) -> (String, Rgba) {
    match a {
        Axis::X => ("X".into(), qc(220, 56, 69, 255)),
        Axis::Y => ("Y".into(), qc(40, 158, 92, 255)),
        _ => ("Z".into(), qc(51, 102, 199, 255)),
    }
}
