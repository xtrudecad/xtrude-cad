// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Toolbar icons and tool cursors, drawn in code: each is the plainest
//! picture of what the tool does, on a 48-px canvas, ink in the theme's
//! text colour and the Xtrude CAD blue as the accent. The painter is a
//! small state-keeping layer over tiny-skia (pen, brush, transform stack),
//! whose pens scale the dash pattern with the pen width and default to a
//! square cap; the icon canvas's own pen is round. An icon's text is collected and drawn
//! by the caller (tiny-skia has no fonts); a cursor's is baked into its
//! pixels as glyph outlines, since the OS shows the cursor.

use std::f32::consts::PI;

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, Rect, Stroke,
    StrokeDash, Transform,
};

/// The canvas the icons are drawn on.
pub const PX: f32 = 48.0;

/// An 8-bit RGBA colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    fn alpha(self, a: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }

    fn color(self) -> Color {
        Color::from_rgba8(self.0, self.1, self.2, self.3)
    }
}

/// The Xtrude CAD blue.
pub const ACCENT: Rgba = Rgba(54, 137, 230, 255);

/// A stroke style: colour, width, dashes, cap and join.
#[derive(Clone, Copy, Debug)]
pub struct Pen {
    color: Rgba,
    width: f32,
    dash: bool,
    cap: LineCap,
    join: LineJoin,
}

impl Pen {
    /// A solid pen: square caps, bevel joins.
    pub fn new(color: Rgba, width: f32) -> Pen {
        Pen {
            color,
            width,
            dash: false,
            cap: LineCap::Square,
            join: LineJoin::Bevel,
        }
    }

    /// Round caps and joins.
    pub fn round(color: Rgba, width: f32) -> Pen {
        Pen {
            cap: LineCap::Round,
            join: LineJoin::Round,
            ..Pen::new(color, width)
        }
    }

    pub fn dashed(mut self) -> Pen {
        self.dash = true;
        self
    }

    fn cap(mut self, c: LineCap) -> Pen {
        self.cap = c;
        self
    }

    fn join(mut self, j: LineJoin) -> Pen {
        self.join = j;
        self
    }

    fn stroke(&self, scale: f32) -> Stroke {
        let mut s = Stroke {
            width: self.width,
            line_cap: self.cap,
            line_join: self.join,
            ..Stroke::default()
        };
        if self.dash {
            // Dashes 4 on, 2 off, in pen widths.
            let w = self.width.max(1.0 / scale.max(1e-3));
            s.dash = StrokeDash::new(vec![4.0 * w, 2.0 * w], 0.0);
        }
        s
    }
}

/// A path being drawn, as a tiny-skia builder plus its current point.
#[derive(Clone, Default)]
pub struct IconPath {
    b: PathBuilder,
    cur: (f32, f32),
}

impl IconPath {
    pub fn new() -> IconPath {
        IconPath::default()
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.b.move_to(x, y);
        self.cur = (x, y);
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.b.line_to(x, y);
        self.cur = (x, y);
        self
    }

    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        self.b.quad_to(cx, cy, x, y);
        self.cur = (x, y);
        self
    }

    pub fn cubic_to(
        &mut self,
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    ) -> &mut Self {
        self.b.cubic_to(c1x, c1y, c2x, c2y, x, y);
        self.cur = (x, y);
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.b.close();
        self
    }

    /// `arcTo(rect, start°, sweep°)`: a line to the arc's start, then the
    /// arc (angles in degrees, counter-clockwise with y up).
    pub fn arc_to(&mut self, r: (f32, f32, f32, f32), start: f32, sweep: f32) -> &mut Self {
        let (x, y, w, h) = r;
        let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let at = |deg: f32| {
            let a = deg * PI / 180.0;
            (cx + rx * a.cos(), cy - ry * a.sin())
        };
        let p0 = at(start);
        self.line_to(p0.0, p0.1);
        arc_segments(&mut self.b, cx, cy, rx, ry, start, sweep);
        self.cur = at(start + sweep);
        self
    }

    pub fn add_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) -> &mut Self {
        if let Some(r) = Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry) {
            self.b.push_oval(r);
        }
        self
    }

    pub fn add_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
        ry: f32,
    ) -> &mut Self {
        rounded_rect(&mut self.b, x, y, w, h, rx, ry);
        self
    }

    fn finish(&self) -> Option<Path> {
        self.b.clone().finish()
    }
}

/// Cubic segments of an elliptical arc (≤ 90° each).
fn arc_segments(b: &mut PathBuilder, cx: f32, cy: f32, rx: f32, ry: f32, start: f32, sweep: f32) {
    let n = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep / n as f32;
    for i in 0..n {
        let a0 = (start + step * i as f32) * PI / 180.0;
        let a1 = (start + step * (i + 1) as f32) * PI / 180.0;
        let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan();
        let (c0, s0, c1, s1) = (a0.cos(), a0.sin(), a1.cos(), a1.sin());
        // y down: point = (cx + rx cos, cy − ry sin)
        let p1 = (cx + rx * (c0 - k * s0), cy - ry * (s0 + k * c0));
        let p2 = (cx + rx * (c1 + k * s1), cy - ry * (s1 - k * c1));
        let p3 = (cx + rx * c1, cy - ry * s1);
        b.cubic_to(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1);
    }
}

fn rounded_rect(b: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) {
    let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
    b.move_to(x + rx, y);
    b.line_to(x + w - rx, y);
    arc_segments(b, x + w - rx, y + ry, rx, ry, 90.0, -90.0);
    b.line_to(x + w, y + h - ry);
    arc_segments(b, x + w - rx, y + h - ry, rx, ry, 0.0, -90.0);
    b.line_to(x + rx, y + h);
    arc_segments(b, x + rx, y + h - ry, rx, ry, 270.0, -90.0);
    b.line_to(x, y + ry);
    arc_segments(b, x + rx, y + ry, rx, ry, 180.0, -90.0);
    b.close();
}

/// Text the caller draws over the icon (48-space position, after the
/// painter's transform).
#[derive(Clone, Debug, PartialEq)]
pub struct IconText {
    pub at: (f32, f32),
    pub text: String,
    pub px: f32,
    pub bold: bool,
    pub color: Rgba,
    /// Centre the text on `at` (both ways) or put its
    /// baseline start there.
    pub centered: bool,
}

#[derive(Clone, Copy)]
struct State {
    t: Transform,
    pen: Option<Pen>,
    brush: Option<Rgba>,
    font_px: f32,
    bold: bool,
}

/// A painter on a pixmap: pen, brush, font and a transform stack.
pub struct Painter {
    pub pixmap: Pixmap,
    scale: f32,
    st: State,
    stack: Vec<State>,
    pub texts: Vec<IconText>,
}

impl Painter {
    /// A canvas of `PX × scale` pixels with a round 3-px pen.
    pub fn canvas(scale: f32, ink: Rgba) -> Option<Painter> {
        let side = (PX * scale).round() as u32;
        Some(Painter {
            pixmap: Pixmap::new(side, side)?,
            scale,
            st: State {
                t: Transform::from_scale(scale, scale),
                pen: Some(Pen::round(ink, 3.0)),
                brush: None,
                font_px: 12.0,
                bold: false,
            },
            stack: Vec::new(),
            texts: Vec::new(),
        })
    }

    pub fn save(&mut self) {
        self.stack.push(self.st);
    }

    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.st = s;
        }
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.st.t = self.st.t.pre_translate(x, y);
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.st.t = self.st.t.pre_scale(sx, sy);
    }

    pub fn rotate(&mut self, deg: f32) {
        self.st.t = self.st.t.pre_concat(Transform::from_rotate(deg));
    }

    pub fn set_pen(&mut self, p: Pen) {
        self.st.pen = Some(p);
    }

    pub fn no_pen(&mut self) {
        self.st.pen = None;
    }

    pub fn pen(&self) -> Option<Pen> {
        self.st.pen
    }

    pub fn set_brush(&mut self, c: Rgba) {
        self.st.brush = Some(c);
    }

    pub fn no_brush(&mut self) {
        self.st.brush = None;
    }

    pub fn set_font(&mut self, px: f32, bold: bool) {
        self.st.font_px = px;
        self.st.bold = bold;
    }

    fn paint(c: Rgba) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color(c.color());
        p.anti_alias = true;
        p
    }

    /// Fill with the brush, then stroke with the pen, so the outline stays
    /// on top.
    pub fn draw(&mut self, path: &Path, mask: Option<&Mask>) {
        if let Some(b) = self.st.brush {
            self.pixmap
                .fill_path(path, &Self::paint(b), FillRule::Winding, self.st.t, mask);
        }
        self.stroke_only(path, mask);
    }

    fn stroke_only(&mut self, path: &Path, mask: Option<&Mask>) {
        if let Some(p) = self.st.pen {
            let mut s = p.stroke(self.scale);
            if s.width <= 0.0 {
                s.width = 1.0 / self.scale;
            }
            self.pixmap
                .stroke_path(path, &Self::paint(p.color), &s, self.st.t, mask);
        }
    }

    pub fn draw_path(&mut self, p: &IconPath) {
        if let Some(path) = p.finish() {
            self.draw(&path, None);
        }
    }

    pub fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let mut b = PathBuilder::new();
        b.move_to(x0, y0);
        b.line_to(x1, y1);
        if let Some(path) = b.finish() {
            self.stroke_only(&path, None);
        }
    }

    pub fn draw_polyline_closed(&mut self, pts: &[(f32, f32)]) {
        let mut b = PathBuilder::new();
        for (i, &(x, y)) in pts.iter().enumerate() {
            if i == 0 {
                b.move_to(x, y);
            } else {
                b.line_to(x, y);
            }
        }
        b.close();
        if let Some(path) = b.finish() {
            self.draw(&path, None);
        }
    }

    pub fn draw_polygon(&mut self, pts: &[(f32, f32)]) {
        self.draw_polyline_closed(pts);
    }

    pub fn draw_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            self.draw(&PathBuilder::from_rect(r), None);
        }
    }

    pub fn draw_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) {
        let mut b = PathBuilder::new();
        rounded_rect(&mut b, x, y, w, h, rx, ry);
        if let Some(path) = b.finish() {
            self.draw(&path, None);
        }
    }

    pub fn draw_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
        if let Some(path) = PathBuilder::from_oval(
            Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry)
                .unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
        ) {
            self.draw(&path, None);
        }
    }

    /// `drawArc(rect, start·16, span·16)`: stroke only.
    pub fn draw_arc(&mut self, x: f32, y: f32, w: f32, h: f32, start16: f32, span16: f32) {
        let (start, sweep) = (start16 / 16.0, span16 / 16.0);
        let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let a = start * PI / 180.0;
        let mut b = PathBuilder::new();
        b.move_to(cx + rx * a.cos(), cy - ry * a.sin());
        arc_segments(&mut b, cx, cy, rx, ry, start, sweep);
        if let Some(path) = b.finish() {
            self.stroke_only(&path, None);
        }
    }

    /// `drawText`: recorded for the caller, in 48-space.
    pub fn draw_text(&mut self, x: f32, y: f32, text: &str, centered: bool) {
        let color = self.st.pen.map_or(Rgba(0, 0, 0, 255), |p| p.color);
        let t = self.st.t.post_scale(1.0 / self.scale, 1.0 / self.scale);
        let mut pt = [tiny_skia::Point::from_xy(x, y)];
        t.map_points(&mut pt);
        let s = (t.sx * t.sx + t.ky * t.ky).sqrt();
        self.texts.push(IconText {
            at: (pt[0].x, pt[0].y),
            text: text.to_string(),
            px: self.st.font_px * s,
            bold: self.st.bold,
            color,
            centered,
        });
    }

    fn mask(&self) -> Option<Mask> {
        Mask::new(self.pixmap.width(), self.pixmap.height())
    }
}

// ---- helpers ------------------------------------------------------------------

fn dot(p: &mut Painter, x: f32, y: f32, r: f32, color: Option<Rgba>) {
    p.save();
    p.no_pen();
    p.set_brush(color.unwrap_or(ACCENT));
    p.draw_ellipse(x, y, r, r);
    p.restore();
}

fn guide(p: &mut Painter, ink: Rgba, a: (f32, f32), b: (f32, f32), dashed: bool) {
    let mut pen = Pen::new(ink.alpha(140), 1.8).cap(LineCap::Round);
    if dashed {
        pen = pen.dashed();
    }
    p.save();
    p.set_pen(pen);
    p.draw_line(a.0, a.1, b.0, b.1);
    p.restore();
}

fn solid_arrow(p: &mut Painter, color: Rgba, from: (f32, f32), to: (f32, f32), w: f32, head: f32) {
    let ((x0, y0), (x1, y1)) = (from, to);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let l = dx.hypot(dy).max(1e-6);
    let (ux, uy) = (dx / l, dy / l);
    let (bx, by) = (x1 - ux * head, y1 - uy * head);
    p.save();
    p.set_pen(Pen::new(color, w).cap(LineCap::Butt));
    p.draw_line(x0, y0, bx + ux, by + uy);
    p.no_pen();
    p.set_brush(color);
    let hw = head * 0.7;
    p.draw_polygon(&[
        (x1, y1),
        (bx - uy * hw, by + ux * hw),
        (bx + uy * hw, by - ux * hw),
    ]);
    p.restore();
}

fn magnifier(p: &mut Painter, ink: Rgba, cx: f32, cy: f32, r: f32, handle: bool) {
    p.no_brush();
    p.draw_ellipse(cx, cy, r, r);
    if handle {
        let d = r / 2f32.sqrt();
        p.save();
        p.set_pen(Pen::new(ink, 4.0).cap(LineCap::Round));
        p.draw_line(cx + d, cy + d, cx + d + 8.0, cy + d + 8.0);
        p.restore();
    }
}

fn corner_brackets(p: &mut Painter) {
    let (a, b, arm) = (7.0, 41.0, 8.0);
    for (x, y, dx, dy) in [
        (a, a, 1.0, 1.0),
        (b, a, -1.0, 1.0),
        (a, b, 1.0, -1.0),
        (b, b, -1.0, -1.0),
    ] {
        p.draw_line(x, y, x + dx * arm, y);
        p.draw_line(x, y, x, y + dy * arm);
    }
}

fn dim_tick(p: &mut Painter, ink: Rgba, x: f32, y: f32) {
    let s = 3.5;
    p.save();
    p.no_pen();
    p.set_brush(ink);
    p.draw_ellipse(x, y, s * 0.8, s * 0.8);
    p.restore();
}

// ---- the icons ------------------------------------------------------------------

fn select(p: &mut Painter, ink: Rgba) {
    let mut path = IconPath::new();
    path.move_to(16.0, 12.0)
        .line_to(16.0, 36.0)
        .line_to(23.0, 29.0)
        .line_to(28.0, 39.0)
        .line_to(32.0, 37.0)
        .line_to(27.0, 27.0)
        .line_to(36.0, 27.0)
        .close();
    p.set_brush(ink);
    p.draw_path(&path);
}

fn line(p: &mut Painter, _ink: Rgba) {
    p.draw_line(12.0, 36.0, 36.0, 12.0);
    dot(p, 12.0, 36.0, 3.2, None);
    dot(p, 36.0, 12.0, 3.2, None);
}

fn freehand(p: &mut Painter, _ink: Rgba) {
    let mut path = IconPath::new();
    path.move_to(9.0, 33.0)
        .cubic_to(15.0, 33.0, 19.0, 16.0, 26.0, 17.0)
        .cubic_to(33.0, 18.0, 30.0, 30.0, 24.0, 28.0)
        .cubic_to(19.0, 26.0, 24.0, 14.0, 31.0, 16.0)
        .cubic_to(36.0, 17.0, 37.0, 26.0, 40.0, 29.0);
    p.no_brush();
    p.draw_path(&path);
    dot(p, 9.0, 33.0, 3.2, None);
}

fn rectangle(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_rect(11.0, 14.0, 26.0, 20.0);
    guide(p, ink, (11.0, 34.0), (37.0, 14.0), true);
    dot(p, 11.0, 34.0, 2.9, None);
    dot(p, 37.0, 14.0, 2.9, None);
}

fn rectangle_center(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_rect(11.0, 14.0, 26.0, 20.0);
    guide(p, ink, (24.0, 24.0), (37.0, 14.0), true);
    dot(p, 24.0, 24.0, 2.9, None);
    dot(p, 37.0, 14.0, 2.9, None);
}

/// An obround and the segment between its two centres.
fn slot(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_rounded_rect(7.0, 16.0, 34.0, 16.0, 8.0, 8.0);
    guide(p, ink, (15.0, 24.0), (33.0, 24.0), true);
    dot(p, 15.0, 24.0, 2.9, None);
    dot(p, 33.0, 24.0, 2.9, None);
}

fn rotated_rect(p: &mut Painter, _ink: Rgba) {
    let pp = (10.0f32, 38.0f32);
    let ang = 28f32.to_radians();
    let (l, w) = (23.0, 13.0);
    let b = (pp.0 + l * ang.cos(), pp.1 - l * ang.sin());
    let c = (b.0 - w * ang.sin(), b.1 - w * ang.cos());
    let d = (pp.0 - w * ang.sin(), pp.1 - w * ang.cos());
    p.no_brush();
    p.draw_polygon(&[pp, b, c, d]);
    let (cx, cy, r) = (pp.0, pp.1, 30.0);
    p.save();
    p.set_pen(Pen::round(ACCENT, 2.8));
    p.no_brush();
    p.draw_arc(cx - r, cy - r, 2.0 * r, 2.0 * r, 12.0 * 16.0, 58.0 * 16.0);
    let a = 70f32.to_radians();
    let tip = (cx + r * a.cos(), cy - r * a.sin());
    p.no_pen();
    p.set_brush(ACCENT);
    let (tx, ty) = (-a.sin(), -a.cos());
    let (nx, ny) = (-ty, tx);
    p.draw_polygon(&[
        (tip.0 + tx * 6.5, tip.1 + ty * 6.5),
        (tip.0 + nx * 4.5, tip.1 + ny * 4.5),
        (tip.0 - nx * 4.5, tip.1 - ny * 4.5),
    ]);
    p.restore();
    dot(p, pp.0, pp.1, 3.3, None);
}

fn circle(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_ellipse(24.0, 24.0, 13.0, 13.0);
    guide(p, ink, (24.0, 24.0), (37.0, 24.0), true);
    dot(p, 24.0, 24.0, 2.9, None);
    dot(p, 37.0, 24.0, 2.9, None);
}

fn polygon(p: &mut Painter, ink: Rgba) {
    let pts: Vec<(f32, f32)> = (0..6)
        .map(|i| {
            let a = (60.0 * i as f32 - 30.0).to_radians();
            (24.0 + 13.0 * a.cos(), 24.0 + 13.0 * a.sin())
        })
        .collect();
    p.no_brush();
    p.draw_polygon(&pts);
    guide(p, ink, (24.0, 24.0), pts[0], true);
    dot(p, 24.0, 24.0, 2.9, None);
    dot(p, pts[0].0, pts[0].1, 2.9, None);
}

fn arc_shape(p: &mut Painter) {
    let mut path = IconPath::new();
    path.move_to(11.0, 34.0).quad_to(24.0, 4.0, 37.0, 34.0);
    p.no_brush();
    p.draw_path(&path);
}

fn arc(p: &mut Painter, ink: Rgba) {
    arc_shape(p);
    guide(p, ink, (11.0, 34.0), (37.0, 34.0), true);
    guide(p, ink, (24.0, 34.0), (24.0, 19.0), true);
    dot(p, 11.0, 34.0, 2.9, None);
    dot(p, 37.0, 34.0, 2.9, None);
    dot(p, 24.0, 19.0, 2.4, None);
}

fn arc3(p: &mut Painter, _ink: Rgba) {
    arc_shape(p);
    dot(p, 11.0, 34.0, 2.9, None);
    dot(p, 37.0, 34.0, 2.9, None);
    dot(p, 24.0, 19.0, 2.9, None);
}

fn sector(p: &mut Painter, _ink: Rgba) {
    let mut wedge = IconPath::new();
    wedge
        .move_to(24.0, 26.0)
        .arc_to((9.0, 11.0, 30.0, 30.0), 20.0, 110.0)
        .close();
    p.save();
    p.no_pen();
    p.set_brush(ACCENT.alpha(110));
    p.draw_path(&wedge);
    p.restore();
    p.no_brush();
    p.draw_path(&wedge);
    dot(p, 24.0, 26.0, 2.9, None);
}

fn rotate(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    let (cx, cy, r) = (24.0f32, 24.0f32, 11.0f32);
    p.draw_arc(
        cx - r,
        cy - r,
        2.0 * r,
        2.0 * r,
        158.0 * 16.0,
        -138.0 * 16.0,
    );
    p.draw_arc(
        cx - r,
        cy - r,
        2.0 * r,
        2.0 * r,
        338.0 * 16.0,
        -138.0 * 16.0,
    );
    for a_deg in [20.0f32, 200.0] {
        let a = a_deg.to_radians();
        let (px, py) = (cx + r * a.cos(), cy - r * a.sin());
        let (bx, by) = (-a.sin(), -a.cos());
        for rot in [32f32.to_radians(), (-32f32).to_radians()] {
            let dx = bx * rot.cos() - by * rot.sin();
            let dy = bx * rot.sin() + by * rot.cos();
            p.draw_line(px, py, px + 6.5 * dx, py + 6.5 * dy);
        }
    }
    dot(p, cx, cy, 2.4, None);
}

fn center_arc(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_arc(10.0, 10.0, 28.0, 28.0, 0.0, 105.0 * 16.0);
    let a = 105f32.to_radians();
    let (ex, ey) = (24.0 + 14.0 * a.cos(), 24.0 - 14.0 * a.sin());
    guide(p, ink, (24.0, 24.0), (38.0, 24.0), true);
    guide(p, ink, (24.0, 24.0), (ex, ey), true);
    dot(p, 24.0, 24.0, 2.9, None);
    dot(p, 38.0, 24.0, 2.9, None);
    dot(p, ex, ey, 2.9, None);
}

fn flip(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.save();
    p.set_pen(Pen::new(ink, 2.0).dashed());
    p.draw_line(24.0, 8.0, 24.0, 40.0);
    p.restore();
    p.set_brush(ink);
    p.draw_polygon(&[(20.0, 14.0), (20.0, 34.0), (10.0, 30.0)]);
    p.no_brush();
    p.draw_polygon(&[(28.0, 14.0), (28.0, 34.0), (38.0, 30.0)]);
}

fn scale(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_rect(11.0, 27.0, 10.0, 10.0);
    p.draw_rect(17.0, 11.0, 20.0, 20.0);
    p.draw_line(14.0, 34.0, 33.0, 15.0);
    p.draw_line(33.0, 15.0, 27.0, 15.0);
    p.draw_line(33.0, 15.0, 33.0, 21.0);
}

fn sweep(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    let mut path = IconPath::new();
    path.move_to(12.0, 36.0).quad_to(14.0, 16.0, 36.0, 14.0);
    p.draw_path(&path);
    p.save();
    p.translate(12.0, 36.0);
    p.rotate(-75.0);
    p.draw_rect(-4.5, -4.5, 9.0, 9.0);
    p.restore();
    dot(p, 36.0, 14.0, 3.2, None);
}

/// Three squares in a row: the original in ink, its copies in the
/// accent, the direction dashed beneath them.
fn pattern_linear(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_rect(5.0, 14.0, 11.0, 11.0);
    p.save();
    p.set_pen(Pen::round(ACCENT, 2.8));
    p.draw_rect(19.0, 14.0, 11.0, 11.0);
    p.draw_rect(33.0, 14.0, 11.0, 11.0);
    p.restore();
    guide(p, ink, (10.5, 34.0), (31.0, 34.0), true);
    solid_arrow(p, ACCENT, (30.0, 34.0), (40.0, 34.0), 2.4, 5.5);
}

/// Squares round a centre: the original in ink, its copies in the accent.
fn pattern_circular(p: &mut Painter, ink: Rgba) {
    let (cx, cy, r, h) = (24.0f32, 24.0f32, 15.0f32, 3.4f32);
    p.no_brush();
    p.save();
    p.set_pen(Pen::new(ink.alpha(110), 1.6).dashed());
    p.draw_ellipse(cx, cy, r, r);
    p.restore();
    for k in 0..6 {
        let a = (90.0 - 60.0 * k as f32).to_radians();
        let (x, y) = (cx + r * a.cos(), cy - r * a.sin());
        p.save();
        if k > 0 {
            p.set_pen(Pen::round(ACCENT, 2.4));
        }
        p.draw_rect(x - h, y - h, 2.0 * h, 2.0 * h);
        p.restore();
    }
    dot(p, cx, cy, 2.4, None);
}

/// A profile beside its dashed axis, and the turn round the axis.
fn revolve(p: &mut Painter, ink: Rgba) {
    guide(p, ink, (15.0, 4.0), (15.0, 44.0), true);
    p.no_brush();
    p.draw_rect(21.0, 8.0, 10.0, 21.0);
    p.save();
    p.set_pen(Pen::round(ACCENT, 2.8));
    // the front half of the turn, right to left, round the axis
    p.draw_arc(4.0, 31.0, 22.0, 10.0, 0.0, -180.0 * 16.0);
    p.restore();
    solid_arrow(p, ACCENT, (4.0, 37.0), (4.5, 29.0), 2.8, 6.0);
}

const SECTION_FRAME: [(f32, f32); 4] = [(9.0, 30.0), (26.0, 39.0), (39.0, 21.0), (22.0, 12.0)];

fn section(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_rect(9.0, 13.0, 30.0, 22.0);
    p.save();
    p.set_pen(Pen::new(ACCENT, 3.0).dashed().cap(LineCap::Round));
    p.draw_line(24.0, 6.0, 24.0, 42.0);
    p.restore();
    solid_arrow(p, ACCENT, (28.0, 9.0), (38.0, 9.0), 2.8, 6.0);
}

fn section_planes(p: &mut Painter, ink: Rgba) {
    p.save();
    p.set_pen(Pen::new(ink.alpha(110), 1.8).dashed());
    p.no_brush();
    p.draw_polygon(&SECTION_FRAME);
    p.restore();
    p.save();
    p.set_pen(Pen::new(ink, 3.0).cap(LineCap::Round));
    let pts = SECTION_FRAME;
    for i in 0..4 {
        let c = pts[i];
        for j in [1i32, -1] {
            let n = pts[((i as i32 + j).rem_euclid(4)) as usize];
            let (dx, dy) = (n.0 - c.0, n.1 - c.1);
            let l = dx.hypot(dy).max(1e-6);
            p.draw_line(c.0, c.1, c.0 + dx * 7.0 / l, c.1 + dy * 7.0 / l);
        }
    }
    p.restore();
}

fn section_cuts(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_line(24.0, 13.0, 39.0, 13.0);
    p.draw_line(39.0, 13.0, 39.0, 35.0);
    p.draw_line(39.0, 35.0, 24.0, 35.0);
    p.save();
    p.set_pen(Pen::new(ACCENT, 4.0).cap(LineCap::Round));
    p.draw_line(24.0, 13.0, 24.0, 35.0);
    p.restore();
}

fn section_fill(p: &mut Painter, ink: Rgba) {
    section_cuts(p, ink);
    p.save();
    p.no_pen();
    p.set_brush(ACCENT.alpha(150));
    p.draw_rect(24.0, 13.0, 15.0, 22.0);
    p.restore();
}

fn protractor(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_arc(10.0, 10.0, 28.0, 28.0, 0.0, 180.0 * 16.0);
    p.draw_line(10.0, 24.0, 38.0, 24.0);
    for adeg in [30.0f32, 60.0, 90.0, 120.0, 150.0] {
        let a = adeg.to_radians();
        p.draw_line(
            24.0 + 11.0 * a.cos(),
            24.0 - 11.0 * a.sin(),
            24.0 + 14.0 * a.cos(),
            24.0 - 14.0 * a.sin(),
        );
    }
    if let Some(pen) = p.pen() {
        p.set_pen(pen.dashed());
        p.draw_line(24.0, 24.0, 40.0, 11.0);
        p.set_pen(pen);
    }
    dot(p, 24.0, 24.0, 3.2, None);
}

fn pushpull(p: &mut Painter, ink: Rgba) {
    let (cx, y_top, half_w, half_h, thick) = (24.0f32, 33.0f32, 15.0f32, 7.0f32, 5.0f32);
    let top = [
        (cx - half_w, y_top),
        (cx, y_top - half_h),
        (cx + half_w, y_top),
        (cx, y_top + half_h),
    ];
    p.no_brush();
    p.save();
    p.no_pen();
    p.set_brush(ACCENT.alpha(130));
    p.draw_polygon(&top);
    p.restore();
    p.draw_polygon(&top);
    for (x, y) in [
        (cx - half_w, y_top),
        (cx, y_top + half_h),
        (cx + half_w, y_top),
    ] {
        p.draw_line(x, y, x, y + thick);
    }
    p.draw_line(cx - half_w, y_top + thick, cx, y_top + half_h + thick);
    p.draw_line(cx, y_top + half_h + thick, cx + half_w, y_top + thick);
    p.save();
    p.set_pen(Pen::new(ink, 5.0).cap(LineCap::Butt));
    p.draw_line(cx, 32.0, cx, 21.0);
    p.restore();
    p.save();
    p.no_pen();
    p.set_brush(ink);
    p.draw_polygon(&[(cx, 11.0), (cx - 8.5, 22.0), (cx + 8.5, 22.0)]);
    p.restore();
}

fn offset(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_rect(10.0, 12.0, 28.0, 24.0);
    p.draw_rect(16.0, 18.0, 16.0, 12.0);
}

fn fillet(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.save();
    p.set_pen(Pen::new(ink, 1.6).dashed());
    p.draw_line(12.0, 24.0, 12.0, 12.0);
    p.draw_line(12.0, 12.0, 24.0, 12.0);
    p.restore();
    let mut path = IconPath::new();
    path.move_to(12.0, 38.0)
        .line_to(12.0, 24.0)
        .quad_to(12.0, 12.0, 24.0, 12.0)
        .line_to(38.0, 12.0);
    p.draw_path(&path);
    p.save();
    p.set_pen(Pen::new(ACCENT, 3.0));
    let mut arc = IconPath::new();
    arc.move_to(12.0, 24.0).quad_to(12.0, 12.0, 24.0, 12.0);
    p.draw_path(&arc);
    p.restore();
}

/// A corner in profile, its edge cut flat at 45°.
fn chamfer(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.save();
    p.set_pen(Pen::new(ink, 1.6).dashed());
    p.draw_line(12.0, 24.0, 12.0, 12.0);
    p.draw_line(12.0, 12.0, 24.0, 12.0);
    p.restore();
    let mut path = IconPath::new();
    path.move_to(12.0, 38.0)
        .line_to(12.0, 24.0)
        .line_to(24.0, 12.0)
        .line_to(38.0, 12.0);
    p.draw_path(&path);
    p.save();
    p.set_pen(Pen::new(ACCENT, 3.0));
    p.draw_line(12.0, 24.0, 24.0, 12.0);
    p.restore();
}

/// A plate seen from above at an angle, the hole through it in the accent
/// and its axis dashed.
fn hole(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_polygon(&[(4.0, 24.0), (24.0, 14.0), (44.0, 24.0), (24.0, 34.0)]);
    let mut sides = IconPath::new();
    sides
        .move_to(4.0, 24.0)
        .line_to(4.0, 30.0)
        .line_to(24.0, 40.0)
        .line_to(44.0, 30.0)
        .line_to(44.0, 24.0);
    p.draw_path(&sides);
    p.draw_line(24.0, 34.0, 24.0, 40.0);
    p.save();
    p.no_pen();
    p.set_brush(ACCENT);
    p.draw_ellipse(24.0, 24.0, 8.0, 4.0);
    p.restore();
    guide(p, ink, (24.0, 6.0), (24.0, 18.0), true);
}

/// A box in section, hollowed with its top open: the walls and floor, the
/// cavity in the accent.
fn shell_icon(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_polygon(&[
        (9.0, 12.0),
        (9.0, 38.0),
        (39.0, 38.0),
        (39.0, 12.0),
        (33.0, 12.0),
        (33.0, 32.0),
        (15.0, 32.0),
        (15.0, 12.0),
    ]);
    p.save();
    p.set_pen(Pen::new(ACCENT, 3.0).cap(LineCap::Round));
    let mut cavity = IconPath::new();
    cavity
        .move_to(15.0, 12.0)
        .line_to(15.0, 32.0)
        .line_to(33.0, 32.0)
        .line_to(33.0, 12.0);
    p.draw_path(&cavity);
    p.restore();
}

fn move_icon(p: &mut Painter, _ink: Rgba) {
    p.draw_line(24.0, 10.0, 24.0, 38.0);
    p.draw_line(10.0, 24.0, 38.0, 24.0);
    for (x, y, dx1, dy1, dx2, dy2) in [
        (24.0, 10.0, -4.0, 5.0, 4.0, 5.0),
        (24.0, 38.0, -4.0, -5.0, 4.0, -5.0),
        (10.0, 24.0, 5.0, -4.0, 5.0, 4.0),
        (38.0, 24.0, -5.0, -4.0, -5.0, 4.0),
    ] {
        p.draw_line(x, y, x + dx1, y + dy1);
        p.draw_line(x, y, x + dx2, y + dy2);
    }
}

fn eyedropper(p: &mut Painter, ink: Rgba) {
    let pen = Pen::round(ink, 3.0);
    p.save();
    p.translate(24.0, 24.0);
    p.scale(0.85, 0.85);
    p.translate(-24.0, -24.0);
    p.set_pen(pen);
    p.no_brush();
    p.draw_line(26.5, 17.0, 11.0, 32.5);
    p.draw_line(31.0, 21.5, 15.5, 37.0);
    p.draw_line(11.0, 32.5, 8.0, 40.0);
    p.draw_line(15.5, 37.0, 8.0, 40.0);
    p.draw_line(23.5, 14.5, 33.5, 24.5);
    p.save();
    p.no_pen();
    p.set_brush(ink);
    p.draw_rounded_rect(26.5, 5.0, 16.0, 16.0, 6.5, 6.5);
    p.restore();
    p.draw_line(29.5, 12.0, 35.5, 18.0);
    dot(p, 5.5, 43.5, 2.6, None);
    p.restore();
}

fn paint(p: &mut Painter, ink: Rgba) {
    let (s, ox, oy) = (2.5f32, 1.0f32, 0.0f32);
    let pt = |u: f32, v: f32| (u * s + ox, v * s + oy);
    let (top, right, bottom, left) = (
        pt(10.29, 2.4),
        pt(14.89, 7.0),
        pt(10.29, 11.6),
        pt(5.69, 7.0),
    );
    p.no_pen();
    p.set_brush(ACCENT);
    p.draw_polygon(&[left, right, bottom]);
    p.set_pen(Pen::new(ink, 3.4).join(LineJoin::Round));
    p.no_brush();
    p.draw_polygon(&[top, right, bottom, left]);
    p.set_pen(Pen::new(ink, 4.0).cap(LineCap::Round));
    let (a, b) = (pt(10.9, 2.5), pt(12.3, 1.3));
    p.draw_line(a.0, a.1, b.0, b.1);
    p.no_pen();
    p.set_brush(ink);
    p.draw_polygon(&[pt(5.88, 5.74), pt(4.0, 7.4), pt(7.0, 9.0)]);
    p.set_brush(ACCENT);
    let mut drop = IconPath::new();
    let q = |path: &mut IconPath, c: (f32, f32), e: (f32, f32)| {
        path.quad_to(c.0, c.1, e.0, e.1);
    };
    let st = pt(3.5, 9.0);
    drop.move_to(st.0, st.1);
    q(&mut drop, pt(6.0, 11.6), pt(6.0, 13.4));
    q(&mut drop, pt(6.0, 16.0), pt(3.5, 16.0));
    q(&mut drop, pt(1.0, 16.0), pt(1.0, 13.4));
    q(&mut drop, pt(1.0, 11.6), pt(3.5, 9.0));
    p.draw_path(&drop);
}

fn dimension(p: &mut Painter, ink: Rgba) {
    let (a, b) = ((10.0f32, 39.0f32), (38.0f32, 39.0f32));
    p.draw_line(a.0, a.1, b.0, b.1);
    guide(p, ink, (a.0, 37.0), (a.0, 17.0), true);
    guide(p, ink, (b.0, 37.0), (b.0, 17.0), true);
    p.draw_line(a.0, 21.0, b.0, 21.0);
    dim_tick(p, ink, a.0, 21.0);
    dim_tick(p, ink, b.0, 21.0);
    p.save();
    p.set_font(13.0, true);
    p.set_pen(Pen::new(ink, 1.0));
    p.draw_text(24.0, 12.0, "3", true);
    p.restore();
    dot(p, a.0, a.1, 2.9, None);
    dot(p, b.0, b.1, 2.9, None);
}

fn dimension_style(p: &mut Painter, ink: Rgba) {
    p.draw_line(9.0, 36.0, 39.0, 36.0);
    p.draw_line(9.0, 30.0, 9.0, 42.0);
    p.draw_line(39.0, 30.0, 39.0, 42.0);
    p.save();
    p.set_pen(Pen::new(ink, 3.0).cap(LineCap::Round));
    p.draw_line(40.0, 8.0, 28.0, 20.0);
    p.no_pen();
    p.set_brush(ACCENT);
    p.draw_polygon(&[(29.0, 18.0), (31.0, 22.0), (25.0, 26.0), (22.0, 23.0)]);
    p.restore();
}

fn undo(p: &mut Painter, ink: Rgba) {
    let mut path = IconPath::new();
    path.move_to(4.0, 17.0)
        .line_to(21.0, 4.0)
        .line_to(20.0, 11.0)
        .cubic_to(37.0, 10.0, 47.0, 24.0, 33.0, 44.0)
        .cubic_to(32.0, 45.5, 29.5, 45.0, 30.0, 43.0)
        .cubic_to(36.0, 31.0, 31.0, 23.0, 19.0, 24.0)
        .line_to(18.0, 31.0)
        .close();
    p.save();
    p.translate(PX / 2.0, PX / 2.0);
    p.scale(0.8, 0.8);
    p.translate(-PX / 2.0 - 1.5, -PX / 2.0);
    p.set_pen(Pen::round(ink, 2.6 / 0.8));
    p.set_brush(ACCENT.alpha(150));
    p.draw_path(&path);
    p.restore();
}

fn redo(p: &mut Painter, ink: Rgba) {
    p.save();
    p.translate(PX, 0.0);
    p.scale(-1.0, 1.0);
    undo(p, ink);
    p.restore();
}

fn orbit(p: &mut Painter, ink: Rgba) {
    let (cx, cy, rx, ry) = (24.0f32, 25.0f32, 15.0f32, 7.6f32);
    let ring = Pen::round(ink, 3.0);
    p.set_pen(ring);
    p.no_brush();
    p.draw_arc(
        cx - rx,
        cy - ry,
        2.0 * rx,
        2.0 * ry,
        150.0 * 16.0,
        -132.0 * 16.0,
    );
    p.no_pen();
    p.set_brush(ACCENT);
    p.draw_ellipse(cx, 19.5, 7.6, 7.6);
    p.set_pen(ring);
    p.no_brush();
    let (start_a, end_a) = (214.0f32, 338.0f32);
    p.draw_arc(
        cx - rx,
        cy - ry,
        2.0 * rx,
        2.0 * ry,
        start_a * 16.0,
        (end_a - start_a) * 16.0,
    );
    let a = end_a.to_radians();
    let (px, py) = (cx + rx * a.cos(), cy - ry * a.sin());
    let (mut tx, mut ty) = (-rx * a.sin(), -ry * a.cos());
    let tl = tx.hypot(ty);
    tx /= tl;
    ty /= tl;
    for rot in [38f32.to_radians(), (-38f32).to_radians()] {
        let dx = -tx * rot.cos() + ty * rot.sin();
        let dy = -tx * rot.sin() - ty * rot.cos();
        p.draw_line(px, py, px + 7.0 * dx, py + 7.0 * dy);
    }
}

fn pan(p: &mut Painter, ink: Rgba) {
    let mut h = IconPath::new();
    h.move_to(17.5, 33.0)
        .line_to(17.5, 14.5)
        .cubic_to(17.5, 9.5, 24.5, 9.5, 24.5, 14.5)
        .line_to(24.5, 26.0)
        .cubic_to(25.0, 22.5, 30.5, 22.5, 31.0, 26.0)
        .cubic_to(31.5, 23.5, 36.5, 23.5, 37.0, 27.0)
        .cubic_to(37.5, 25.0, 42.0, 25.5, 42.0, 29.0)
        .line_to(42.0, 34.0)
        .cubic_to(42.0, 41.0, 37.0, 45.0, 31.0, 45.0)
        .line_to(23.0, 45.0)
        .cubic_to(17.0, 45.0, 13.0, 41.0, 12.0, 37.0)
        .cubic_to(11.0, 34.0, 8.0, 32.5, 8.5, 29.0)
        .cubic_to(9.0, 26.5, 12.5, 26.0, 14.5, 28.0)
        .cubic_to(15.5, 29.5, 16.5, 31.5, 17.5, 33.0)
        .close();
    p.save();
    p.translate(24.0, 25.0);
    p.scale(0.88, 0.88);
    p.translate(-24.0, -25.0);
    p.set_pen(Pen::round(ink, 2.8));
    p.no_brush();
    p.draw_path(&h);
    let r = 7.0;
    p.set_pen(Pen::new(ACCENT, 2.4).cap(LineCap::Round));
    p.draw_arc(
        21.0 - r,
        13.5 - r,
        2.0 * r,
        2.0 * r,
        20.0 * 16.0,
        140.0 * 16.0,
    );
    p.set_pen(Pen::round(ACCENT, 2.5));
    let y = 13.5;
    p.draw_line(3.5, y, 11.5, y);
    p.draw_line(3.5, y, 7.0, y - 3.5);
    p.draw_line(3.5, y, 7.0, y + 3.5);
    p.draw_line(30.5, y, 38.5, y);
    p.draw_line(38.5, y, 35.0, y - 3.5);
    p.draw_line(38.5, y, 35.0, y + 3.5);
    p.restore();
}

fn eraser(p: &mut Painter, ink: Rgba) {
    let (w, h, tip) = (30.0f32, 15.0f32, 0.38f32);
    p.save();
    p.translate(26.0, 21.0);
    p.rotate(-40.0);
    p.no_pen();
    p.set_brush(ACCENT);
    p.draw_rounded_rect(-w / 2.0, -h / 2.0, w * tip, h, 3.0, 3.0);
    p.draw_rect(-w / 2.0 + 3.0, -h / 2.0, w * tip - 3.0, h);
    p.set_pen(Pen::round(ink, 2.8));
    p.no_brush();
    let mut body = IconPath::new();
    body.add_rounded_rect(-w / 2.0, -h / 2.0, w, h, 3.0, 3.0);
    p.draw_path(&body);
    p.draw_line(-w / 2.0 + w * tip, -h / 2.0, -w / 2.0 + w * tip, h / 2.0);
    p.restore();
    for (x, y) in [(9.0, 40.0), (15.0, 42.0), (12.0, 36.0)] {
        dot(p, x, y, 1.6, Some(ink.alpha(170)));
    }
}

fn measure(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_ellipse(30.0, 20.0, 8.5, 8.5);
    p.draw_ellipse(30.0, 20.0, 2.6, 2.6);
    p.draw_line(30.0, 28.5, 10.0, 28.5);
    p.draw_line(10.0, 25.5, 10.0, 31.5);
    for x in [24.0, 19.0, 14.0] {
        p.draw_line(x, 28.5, x, 25.8);
    }
}

fn zoom(p: &mut Painter, ink: Rgba) {
    magnifier(p, ink, 21.0, 21.0, 9.0, true);
}

fn zoom_window(p: &mut Painter, ink: Rgba) {
    p.no_brush();
    p.draw_rect(9.0, 11.0, 30.0, 26.0);
    magnifier(p, ink, 21.0, 22.0, 6.5, true);
}

fn zoom_extents(p: &mut Painter, ink: Rgba) {
    p.save();
    p.set_pen(Pen::round(ACCENT, 3.4));
    p.no_brush();
    corner_brackets(p);
    p.restore();
    magnifier(p, ink, 22.0, 22.0, 7.5, true);
}

fn zoom_selection(p: &mut Painter, ink: Rgba) {
    p.save();
    p.set_pen(Pen::round(ACCENT, 3.4));
    p.no_brush();
    corner_brackets(p);
    p.set_pen(Pen::round(ink, 2.6));
    p.set_brush(ACCENT);
    p.draw_rect(17.0, 17.0, 14.0, 14.0);
    p.restore();
}

/// A box seen in perspective: its far face small, its near face (in the
/// accent) large, the edges between them running to the eye.
fn projection(p: &mut Painter, _ink: Rgba) {
    let far = [(20.0, 14.0), (34.0, 14.0), (34.0, 26.0), (20.0, 26.0)];
    let near = [(8.0, 20.0), (28.0, 20.0), (28.0, 40.0), (8.0, 40.0)];
    accent_fill(p, &near, 150);
    p.no_brush();
    p.draw_polygon(&far);
    for (a, b) in far.iter().zip(near.iter()) {
        p.draw_line(a.0, a.1, b.0, b.1);
    }
    p.draw_polygon(&near);
}

/// An "i" in a circle: what the model holds.
fn model_info(p: &mut Painter, _ink: Rgba) {
    p.no_brush();
    p.draw_ellipse(24.0, 24.0, 16.0, 16.0);
    dot(p, 24.0, 15.5, 2.8, None);
    p.save();
    p.set_pen(Pen::round(ACCENT, 4.2));
    p.draw_line(24.0, 22.0, 24.0, 33.0);
    p.restore();
}

/// A solid under the magnifier: a cube, and a lens over its corner.
fn solid_inspector(p: &mut Painter, ink: Rgba) {
    p.save();
    p.translate(-5.0, -4.0);
    cube(p, &[("top", 90), ("front", 150), ("right", 120)]);
    p.restore();
    magnifier(p, ink, 30.0, 30.0, 8.0, true);
}

// ---- the view cube ----------------------------------------------------------------

fn cube_corner(k: &str) -> (f32, f32) {
    let f = (24.0f32, 40.0f32);
    let (l, r, u) = ((-13.0f32, -6.0f32), (13.0f32, -6.0f32), (0.0f32, -16.0f32));
    let add = |a: (f32, f32), b: (f32, f32)| (a.0 + b.0, a.1 + b.1);
    match k {
        "F" => f,
        "L" => add(f, l),
        "R" => add(f, r),
        "B" => add(add(f, l), r),
        "F'" => add(f, u),
        "L'" => add(add(f, l), u),
        "R'" => add(add(f, r), u),
        _ => add(add(add(f, l), r), u),
    }
}

fn cube_face(name: &str) -> Vec<(f32, f32)> {
    let keys: [&str; 4] = match name {
        "front" => ["F", "L", "L'", "F'"],
        "right" => ["F", "R", "R'", "F'"],
        "top" => ["F'", "L'", "B'", "R'"],
        "back" => ["B", "R", "R'", "B'"],
        "left" => ["B", "L", "L'", "B'"],
        _ => ["F", "L", "B", "R"],
    };
    keys.iter().map(|k| cube_corner(k)).collect()
}

const CUBE_EDGES: [(&str, &str); 9] = [
    ("F", "L"),
    ("F", "R"),
    ("F", "F'"),
    ("L", "L'"),
    ("R", "R'"),
    ("F'", "L'"),
    ("F'", "R'"),
    ("L'", "B'"),
    ("R'", "B'"),
];

fn accent_fill(p: &mut Painter, poly: &[(f32, f32)], alpha: u8) {
    p.save();
    p.no_pen();
    p.set_brush(ACCENT.alpha(alpha));
    p.draw_polygon(poly);
    p.restore();
}

fn cube(p: &mut Painter, fills: &[(&str, u8)]) {
    for (face, alpha) in fills {
        accent_fill(p, &cube_face(face), *alpha);
    }
    p.no_brush();
    for (a, b) in CUBE_EDGES {
        let (pa, pb) = (cube_corner(a), cube_corner(b));
        p.draw_line(pa.0, pa.1, pb.0, pb.1);
    }
}

/// The model tree's row icons: a cube, empty for a group, its faces in the
/// accent for a component. They are drawn small (a line of text), so the
/// pen is heavier and the cube is centred on the canvas.
fn tree_row(p: &mut Painter, ink: Rgba, component: bool) {
    p.save();
    p.translate(0.0, -5.0);
    p.set_pen(Pen::round(ink, 4.0));
    let fills: &[(&str, u8)] = if component {
        &[("top", 170), ("front", 255), ("right", 215)]
    } else {
        &[]
    };
    cube(p, fills);
    p.restore();
}

fn view_face(p: &mut Painter, face: &str) {
    let pull = match face {
        "back" => Some((13.0f32, -6.0f32)),
        "left" => Some((-13.0, -6.0)),
        "bottom" => Some((0.0, 12.0)),
        _ => None,
    };
    let Some(pull) = pull else {
        cube(p, &[(face, 160)]);
        return;
    };
    let loose: Vec<(f32, f32)> = cube_face(face)
        .iter()
        .map(|q| (q.0 + pull.0, q.1 + pull.1))
        .collect();
    let corners = ["F", "L", "R", "B", "F'", "L'", "R'", "B'"].map(cube_corner);
    let pts: Vec<(f32, f32)> = corners
        .iter()
        .copied()
        .chain(loose.iter().copied())
        .collect();
    let x0 = pts.iter().map(|q| q.0).fold(f32::MAX, f32::min);
    let x1 = pts.iter().map(|q| q.0).fold(f32::MIN, f32::max);
    let y0 = pts.iter().map(|q| q.1).fold(f32::MAX, f32::min);
    let y1 = pts.iter().map(|q| q.1).fold(f32::MIN, f32::max);
    let k = ((PX - 12.0) / (x1 - x0).max(y1 - y0)).min(1.0);
    p.save();
    p.translate(PX / 2.0, PX / 2.0);
    p.scale(k, k);
    p.translate(-(x0 + x1) / 2.0, -(y0 + y1) / 2.0);
    if let Some(mut pen) = p.pen() {
        pen.width /= k;
        p.set_pen(pen);
    }
    cube(p, &[]);
    accent_fill(p, &loose, 170);
    p.no_brush();
    p.draw_polygon(&loose);
    p.restore();
}

fn text(p: &mut Painter, ink: Rgba) {
    p.save();
    p.set_pen(Pen::new(ink.alpha(150), 1.8).dashed());
    p.no_brush();
    p.draw_rect(9.0, 10.0, 30.0, 28.0);
    p.restore();
    p.save();
    p.set_pen(Pen::round(ink, 3.6));
    p.draw_line(16.0, 17.0, 32.0, 17.0);
    p.draw_line(24.0, 17.0, 24.0, 32.0);
    p.restore();
    dot(p, 9.0, 38.0, 3.0, None);
}

fn text3d(p: &mut Painter, ink: Rgba) {
    p.set_font(30.0, true);
    // the "A" (about 20 wide, 21 high) and its 6-px extrusion, centred
    let base = (11.0f32, 37.0f32);
    p.set_pen(Pen::new(ACCENT, 1.0));
    let (d, n) = (6.0f32, 12);
    for i in (1..=n).rev() {
        let off = d * i as f32 / n as f32;
        p.draw_text(base.0 + off, base.1 - off, "A", false);
    }
    p.set_pen(Pen::new(ink, 1.0));
    p.draw_text(base.0, base.1, "A", false);
}

fn save(p: &mut Painter, _ink: Rgba) {
    p.draw_rect(9.0, 9.0, 30.0, 30.0);
    p.draw_rect(16.0, 9.0, 16.0, 9.0);
    p.draw_rect(15.0, 25.0, 18.0, 14.0);
    p.draw_line(27.0, 11.0, 27.0, 16.0);
}

fn styles_icon(p: &mut Painter, _ink: Rgba) {
    p.save();
    for (dx, dy, fill) in [
        (0.0, 6.0, None),
        (5.0, 3.0, None),
        (10.0, 0.0, Some(ACCENT)),
    ] {
        match fill {
            Some(c) => p.set_brush(c),
            None => p.no_brush(),
        }
        p.draw_rounded_rect(10.0 + dx, 12.0 + dy, 18.0, 24.0, 3.0, 3.0);
    }
    p.restore();
}

fn chevron(p: &mut Painter, color: Rgba, pts: &[[(f32, f32); 3]]) {
    p.save();
    p.set_pen(Pen::round(color, 3.4));
    for c in pts {
        p.draw_line(c[0].0, c[0].1, c[1].0, c[1].1);
        p.draw_line(c[1].0, c[1].1, c[2].0, c[2].1);
    }
    p.restore();
}

// ---- Solids: two overlapping discs --------------------------------------------------

const DA: (f32, f32, f32) = (18.0, 20.0, 12.5);
const DB: (f32, f32, f32) = (30.0, 28.0, 12.5);

fn disc(d: (f32, f32, f32)) -> Path {
    PathBuilder::from_circle(d.0, d.1, d.2)
        .unwrap_or_else(|| PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()))
}

/// A region of the two discs.
#[derive(Clone, Copy)]
enum Region {
    A,
    B,
    Union,
    Inter,
    AminusB,
    BminusA,
}

fn region_mask(p: &Painter, r: Region) -> Option<Mask> {
    let t = p.st.t;
    let mut ma = p.mask()?;
    ma.fill_path(&disc(DA), FillRule::Winding, true, t);
    let mut mb = p.mask()?;
    mb.fill_path(&disc(DB), FillRule::Winding, true, t);
    let (a, b) = (ma.data().to_vec(), mb.data().to_vec());
    let f: fn(u8, u8) -> u8 = match r {
        Region::A => |a, _| a,
        Region::B => |_, b| b,
        Region::Union => |a, b| a.max(b),
        Region::Inter => |a, b| a.min(b),
        Region::AminusB => |a, b| a.min(255 - b),
        Region::BminusA => |a, b| b.min(255 - a),
    };
    for (o, (x, y)) in ma.data_mut().iter_mut().zip(a.iter().zip(b.iter())) {
        *o = f(*x, *y);
    }
    Some(ma)
}

/// The region's outline: each disc's circle where it bounds the region.
fn region_outline(p: &mut Painter, r: Region) {
    let t = p.st.t;
    let (Some(mut in_a), Some(mut in_b)) = (p.mask(), p.mask()) else {
        return;
    };
    // slightly widened so the stroke on the boundary is not cut in half
    in_a.fill_path(&disc((DA.0, DA.1, DA.2 + 0.01)), FillRule::Winding, true, t);
    in_b.fill_path(&disc((DB.0, DB.1, DB.2 + 0.01)), FillRule::Winding, true, t);
    let mut out_a = in_a.clone();
    out_a.invert();
    let mut out_b = in_b.clone();
    out_b.invert();
    // (circle, where it is part of the outline)
    let parts: Vec<((f32, f32, f32), Option<Mask>)> = match r {
        Region::A => vec![(DA, None)],
        Region::B => vec![(DB, None)],
        Region::Union => vec![(DA, Some(out_b)), (DB, Some(out_a))],
        Region::Inter => vec![(DA, Some(in_b)), (DB, Some(in_a))],
        Region::AminusB => vec![(DA, Some(out_b)), (DB, Some(in_a))],
        Region::BminusA => vec![(DB, Some(out_a)), (DA, Some(in_b))],
    };
    for (d, m) in parts {
        p.stroke_only(&disc(d), m.as_ref());
    }
}

fn solid_keep(p: &mut Painter, ink: Rgba, r: Region, alpha: u8) {
    p.save();
    if let Some(m) = region_mask(p, r) {
        let full = PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, PX, PX).unwrap());
        let mut paint = Painter::paint(ACCENT.alpha(alpha));
        paint.anti_alias = true;
        let t = p.st.t;
        p.pixmap
            .fill_path(&full, &paint, FillRule::Winding, t, Some(&m));
    }
    p.set_pen(Pen::new(ink, 2.6).join(LineJoin::Round));
    region_outline(p, r);
    p.restore();
}

fn solid_gone(p: &mut Painter, ink: Rgba, r: Region) {
    p.save();
    p.set_pen(Pen::new(ink.alpha(140), 1.8).dashed().cap(LineCap::Butt));
    region_outline(p, r);
    p.restore();
}

fn solid(p: &mut Painter, ink: Rgba, key: &str) -> bool {
    match key {
        "merge_solids" => {
            solid_keep(p, ink, Region::Union, 70);
            p.save();
            p.set_pen(Pen::new(ink, 3.6));
            region_outline(p, Region::Union);
            p.restore();
        }
        "solid_union" => {
            solid_keep(p, ink, Region::Union, 150);
            solid_gone(p, ink, Region::Inter);
        }
        "solid_subtract" => {
            solid_gone(p, ink, Region::A);
            solid_keep(p, ink, Region::BminusA, 150);
        }
        "solid_trim" => {
            solid_keep(p, ink, Region::A, 90);
            solid_keep(p, ink, Region::BminusA, 150);
        }
        "solid_intersect" => {
            solid_gone(p, ink, Region::A);
            solid_gone(p, ink, Region::B);
            solid_keep(p, ink, Region::Inter, 150);
        }
        "solid_split" => {
            solid_keep(p, ink, Region::AminusB, 60);
            solid_keep(p, ink, Region::BminusA, 60);
            solid_keep(p, ink, Region::Inter, 200);
        }
        _ => return false,
    }
    true
}

/// Draw `key`; `false` for an unknown key.
/// Every key [`draw`] knows.
pub const KEYS: &[&str] = &[
    "select",
    "save",
    "line",
    "freehand",
    "rectangle",
    "rectangle_center",
    "rotated_rect",
    "circle",
    "polygon",
    "slot",
    "arc",
    "arc3",
    "center_arc",
    "sector",
    "rotate",
    "scale",
    "flip",
    "pattern_linear",
    "pattern_circular",
    "sweep",
    "revolve",
    "pushpull",
    "hole",
    "offset",
    "fillet",
    "chamfer",
    "shell",
    "move",
    "paint",
    "eyedropper",
    "dimension",
    "dimension_style",
    "orbit",
    "pan",
    "undo",
    "redo",
    "text",
    "text3d",
    "eraser",
    "measure",
    "protractor",
    "section",
    "styles",
    "section_planes",
    "section_cuts",
    "section_fill",
    "zoom",
    "zoom_window",
    "zoom_extents",
    "zoom_selection",
    "projection",
    "model_info",
    "solid_inspector",
    "view_iso",
    "view_top",
    "view_bottom",
    "view_front",
    "view_back",
    "view_right",
    "view_left",
    "side_collapse",
    "side_expand",
    "overflow_h",
    "overflow_v",
    "tree_group",
    "tree_component",
];

pub fn draw(p: &mut Painter, key: &str, ink: Rgba) -> bool {
    let f: fn(&mut Painter, Rgba) = match key {
        "select" => select,
        "save" => save,
        "line" => line,
        "freehand" => freehand,
        "rectangle" => rectangle,
        "rectangle_center" => rectangle_center,
        "rotated_rect" => rotated_rect,
        "circle" => circle,
        "polygon" => polygon,
        "slot" => slot,
        "arc" => arc,
        "arc3" => arc3,
        "center_arc" => center_arc,
        "sector" => sector,
        "rotate" => rotate,
        "scale" => scale,
        "flip" => flip,
        "pattern_linear" => pattern_linear,
        "pattern_circular" => pattern_circular,
        "sweep" => sweep,
        "revolve" => revolve,
        "pushpull" => pushpull,
        "offset" => offset,
        "fillet" => fillet,
        "chamfer" => chamfer,
        "shell" => shell_icon,
        "hole" => hole,
        "move" => move_icon,
        "paint" => paint,
        "eyedropper" => eyedropper,
        "dimension" => dimension,
        "dimension_style" => dimension_style,
        "orbit" => orbit,
        "pan" => pan,
        "undo" => undo,
        "redo" => redo,
        "text" => text,
        "text3d" => text3d,
        "eraser" => eraser,
        "measure" => measure,
        "protractor" => protractor,
        "section" => section,
        "styles" => styles_icon,
        "section_planes" => section_planes,
        "section_cuts" => section_cuts,
        "section_fill" => section_fill,
        "zoom" => zoom,
        "zoom_window" => zoom_window,
        "zoom_extents" => zoom_extents,
        "zoom_selection" => zoom_selection,
        "projection" => projection,
        "model_info" => model_info,
        "solid_inspector" => solid_inspector,
        "view_iso" => |p, _| cube(p, &[("top", 60), ("front", 150), ("right", 100)]),
        "view_top" => |p, _| view_face(p, "top"),
        "view_bottom" => |p, _| view_face(p, "bottom"),
        "view_front" => |p, _| view_face(p, "front"),
        "view_back" => |p, _| view_face(p, "back"),
        "view_right" => |p, _| view_face(p, "right"),
        "view_left" => |p, _| view_face(p, "left"),
        "side_collapse" => |p, ink| chevron(p, ink, &[[(19.0, 13.0), (30.0, 24.0), (19.0, 35.0)]]),
        "side_expand" => |p, ink| chevron(p, ink, &[[(29.0, 13.0), (18.0, 24.0), (29.0, 35.0)]]),
        "overflow_h" => |p, _| {
            chevron(
                p,
                ACCENT,
                &[
                    [(10.0, 14.0), (20.0, 24.0), (10.0, 34.0)],
                    [(24.0, 14.0), (34.0, 24.0), (24.0, 34.0)],
                ],
            )
        },
        "overflow_v" => |p, _| {
            chevron(
                p,
                ACCENT,
                &[
                    [(14.0, 10.0), (24.0, 20.0), (34.0, 10.0)],
                    [(14.0, 24.0), (24.0, 34.0), (34.0, 24.0)],
                ],
            )
        },
        "tree_group" => |p, ink| tree_row(p, ink, false),
        "tree_component" => |p, ink| tree_row(p, ink, true),
        k => return solid(p, ink, k),
    };
    f(p, ink);
    true
}

/// An icon rendered at `scale` × 48 px.
pub struct Rendered {
    pub size: u32,
    /// Premultiplied RGBA.
    pub rgba: Vec<u8>,
    pub texts: Vec<IconText>,
}

/// `tool_icon(key)` as pixels.
pub fn render(key: &str, scale: f32, ink: Rgba) -> Option<Rendered> {
    let mut p = Painter::canvas(scale, ink)?;
    if !draw(&mut p, key, ink) {
        return None;
    }
    Some(Rendered {
        size: p.pixmap.width(),
        rgba: p.pixmap.data().to_vec(),
        texts: p.texts,
    })
}

// ---- tool cursors -------------------------------------------------------------------

/// The pencil-cursor tools and their badge.
fn pencil_badge(key: &str) -> Option<Option<&'static str>> {
    Some(match key {
        "line" => None,
        "freehand" => Some("freehand"),
        "rectangle" => Some("rectangle"),
        "rectangle_center" => Some("rectangle_center"),
        "rotated_rect" => Some("rotated_rect"),
        "circle" => Some("circle"),
        "polygon" => Some("polygon"),
        "slot" => Some("slot"),
        "arc" => Some("arc"),
        "arc3" => Some("arc3"),
        "center_arc" => Some("center_arc"),
        "sector" => Some("sector"),
        _ => return None,
    })
}

/// A cursor's hotspot, in 48-space.
fn hotspot(key: &str) -> Option<(f32, f32)> {
    Some(match key {
        "move" | "rotate" | "scale" | "flip" | "pushpull" | "offset" | "sweep" | "fillet"
        | "chamfer" | "shell" | "hole" | "text" | "text3d" | "protractor" | "orbit" | "pan"
        | "pattern_linear" | "pattern_circular" | "revolve" => (24.0, 24.0),
        "dimension" => (12.0, 30.0),
        "paint" => (13.0, 35.0),
        "eyedropper" => (9.5, 38.5),
        "eraser" => (15.0, 31.0),
        "measure" => (10.0, 28.0),
        "zoom" => (21.0, 21.0),
        "zoom_window" => (21.0, 22.0),
        "section" => (24.0, 27.0),
        _ => return None,
    })
}

fn pencil(p: &mut Painter, ink: Rgba) {
    p.save();
    p.translate(6.0, 42.0);
    p.rotate(-45.0);
    p.set_pen(Pen::round(ink, 3.0));
    p.no_brush();
    p.draw_polygon(&[(0.0, 0.0), (10.0, -5.5), (10.0, 5.5)]);
    p.draw_line(10.0, -5.5, 40.0, -5.5);
    p.draw_line(10.0, 5.5, 40.0, 5.5);
    p.draw_line(40.0, -5.5, 40.0, 5.5);
    p.no_pen();
    p.set_brush(ACCENT);
    p.draw_polygon(&[(-0.6, 0.0), (4.2, -2.4), (4.2, 2.4)]);
    p.restore();
}

/// The Solids tools' pointer (`solid_cursor`): an arrow with a grey «×»
/// badge over anything that is not a solid, and a «1» or «2» on an accent
/// disc over a solid, for the pick it will be. Drawn in 32-px space, the
/// hotspot at the arrow's tip.
pub fn solid_cursor(state: &str, dpr: f32) -> Option<(Rendered, (f32, f32))> {
    let white = Rgba(255, 255, 255, 255);
    let mut p = Painter::canvas(CURSOR_SIZE / PX * dpr, white)?;
    p.scale(PX / CURSOR_SIZE, PX / CURSOR_SIZE);
    let mut arrow = IconPath::new();
    arrow
        .move_to(1.0, 1.0)
        .line_to(1.0, 17.0)
        .line_to(5.0, 13.0)
        .line_to(8.0, 20.0)
        .line_to(10.5, 19.0)
        .line_to(7.5, 12.0)
        .line_to(13.0, 12.0)
        .close();
    p.set_pen(Pen::new(white, 1.2));
    p.set_brush(Rgba(0, 0, 0, 255));
    p.draw_path(&arrow);
    if state == "no" {
        p.set_pen(Pen::new(white, 1.2));
        p.set_brush(Rgba(90, 94, 102, 255));
        p.draw_rounded_rect(15.0, 15.0, 14.0, 14.0, 3.0, 3.0);
        p.set_pen(Pen::new(white, 2.2));
        p.draw_line(18.5, 18.5, 25.5, 25.5);
        p.draw_line(25.5, 18.5, 18.5, 25.5);
    } else {
        p.set_pen(Pen::new(white, 1.2));
        p.set_brush(ACCENT);
        p.draw_ellipse(22.0, 22.0, 7.5, 7.5);
        p.set_pen(Pen::new(white, 1.0));
        p.set_font(11.0, true);
        p.draw_text(22.0, 22.0, state, true);
    }
    let texts = std::mem::take(&mut p.texts);
    bake_texts(&mut p.pixmap, &texts);
    let side = p.pixmap.width();
    Some((
        Rendered {
            size: side,
            rgba: p.pixmap.data().to_vec(),
            texts: Vec::new(),
        },
        (1.0, 1.0),
    ))
}

/// The cursor canvas.
pub const CURSOR_SIZE: f32 = 32.0;

/// Whether `key` has a pointer bitmap (`cursor` gives `Some`).
pub fn has_cursor(key: &str) -> bool {
    if key == "select" {
        return false;
    }
    matches!(key, "solid_no" | "solid_1" | "solid_2")
        || pencil_badge(key).is_some()
        || hotspot(key).is_some()
}

/// `tool_cursor(key, plus)`: the pixels (with the halo) and the hotspot,
/// or `None` to keep the standard arrow.
pub fn cursor(key: &str, plus: bool, ink: Rgba, dpr: f32) -> Option<(Rendered, (f32, f32))> {
    if key == "select" {
        return None;
    }
    if let Some(state) = key.strip_prefix("solid_") {
        if matches!(state, "no" | "1" | "2") {
            return solid_cursor(state, dpr);
        }
    }
    let badge = pencil_badge(key);
    let hot48 = if badge.is_some() {
        Some((6.0, 42.0))
    } else {
        hotspot(key)
    }?;
    let scale = CURSOR_SIZE / PX * dpr;
    let mut p = Painter::canvas(scale, ink)?;
    match badge {
        Some(b) => {
            pencil(&mut p, ink);
            if let Some(b) = b {
                p.save();
                p.translate(28.0, 28.0);
                p.scale(20.0 / PX, 20.0 / PX);
                p.set_pen(Pen::round(ink, 4.4));
                draw(&mut p, b, ink);
                p.restore();
            }
        }
        None => {
            if !draw(&mut p, key, ink) {
                return None;
            }
        }
    }
    if plus {
        p.set_pen(Pen::new(ink, 4.0).cap(LineCap::Round));
        p.draw_line(34.0, 6.0, 44.0, 6.0);
        p.draw_line(39.0, 1.0, 39.0, 11.0);
    }
    // the halo: the art's silhouette in the contrasting tone, ±1 px
    let lightness = (ink.0 as u32 * 299 + ink.1 as u32 * 587 + ink.2 as u32 * 114) / 1000;
    let halo = if lightness < 128 {
        Rgba(255, 255, 255, 225)
    } else {
        Rgba(28, 32, 38, 225)
    };
    let side = p.pixmap.width() as i32;
    let art = p.pixmap.data().to_vec();
    let mut out = vec![0u8; art.len()];
    let px = |x: i32, y: i32| -> usize { ((y * side + x) * 4) as usize };
    let blend = |dst: &mut [u8], src: [f32; 4]| {
        let a = src[3];
        for i in 0..4 {
            dst[i] = (src[i] + dst[i] as f32 * (1.0 - a / 255.0))
                .round()
                .min(255.0) as u8;
        }
    };
    for y in 0..side {
        for x in 0..side {
            let mut a: u8 = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (sx, sy) = (x - dx, y - dy);
                    if sx >= 0 && sy >= 0 && sx < side && sy < side {
                        a = a.max(art[px(sx, sy) + 3]);
                    }
                }
            }
            if a > 0 {
                let k = a as f32 / 255.0 * halo.3 as f32 / 255.0;
                let s = [
                    halo.0 as f32 * k,
                    halo.1 as f32 * k,
                    halo.2 as f32 * k,
                    255.0 * k,
                ];
                blend(&mut out[px(x, y)..px(x, y) + 4], s);
            }
        }
    }
    for i in (0..art.len()).step_by(4) {
        let s = [
            art[i] as f32,
            art[i + 1] as f32,
            art[i + 2] as f32,
            art[i + 3] as f32,
        ];
        blend(&mut out[i..i + 4], s);
    }
    if let Some(mut baked) = tiny_skia::IntSize::from_wh(side as u32, side as u32)
        .and_then(|size| Pixmap::from_vec(out.clone(), size))
    {
        bake_texts(&mut baked, &p.texts);
        out = baked.take();
    }
    let hs = CURSOR_SIZE / PX;
    let hot = (
        (hot48.0 * hs).round().clamp(0.0, CURSOR_SIZE - 1.0),
        (hot48.1 * hs).round().clamp(0.0, CURSOR_SIZE - 1.0),
    );
    Some((
        Rendered {
            size: side as u32,
            rgba: out,
            texts: Vec::new(),
        },
        hot,
    ))
}

/// Draw `texts` into `pixmap` (a 48-space canvas at any scale) as glyph
/// outlines of the sans-serif system font, the one 3D text uses. A pixel
/// size is the font's em, the way the icons were designed. Without a
/// font the text is left out.
fn bake_texts(pixmap: &mut Pixmap, texts: &[IconText]) {
    use xtrude_core::text3d;
    let k = pixmap.width() as f32 / PX;
    for t in texts {
        let Some((ascent, _, height)) = text3d::vertical_metrics("sans-serif", t.bold, false)
        else {
            continue;
        };
        let f = t.px / text3d::FONT_PX as f32;
        let (x0, baseline) = if t.centered {
            let advance = text3d::horizontal_advance(&t.text, "sans-serif", t.bold, false);
            (
                t.at.0 - advance as f32 * f / 2.0,
                t.at.1 + (ascent - height / 2.0) as f32 * f,
            )
        } else {
            t.at
        };
        let mut b = PathBuilder::new();
        for ring in text3d::rings(&t.text, "sans-serif", t.bold, false, 0.0) {
            let pt =
                |&(x, y): &(f64, f64)| ((x0 + x as f32 * f) * k, (baseline - y as f32 * f) * k);
            let mut pts = ring.iter().map(pt);
            if let Some((x, y)) = pts.next() {
                b.move_to(x, y);
                pts.for_each(|(x, y)| b.line_to(x, y));
                b.close();
            }
        }
        if let Some(path) = b.finish() {
            let mut paint = Paint::default();
            paint.set_color(Color::from_rgba8(
                t.color.0, t.color.1, t.color.2, t.color.3,
            ));
            paint.anti_alias = true;
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

/// A pointer as the OS cursor takes it: straight-alpha RGBA, size and
/// hotspot in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct OsCursor {
    pub rgba: Vec<u8>,
    pub size: [u16; 2],
    pub hotspot: [u16; 2],
}

/// `cursor(key, plus, ink, dpr)` for the OS: the bitmap is already
/// `CURSOR_SIZE × dpr` pixels, and the hotspot (in points) scales the same
/// way. `None` keeps the standard arrow.
pub fn os_cursor(key: &str, plus: bool, ink: Rgba, dpr: f32) -> Option<OsCursor> {
    let (img, hot) = cursor(key, plus, ink, dpr)?;
    let side = u16::try_from(img.size).ok()?;
    let at = |v: f32| (v * dpr).round().clamp(0.0, f32::from(side) - 1.0) as u16;
    Some(OsCursor {
        rgba: unpremultiply(&img.rgba),
        size: [side, side],
        hotspot: [at(hot.0), at(hot.1)],
    })
}

/// Premultiplied RGBA to straight RGBA (fully transparent pixels become
/// zero).
pub fn unpremultiply(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|px| {
            let a = px[3];
            let c = |v: u8| {
                if a == 0 {
                    0
                } else {
                    (f32::from(v) * 255.0 / f32::from(a)).round().min(255.0) as u8
                }
            };
            [c(px[0]), c(px[1]), c(px[2]), a]
        })
        .collect()
}
