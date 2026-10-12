// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The document annotations and tool grips of the overlay: the scale box,
//! section planes, dimensions, text labels, occluded segments, and the
//! active tool's own paint ([`crate::tools::Tool::overlay`]).

use crate::input::ScreenPt;
use glam::DVec3;
use xtrude_model::entity::EntityRef;

use super::overlay::{qc, Paint, Pen, Pt, Rgba};
use super::{format_dim_value, Viewport};
use crate::fontmetrics::FontMetrics;
use crate::tools::OverlayLayer;

const SEL_INK: [u8; 3] = [243, 115, 41];

/// White-haloed text (offset 1 px down-right).
fn halo_text(out: &mut Vec<Paint>, at: Pt, text: &str, ink: Rgba, size: f32) {
    out.push(Paint::Text {
        at: (at.0 + 1.0, at.1 + 1.0),
        text: text.to_string(),
        color: qc(255, 255, 255, 230),
        size,
        bold: true,
        italic: false,
    });
    out.push(Paint::Text {
        at,
        text: text.to_string(),
        color: ink,
        size,
        bold: true,
        italic: false,
    });
}

impl Viewport {
    /// The tool-owned grips and planes, then the annotations, in this
    /// order.
    pub(super) fn overlay_annotations(&mut self, out: &mut Vec<Paint>) {
        self.overlay_tool(OverlayLayer::UnderSectionPlanes, out);
        self.overlay_section_planes(out);
        self.overlay_tool(OverlayLayer::OverSectionPlanes, out);
        self.overlay_dimensions(out);
        self.overlay_text_labels(out);
    }

    fn overlay_section_planes(&mut self, out: &mut Vec<Paint>) {
        if self.scene.doc.section_planes.is_empty() || !self.scene.show_section_planes {
            return;
        }
        let planes: Vec<_> = self
            .scene
            .doc
            .section_planes
            .iter()
            .map(|p| {
                (
                    p.id,
                    p.point,
                    p.normal,
                    p.active,
                    p.symbol.trim().to_string(),
                )
            })
            .collect();
        for (id, point, normal, active, symbol) in planes {
            let corners = self.section_plane_frame(point, normal);
            let mut px = Vec::with_capacity(4);
            for i in 0..4 {
                let seg = self
                    .clip_segment_front(corners[i], corners[(i + 1) % 4])
                    .and_then(|(a, b)| Some((self.world_to_pixel(a)?, self.world_to_pixel(b)?)));
                match seg {
                    Some(s) => px.push(s),
                    None => break,
                }
            }
            if px.len() < 4 {
                continue;
            }
            let selected = self.scene.selection.contains(&EntityRef::SectionPlane(id));
            let (col, mut width) = if active {
                (qc(243, 115, 41, 255), 2.0)
            } else {
                (qc(178, 182, 188, 255), 1.4)
            };
            if selected {
                width += 1.2;
            }
            let frame = if selected {
                Pen::solid(col, width)
            } else {
                Pen::dashed(col, width, 4.0 * width, 2.0 * width)
            };
            for (a, b) in &px {
                out.push(Paint::Line {
                    a: a.tuple(),
                    b: b.tuple(),
                    pen: frame,
                });
            }
            let solid = Pen::solid(col, width);
            let screen: Vec<Option<Pt>> = corners
                .iter()
                .map(|c| self.world_to_pixel(*c).map(ScreenPt::tuple))
                .collect();
            for i in 0..4 {
                let (Some(c), Some(n1), Some(n2)) =
                    (screen[i], screen[(i + 1) % 4], screen[(i + 3) % 4])
                else {
                    continue;
                };
                for n in [n1, n2] {
                    let (dx, dy) = (n.0 - c.0, n.1 - c.1);
                    let ln = match dx.hypot(dy) {
                        0.0 => 1.0,
                        l => l,
                    };
                    let k = (14.0f64).min(ln * 0.18) / ln;
                    out.push(Paint::Line {
                        a: c,
                        b: (c.0 + dx * k, c.1 + dy * k),
                        pen: solid,
                    });
                }
            }
            if !symbol.is_empty() {
                let text: String = symbol.chars().take(3).collect();
                let fm = FontMetrics::new(8.5, true);
                let w = fm.horizontal_advance(&text);
                let r = 9.0;
                for c in screen.iter().flatten() {
                    out.push(Paint::Ellipse {
                        center: *c,
                        r,
                        pen: Some(solid),
                        fill: Some(qc(255, 255, 255, 220)),
                    });
                    out.push(Paint::Text {
                        at: (c.0 - w / 2.0, c.1 + (fm.ascent() - fm.descent()) / 2.0),
                        text: text.clone(),
                        color: col,
                        size: 8.5,
                        bold: true,
                        italic: false,
                    });
                }
            }
        }
    }

    /// The active tool's own paint at `layer`.
    fn overlay_tool(&self, layer: OverlayLayer, out: &mut Vec<Paint>) {
        if let Some(tool) = self.active_tool() {
            tool.overlay(self, layer, out);
        }
    }

    /// The 3D segment `a`–`b` in screen space, skipping the runs hidden
    /// behind solid geometry (16 samples, both ends of a run visible).
    fn occluded_segment(&mut self, out: &mut Vec<Paint>, a: DVec3, b: DVec3, pen: Pen) {
        const SAMPLES: usize = 16;
        let mut prev: Option<Pt> = None;
        let mut prev_vis = false;
        for i in 0..=SAMPLES {
            let w = a + (b - a) * (i as f64 / SAMPLES as f64);
            let px = self.world_to_pixel(w);
            let vis = px.is_some() && !self.occluded_on_screen(w);
            if let (Some(p), Some(q)) = (prev, px) {
                if prev_vis && vis {
                    out.push(Paint::Line {
                        a: p,
                        b: q.tuple(),
                        pen,
                    });
                }
            }
            prev = px.map(|p| p.tuple());
            prev_vis = vis;
        }
    }

    fn style_ink(&self) -> [u8; 3] {
        self.scene.dimension_style.color
    }

    fn style_font_size(&self) -> f64 {
        self.scene.dimension_style.font_size as f64
    }

    fn ink_for(&self, e: EntityRef, default: [u8; 3]) -> Rgba {
        let [r, g, b] = if self.scene.selection.contains(&e) || self.hover_entity == Some(e) {
            SEL_INK
        } else {
            default
        };
        qc(r, g, b, 255)
    }

    fn overlay_dimensions(&mut self, out: &mut Vec<Paint>) {
        if self.scene.dimensions.is_empty() {
            return;
        }
        let default_ink = self.style_ink();
        let size = self.style_font_size() as f32;
        let ends = self.scene.dimension_style.ends.clone();
        let style = self.scene.dimension_style.clone();
        let dims: Vec<_> = {
            let doc = self.scene.doc_view();
            self.scene
                .dimensions
                .iter()
                .filter(|d| self.scene.entity_visible(&EntityRef::Dimension(d.id)))
                .map(|d| {
                    let (ap, bp) = d.line_points(&doc);
                    let text = d.display_text(&format_dim_value(d.value(&doc), &style));
                    (d.id, d.a(&doc), d.b(&doc), ap, bp, d.midpoint(&doc), text)
                })
                .collect()
        };
        for (id, a, b, ap, bp, mid_world, text) in dims {
            let ink = self.ink_for(EntityRef::Dimension(id), default_ink);
            let (Some(pap), Some(pbp)) = (self.world_to_pixel(ap), self.world_to_pixel(bp)) else {
                continue;
            };
            self.occluded_segment(out, a, ap, Pen::solid(ink, 1.0));
            self.occluded_segment(out, b, bp, Pen::solid(ink, 1.0));
            let line = Pen::solid(ink, 1.5);
            self.occluded_segment(out, ap, bp, line);
            let (dx, dy) = (pbp.x - pap.x, pbp.y - pap.y);
            let ln = dx.hypot(dy);
            if ln > 1e-6 && ends != "none" {
                let (ux, uy) = (dx / ln, dy / ln);
                for (ScreenPt { x: cx, y: cy }, w, sign) in [(pap, ap, 1.0), (pbp, bp, -1.0)] {
                    if self.occluded_on_screen(w) {
                        continue;
                    }
                    if ends == "tick" {
                        let (ox, oy) = (-uy * 4.0, ux * 4.0);
                        out.push(Paint::Line {
                            a: (cx - ox, cy - oy),
                            b: (cx + ox, cy + oy),
                            pen: line,
                        });
                    } else {
                        let (l, half) = (10.0, 2.2);
                        let (bx, by) = (cx + sign * ux * l, cy + sign * uy * l);
                        out.push(Paint::Polygon {
                            points: vec![
                                (cx, cy),
                                (bx - uy * half, by + ux * half),
                                (bx + uy * half, by - ux * half),
                            ],
                            pen: None,
                            fill: Some(ink),
                        });
                    }
                }
            }
            if let Some(mid) = self.world_to_pixel(mid_world) {
                if !self.occluded_on_screen(mid_world) {
                    halo_text(out, (mid.x + 4.0, mid.y - 5.0), &text, ink, size);
                }
            }
        }
    }

    fn overlay_text_labels(&mut self, out: &mut Vec<Paint>) {
        if self.scene.text_labels.is_empty() {
            return;
        }
        let default_ink = self.style_ink();
        let size = self.style_font_size();
        let fm = FontMetrics::new(size, true);
        let labels: Vec<_> = self
            .scene
            .text_labels
            .iter()
            .filter(|l| self.scene.entity_visible(&EntityRef::TextLabel(l.id)))
            .map(|l| (l.id, l.anchor, l.position(), l.text.clone()))
            .collect();
        for (id, anchor, pos, text) in labels {
            let ink = self.ink_for(EntityRef::TextLabel(id), default_ink);
            let p_anchor = self.world_to_pixel(anchor);
            let Some(p_pos) = self.world_to_pixel(pos) else {
                continue;
            };
            self.occluded_segment(out, anchor, pos, Pen::solid(ink, 1.2));
            if let Some(pa) = p_anchor {
                if !self.occluded_on_screen(anchor) {
                    out.push(Paint::Ellipse {
                        center: pa.tuple(),
                        r: 2.5,
                        pen: Some(Pen::solid(ink, 1.2)),
                        fill: Some(ink),
                    });
                }
            }
            let mut lines: Vec<&str> = text.lines().collect();
            if lines.is_empty() {
                lines.push("");
            }
            let block_w = lines
                .iter()
                .map(|l| fm.horizontal_advance(l))
                .fold(0.0, f64::max);
            let x = Viewport::text_block_x(p_pos.tuple(), p_anchor.map(|p| p.tuple()), block_w);
            for (i, line) in lines.iter().enumerate() {
                let y = p_pos.y - 4.0 + i as f64 * fm.height();
                halo_text(out, (x, y), line, ink, size as f32);
            }
        }
    }
}
