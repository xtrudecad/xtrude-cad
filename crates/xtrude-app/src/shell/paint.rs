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
        let align = if t.centered {
            egui::Align2::CENTER_CENTER
        } else {
            egui::Align2::LEFT_BOTTOM
        };
        painter.text(p, align, &t.text, font.clone(), c);
        if t.bold {
            painter.text(p + egui::vec2(0.4 * k, 0.0), align, &t.text, font, c);
        }
    }
}
