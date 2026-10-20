// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3D Text — real, editable geometry generated from font glyphs.
//!
//! The system fonts (`fontdb`) supply the glyphs, shaped with `harfrust`
//! (kerning included); `skrifa` reads their outlines and metrics, and the
//! outlines are flattened into polygons by
//! recursive subdivision; each outline ring is classified
//! outer/hole by containment parity, becomes a front face (with holes) and,
//! when a thickness is given, a back face plus side walls — one closed
//! solid per contour group.
//!
//! The text STANDS UP: width along +X, height along +Z (base at z=0),
//! thickness along +Y.

use std::sync::OnceLock;

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use glam::DVec3;
use harfrust::{ShapeOptions, ShaperData, UnicodeBuffer};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlineGlyphCollection, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::mesh::Mesh;

/// Curve flattening happens at font-point scale; 100 pt keeps letter curves
/// smooth without exploding the vertex count.
const FONT_PT: f64 = 100.0;
/// 100 pt at the usual 96 dpi screen resolution.
pub const FONT_PX: f64 = FONT_PT * 96.0 / 72.0;
/// Flattening threshold (pixels): half a pixel, finer than the eye sees.
const FLATTEN: f64 = 0.5;

/// A ring of 2D points in font pixels, y up.
pub type Ring = Vec<(f64, f64)>;

fn database() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        let pick = |db: &Database, names: &[&str]| -> Option<String> {
            names
                .iter()
                .find(|n| {
                    db.faces()
                        .any(|f| f.families.iter().any(|(fam, _)| fam == *n))
                })
                .map(|n| n.to_string())
        };
        if let Some(n) = pick(
            &db,
            &[
                "Noto Sans",
                "DejaVu Sans",
                "Liberation Sans",
                "Cantarell",
                "Arial",
                "Helvetica",
            ],
        ) {
            db.set_sans_serif_family(n);
        }
        if let Some(n) = pick(
            &db,
            &[
                "Noto Serif",
                "DejaVu Serif",
                "Liberation Serif",
                "Times New Roman",
            ],
        ) {
            db.set_serif_family(n);
        }
        if let Some(n) = pick(
            &db,
            &[
                "Noto Sans Mono",
                "DejaVu Sans Mono",
                "Liberation Mono",
                "Courier New",
            ],
        ) {
            db.set_monospace_family(n);
        }
        db
    })
}

fn family_of(name: &str) -> Family<'_> {
    match name.trim().to_ascii_lowercase().as_str() {
        "" | "sans" | "sans serif" | "sans-serif" => Family::SansSerif,
        "serif" => Family::Serif,
        "mono" | "monospace" => Family::Monospace,
        _ => Family::Name(name),
    }
}

/// The face for `family` (falling back to the sans-serif default, then any
/// face at all, so some text always comes out).
fn face_id(family: &str, bold: bool, italic: bool) -> Option<fontdb::ID> {
    let db = database();
    let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
    let style = if italic { Style::Italic } else { Style::Normal };
    let q = |fam: Family<'_>| {
        let families = [fam];
        db.query(&Query {
            families: &families,
            weight,
            stretch: Stretch::Normal,
            style,
        })
    };
    q(family_of(family))
        .or_else(|| q(Family::SansSerif))
        .or_else(|| db.faces().next().map(|f| f.id))
}

struct Flattener {
    rings: Vec<Ring>,
    cur: Ring,
    scale: f64,
    x0: f64,
}

impl Flattener {
    fn pt(&self, x: f32, y: f32) -> (f64, f64) {
        (self.x0 + x as f64 * self.scale, y as f64 * self.scale)
    }

    fn flush(&mut self) {
        let mut ring = std::mem::take(&mut self.cur);
        if ring.len() >= 3 {
            if ring.first() == ring.last() {
                ring.pop();
            }
            if ring.len() >= 3 {
                self.rings.push(ring);
            }
        }
    }

    /// Stack-based subdivision until the control polygon is flat within
    /// the threshold (at most 9 levels).
    fn cubic(&mut self, p: [(f64, f64); 4]) {
        let mut stack: Vec<([(f64, f64); 4], i32)> = vec![(p, 9)];
        while let Some((b, level)) = stack.pop() {
            let (x1, y1) = b[0];
            let (x2, y2) = b[1];
            let (x3, y3) = b[2];
            let (x4, y4) = b[3];
            let (y4y1, x4x1) = (y4 - y1, x4 - x1);
            let mut l = x4x1.abs() + y4y1.abs();
            let d = if l > 1.0 {
                (x4x1 * (y1 - y2) - y4y1 * (x1 - x2)).abs()
                    + (x4x1 * (y1 - y3) - y4y1 * (x1 - x3)).abs()
            } else {
                l = 1.0;
                (x1 - x2).abs() + (y1 - y2).abs() + (x1 - x3).abs() + (y1 - y3).abs()
            };
            if d < FLATTEN * l || level == 0 {
                self.cur.push((x4, y4));
            } else {
                let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
                let (p01, p12, p23) = (mid(b[0], b[1]), mid(b[1], b[2]), mid(b[2], b[3]));
                let (p012, p123) = (mid(p01, p12), mid(p12, p23));
                let m = mid(p012, p123);
                // the first half is processed first: push the second below
                stack.push(([m, p123, p23, b[3]], level - 1));
                stack.push(([b[0], p01, p012, m], level - 1));
            }
        }
    }
}

impl OutlinePen for Flattener {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        let p = self.pt(x, y);
        self.cur.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.cur.push(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // quadratics become cubics (degree elevation), one flattener for both
        let p0 = *self.cur.last().unwrap_or(&(0.0, 0.0));
        let q = self.pt(x1, y1);
        let p = self.pt(x, y);
        let c1 = (
            p0.0 + 2.0 / 3.0 * (q.0 - p0.0),
            p0.1 + 2.0 / 3.0 * (q.1 - p0.1),
        );
        let c2 = (p.0 + 2.0 / 3.0 * (q.0 - p.0), p.1 + 2.0 / 3.0 * (q.1 - p.1));
        self.cubic([p0, c1, c2, p]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = *self.cur.last().unwrap_or(&(0.0, 0.0));
        let (c1, c2, p) = (self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        self.cubic([p0, c1, c2, p]);
    }
    fn close(&mut self) {
        if let (Some(&first), Some(&last)) = (self.cur.first(), self.cur.last()) {
            if first != last {
                self.cur.push(first);
            }
        }
        self.flush();
    }
}

/// The font's metrics in font units (the default instance of a variable
/// font).
fn unscaled_metrics(font: &FontRef<'_>) -> skrifa::metrics::Metrics {
    font.metrics(Size::unscaled(), LocationRef::default())
}

/// Shape `text` and hand each glyph `(the font's outlines, glyph id, pen x
/// in pixels, pixels per font unit)` to `f`; returns the total advance in
/// pixels.
fn shape<F: FnMut(&OutlineGlyphCollection<'_>, GlyphId, f64, f64)>(
    family: &str,
    bold: bool,
    italic: bool,
    text: &str,
    mut f: F,
) -> Option<f64> {
    let id = face_id(family, bold, italic)?;
    database()
        .with_face_data(id, |data, index| {
            let font = FontRef::from_index(data, index).ok()?;
            let scale = FONT_PX / unscaled_metrics(&font).units_per_em as f64;
            let outlines = font.outline_glyphs();
            let shaper_data = ShaperData::new(&font);
            let shaper = shaper_data.shaper(&font).build();
            let mut buf = UnicodeBuffer::new();
            buf.push_str(text);
            buf.guess_segment_properties();
            let out = shaper.shape(buf, ShapeOptions::new());
            let mut pen = 0.0;
            for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
                let x = pen + pos.x_offset as f64 * scale;
                f(&outlines, GlyphId::new(info.glyph_id), x, scale);
                pen += pos.x_advance as f64 * scale;
            }
            Some(pen)
        })
        .flatten()
}

/// Draw glyph `gid` into `pen` in font units (the default instance of a
/// variable font); false when the font has no outline for it.
fn draw_glyph(
    outlines: &OutlineGlyphCollection<'_>,
    gid: GlyphId,
    pen: &mut impl OutlinePen,
) -> bool {
    let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
    outlines
        .get(gid)
        .is_some_and(|glyph| glyph.draw(settings, pen).is_ok())
}

/// Glyph outline rings in font pixels (y up). `x` is where the text
/// starts — a single letter of a longer string is drawn at its advance.
pub fn rings(text: &str, font_family: &str, bold: bool, italic: bool, x: f64) -> Vec<Ring> {
    let mut fl = Flattener {
        rings: Vec::new(),
        cur: Vec::new(),
        scale: 1.0,
        x0: 0.0,
    };
    shape(
        font_family,
        bold,
        italic,
        text,
        |glyphs, gid, pen, scale| {
            fl.scale = scale;
            fl.x0 = x + pen;
            if draw_glyph(glyphs, gid, &mut fl) {
                fl.flush();
            }
            fl.cur.clear();
        },
    );
    fl.rings
}

/// `(ascent, descent, height)` of a font in font pixels (height =
/// ascent + descent), `None` without the font.
pub fn vertical_metrics(font_family: &str, bold: bool, italic: bool) -> Option<(f64, f64, f64)> {
    let id = face_id(font_family, bold, italic)?;
    database()
        .with_face_data(id, |data, index| {
            let metrics = unscaled_metrics(&FontRef::from_index(data, index).ok()?);
            let scale = FONT_PX / metrics.units_per_em as f64;
            let ascent = metrics.ascent as f64 * scale;
            let descent = -(metrics.descent as f64) * scale;
            Some((ascent, descent, ascent + descent))
        })
        .flatten()
}

/// The advance width of `text` in font pixels, kerning included.
pub fn horizontal_advance(text: &str, font_family: &str, bold: bool, italic: bool) -> f64 {
    shape(font_family, bold, italic, text, |_, _, _, _| {}).unwrap_or(0.0)
}

fn ring_area(ring: &[(f64, f64)]) -> f64 {
    let n = ring.len();
    let mut s = 0.0;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        s += x0 * y1 - x1 * y0;
    }
    s / 2.0
}

fn point_in_ring(pt: (f64, f64), ring: &[(f64, f64)]) -> bool {
    let (x, y) = pt;
    let mut inside = false;
    let n = ring.len();
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        if (y0 > y) != (y1 > y) {
            let t = (y - y0) / (y1 - y0);
            if x < x0 + t * (x1 - x0) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Pair every ring with its role by containment parity: even depth = outer
/// contour, odd = hole of its immediate container.
fn group_rings(rings: &[Ring]) -> Vec<(Ring, Vec<Ring>)> {
    let depth: Vec<usize> = rings
        .iter()
        .enumerate()
        .map(|(i, r)| {
            rings
                .iter()
                .enumerate()
                .filter(|(j, o)| *j != i && point_in_ring(r[0], o))
                .count()
        })
        .collect();
    let outers: Vec<usize> = (0..rings.len()).filter(|&i| depth[i] % 2 == 0).collect();
    let mut holes: Vec<Vec<usize>> = vec![Vec::new(); outers.len()];
    for (i, &d) in depth.iter().enumerate() {
        if d % 2 == 0 {
            continue;
        }
        if let Some(k) = outers
            .iter()
            .position(|&j| depth[j] + 1 == d && point_in_ring(rings[i][0], &rings[j]))
        {
            holes[k].push(i);
        }
    }
    outers
        .iter()
        .zip(holes)
        .map(|(&i, hs)| {
            (
                rings[i].clone(),
                hs.iter().map(|&h| rings[h].clone()).collect(),
            )
        })
        .collect()
}

/// The extruded solid of `rings` (font pixels) at `scale` metres per unit,
/// its lowest point at `y_min` sitting on z=0.
fn solid_from_rings(rings: &[Ring], scale: f64, y_min: f64, thickness: f64) -> Mesh {
    let mut mesh = Mesh::new();
    let v = |x: f64, y: f64, side_y: f64| DVec3::new(x * scale, side_y, (y - y_min) * scale);
    for (mut outer, holes) in group_rings(rings) {
        // Windings set ANALYTICALLY: an outer ring CCW in (x, up) maps to a
        // front face whose normal is exactly -Y; walls and back follow.
        if ring_area(&outer) < 0.0 {
            outer.reverse();
        }
        let holes: Vec<Ring> = holes
            .into_iter()
            .map(|mut h| {
                if ring_area(&h) >= 0.0 {
                    h.reverse();
                }
                h
            })
            .collect();
        let front: Vec<DVec3> = outer.iter().map(|&(x, y)| v(x, y, 0.0)).collect();
        let front_holes: Vec<Vec<DVec3>> = holes
            .iter()
            .map(|h| h.iter().map(|&(x, y)| v(x, y, 0.0)).collect())
            .collect();
        mesh.add_face(&front, &front_holes);
        if thickness <= 1e-9 {
            continue;
        }
        let back: Vec<DVec3> = outer
            .iter()
            .rev()
            .map(|&(x, y)| v(x, y, thickness))
            .collect();
        let back_holes: Vec<Vec<DVec3>> = holes
            .iter()
            .map(|h| h.iter().rev().map(|&(x, y)| v(x, y, thickness)).collect())
            .collect();
        mesh.add_face(&back, &back_holes);
        for ring in std::iter::once(&outer).chain(holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                let (x0, y0) = ring[i];
                let (x1, y1) = ring[(i + 1) % n];
                mesh.add_face(
                    &[
                        v(x0, y0, 0.0),
                        v(x0, y0, thickness),
                        v(x1, y1, thickness),
                        v(x1, y1, 0.0),
                    ],
                    &[],
                );
            }
        }
    }
    // Soften wall-to-wall seams at a shallow dihedral so the flattened
    // curves read smooth; the front/back outlines (~90°) stay visible.
    for e in mesh.edge_ids() {
        let fs = mesh.edge(e).faces.clone();
        if fs.len() != 2 {
            continue;
        }
        let (n1, n2) = (mesh.face_normal(fs[0]), mesh.face_normal(fs[1]));
        if n1.y.abs() < 0.5 && n2.y.abs() < 0.5 && n1.dot(n2) > 0.85 {
            mesh.edge_mut(e).soft = true;
        }
    }
    mesh
}

/// `(scale, y_min)` for the whole block: `height` is the REAL height of the
/// block and its lowest point sits at z=0.
fn block_scale(rings: &[Ring], height: f64) -> (f64, f64) {
    let (lo, hi) = rings
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &(_, y)| {
            (lo.min(y), hi.max(y))
        });
    (height / (hi - lo).max(1e-9), lo)
}

/// The 3D-text mesh in ONE piece: `height` is the letter height in metres,
/// `thickness` the extrusion depth (0 → flat faces only).
pub fn build_text_mesh(
    text: &str,
    font_family: &str,
    bold: bool,
    italic: bool,
    height: f64,
    thickness: f64,
) -> Mesh {
    let rs = rings(text, font_family, bold, italic, 0.0);
    if rs.is_empty() {
        return Mesh::new();
    }
    let (scale, y_min) = block_scale(&rs, height);
    solid_from_rings(&rs, scale, y_min, thickness)
}

/// The same text as ONE MESH PER LETTER, `(character, mesh)` in reading
/// order, spaces and empty glyphs left out. Every letter shares the block's
/// scale and baseline; each glyph is drawn at the advance of the text
/// before it (kerning honoured).
pub fn build_text_letters(
    text: &str,
    font_family: &str,
    bold: bool,
    italic: bool,
    height: f64,
    thickness: f64,
) -> Vec<(char, Mesh)> {
    let rs = rings(text, font_family, bold, italic, 0.0);
    if rs.is_empty() {
        return Vec::new();
    }
    let (scale, y_min) = block_scale(&rs, height);
    let mut letters = Vec::new();
    for (i, (byte, ch)) in text.char_indices().enumerate() {
        if ch.is_whitespace() {
            continue;
        }
        let x = if i > 0 {
            horizontal_advance(&text[..byte], font_family, bold, italic)
        } else {
            0.0
        };
        let glyph = rings(&ch.to_string(), font_family, bold, italic, x);
        if glyph.is_empty() {
            continue;
        }
        let mesh = solid_from_rings(&glyph, scale, y_min, thickness);
        if !mesh.faces.is_empty() {
            letters.push((ch, mesh));
        }
    }
    letters
}
