// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport overlay's paint commands on egui's painter, and the
//! toolbar icons.

use crate::viewport::overlay::{Paint, Pen, Pt};

fn color(c: [f32; 4]) -> egui::Color32 {
    egui::Rgba::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]).into()
}

fn pos(origin: egui::Pos2, p: Pt) -> egui::Pos2 {
    egui::pos2(origin.x + p.0 as f32, origin.y + p.1 as f32)
}

fn stroke(p: &Pen) -> egui::Stroke {
    egui::Stroke::new(p.width, color(p.color))
}

/// A line with the pen's dash pattern.
fn line(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, pen: &Pen) {
    match pen.dash {
        None => {
            painter.line_segment([a, b], stroke(pen));
        }
        Some((on, off)) => {
            painter.extend(egui::Shape::dashed_line(&[a, b], stroke(pen), on, off));
        }
    }
}

fn outline(painter: &egui::Painter, pts: &[egui::Pos2], pen: &Pen) {
    for i in 0..pts.len() {
        line(painter, pts[i], pts[(i + 1) % pts.len()], pen);
    }
}

/// Paint `items` with their pixels relative to `origin`.
pub fn overlay(painter: &egui::Painter, origin: egui::Pos2, items: &[Paint]) {
    for item in items {
        match item {
            Paint::Line { a, b, pen } => line(painter, pos(origin, *a), pos(origin, *b), pen),
            Paint::Rect {
                min,
                max,
                pen,
                fill,
            } => {
                let r = egui::Rect::from_min_max(pos(origin, *min), pos(origin, *max));
                if let Some(f) = fill {
                    painter.rect_filled(r, 0.0, color(*f));
                }
                if let Some(p) = pen {
                    outline(
                        painter,
                        &[
                            r.left_top(),
                            r.right_top(),
                            r.right_bottom(),
                            r.left_bottom(),
                        ],
                        p,
                    );
                }
            }
            Paint::Ellipse {
                center,
                r,
                pen,
                fill,
            } => {
                let c = pos(origin, *center);
                if let Some(f) = fill {
                    painter.circle_filled(c, *r as f32, color(*f));
                }
                if let Some(p) = pen {
                    painter.circle_stroke(c, *r as f32, stroke(p));
                }
            }
            Paint::Polygon { points, pen, fill } => {
                let pts: Vec<egui::Pos2> = points.iter().map(|p| pos(origin, *p)).collect();
                if let Some(f) = fill {
                    painter.add(egui::Shape::convex_polygon(
                        pts.clone(),
                        color(*f),
                        egui::Stroke::NONE,
                    ));
                }
                if let Some(p) = pen {
                    outline(painter, &pts, p);
                }
            }
            Paint::Text {
                at,
                text,
                color: c,
                size,
                bold,
                italic,
            } => {
                // typographic points → egui points (96 / 72), baseline
                // at `at`.
                let px = size * 96.0 / 72.0;
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(px),
                        color: color(*c),
                        italics: *italic,
                        ..Default::default()
                    },
                );
                let galley = painter.layout_job(job);
                let mut p = pos(origin, *at);
                p.y -= px * 0.8;
                if *bold {
                    painter.galley(p + egui::vec2(0.5, 0.0), galley.clone(), color(*c));
                }
                painter.galley(p, galley, color(*c));
            }
        }
    }
}

/// An icon's text (drawn over its texture; 48-space mapped onto `rect`).
pub fn icon_texts(painter: &egui::Painter, rect: egui::Rect, texts: &[super::icons::IconText]) {
    let k = rect.width() / super::icons::PX;
    for t in texts {
        let c = egui::Color32::from_rgba_unmultiplied(t.color.0, t.color.1, t.color.2, t.color.3);
        let font = egui::FontId::proportional(t.px * k);
        let p = egui::pos2(rect.min.x + t.at.0 * k, rect.min.y + t.at.1 * k);
        let galley = painter.layout_no_wrap(t.text.clone(), font, c);
        let origin = if t.centered {
            p - galley.size() / 2.0
        } else {
            // the baseline on `p`, not the bottom of the line (which
            // leaves room for descenders and lifts the text)
            p - egui::vec2(0.0, baseline(&galley))
        };
        painter.galley(origin, galley.clone(), c);
        if t.bold {
            painter.galley(origin + egui::vec2(0.4 * k, 0.0), galley, c);
        }
    }
}

/// How far below its top a galley's first line has its baseline.
fn baseline(galley: &egui::Galley) -> f32 {
    galley
        .rows
        .first()
        .and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y))
        .unwrap_or(galley.size().y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text placed by its baseline has the baseline there: above the
    /// bottom of its line box, by the font's descent.
    #[test]
    fn test_the_baseline_sits_inside_the_line_box() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.painter().layout_no_wrap(
                "A".into(),
                egui::FontId::proportional(30.0),
                egui::Color32::WHITE,
            );
            let b = baseline(&galley);
            let h = galley.size().y;
            assert!(b > 0.5 * h && b < h, "baseline {b} in a line {h} high");
        });
        out.textures_delta.clear(); // no GPU to take the font atlas
    }
}
