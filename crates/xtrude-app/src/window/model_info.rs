// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window ▸ Model Info: model statistics in a dialog. The statistics, the
//! copy-as-text summary and the rows of the dialog's tables live here; the
//! shell draws them.
//!
//! The numbers agree with the rest of the application because they come
//! from the same sources:
//!
//! - geometry reads the loose mesh (NOT the active mesh, which is the open
//!   group's while a group is being edited: reading it would double-count);
//! - colours mirror the Paint tray: the face's colour (floats 0–1), named
//!   by its registry name when it has one;
//! - lengths are formatted by the dimension style (unit and precision), so
//!   the dialog speaks the same units as the rest of the document.

use indexmap::IndexMap;
use xtrude_core::attrs::Rgb;
use xtrude_core::math::MatExt;
use xtrude_core::mesh::Mesh;
use xtrude_model::dimension::DimensionStyle;
use xtrude_model::group::iter_placements;
use xtrude_model::i18n::tr;
use xtrude_model::scene::Scene;
use xtrude_model::units::fmt_area;

use crate::viewport::format_dim_value;

/// The menu entry.
pub const NAME: &str = "Model Info";
/// Its status tip.
pub const DESCRIPTION: &str = "Show the model's statistics: faces, edges, groups, colors and more.";
/// How long the title says "copied!".
pub const COPIED_MS: u64 = 1500;

/// The world-space bounding box of everything placed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BBox {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
    pub min_z: f64,
    pub max_z: f64,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
}

/// One paint in use: a colour, and the registry name it goes by.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorStat {
    pub rgb: Rgb,
    pub mat: Option<String>,
    pub faces: usize,
    pub area: f64,
}

impl ColorStat {
    /// The registry name, or `rgb(r,g,b)` for an anonymous colour.
    pub fn label(&self) -> String {
        match &self.mat {
            Some(m) if !m.is_empty() => m.clone(),
            _ => {
                let [r, g, b] = self.rgb.map(byte);
                format!("rgb({r},{g},{b})")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerStat {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

/// The statistics of a document: raw numbers; formatting
/// happens at display time with the dimension style.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelStats {
    pub loose_verts: usize,
    pub loose_edges: usize,
    pub loose_faces: usize,
    pub group_count: usize,
    pub instances: usize,
    pub classic_groups: usize,
    pub group_verts: usize,
    pub group_edges: usize,
    pub group_faces: usize,
    pub total_verts: usize,
    pub total_edges: usize,
    pub total_faces: usize,
    pub tri_count: usize,
    pub bbox: BBox,
    pub colors: Vec<ColorStat>,
    pub layers: Vec<LayerStat>,
    pub dims_count: usize,
    pub text_count: usize,
    pub guides_count: usize,
    pub saved_views_count: usize,
    pub style: DimensionStyle,
}

/// A colour channel 0–1 as a byte, rounded half to even.
fn byte(c: f64) -> i64 {
    (c * 255.0).round_ties_even() as i64
}

/// `n` with comma thousands separators.
pub fn fmt_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn fmt_len(metres: f64, style: &DimensionStyle) -> String {
    format_dim_value(metres, style)
}

/// Walk the document and return its statistics.
pub fn collect_stats(scene: &Scene) -> ModelStats {
    let loose = scene.loose_mesh(); // the real loose mesh, group open or not
    let groups = &scene.doc.groups;

    let loose_verts = loose.verts.len();
    let loose_edges = loose.edges.len();
    let loose_faces = loose.faces.len();

    let group_count = groups.len();
    let instances = groups.iter().filter(|g| g.is_component()).count();

    // Instances share their prototype mesh; counting it once per instance is
    // deliberate: these are render/entity counts, the way modellers usually
    // report them, not a dedup of prototypes.
    let placed: Vec<(&Mesh, _)> = groups
        .iter()
        .flat_map(|g| iter_placements(g, None))
        .map(|(pg, m)| (scene.doc.mesh(pg.mesh), m))
        .collect();

    let group_verts = placed.iter().map(|(gm, _)| gm.verts.len()).sum();
    let group_edges = placed.iter().map(|(gm, _)| gm.edges.len()).sum();
    let group_faces = placed.iter().map(|(gm, _)| gm.faces.len()).sum();

    let all_faces: Vec<(&Mesh, _)> = std::iter::once(loose)
        .chain(placed.iter().map(|(gm, _)| *gm))
        .flat_map(|gm| gm.faces.iter().map(move |(f, _)| (gm, f)))
        .collect();
    let tri_count = all_faces
        .iter()
        .map(|(gm, f)| gm.face(*f).outer.len())
        .filter(|&n| n >= 3)
        .map(|n| n - 2)
        .sum();

    // ---- Bounding box (world space: instance prototypes through their xform)
    let mut positions: Vec<_> = loose.verts.iter().map(|(_, v)| v.position).collect();
    for (gm, m) in &placed {
        positions.extend(gm.verts.iter().map(|(_, v)| match m {
            Some(m) => m.project_point(v.position),
            None => v.position,
        }));
    }
    let bbox = if positions.is_empty() {
        BBox::default()
    } else {
        let (mut lo, mut hi) = (positions[0], positions[0]);
        for p in &positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        BBox {
            min_x: lo.x,
            max_x: hi.x,
            min_y: lo.y,
            max_y: hi.y,
            min_z: lo.z,
            max_z: hi.z,
            width: hi.x - lo.x,
            depth: hi.y - lo.y,
            height: hi.z - lo.z,
        }
    };

    // ---- Colors in use: the Paint tray's idiom (colour floats 0–1) ----------
    let mut colors: IndexMap<[u64; 3], ColorStat> = IndexMap::new();
    for (gm, f) in &all_faces {
        let attrs = &gm.face(*f).attrs;
        let Some(col) = attrs.color() else {
            continue;
        };
        let entry = colors
            .entry(col.map(f64::to_bits))
            .or_insert_with(|| ColorStat {
                rgb: col,
                mat: None,
                faces: 0,
                area: 0.0,
            });
        // Registry identity: the entry shows ITS name instead of an
        // anonymous rgb().
        if let Some(mat) = attrs.paint.mat.as_ref().filter(|m| !m.is_empty()) {
            if entry.mat.is_none() {
                entry.mat = Some(mat.clone());
            }
        }
        entry.faces += 1;
        entry.area += gm.face_area(*f);
    }

    ModelStats {
        loose_verts,
        loose_edges,
        loose_faces,
        group_count,
        instances,
        classic_groups: group_count - instances,
        group_verts,
        group_edges,
        group_faces,
        total_verts: loose_verts + group_verts,
        total_edges: loose_edges + group_edges,
        total_faces: loose_faces + group_faces,
        tri_count,
        bbox,
        colors: colors.into_values().collect(),
        layers: scene
            .doc
            .layers
            .iter()
            .map(|ly| LayerStat {
                name: ly.name.clone(),
                visible: ly.visible,
                locked: ly.locked,
            })
            .collect(),
        dims_count: scene.doc.dimensions.len(),
        text_count: scene.doc.text_labels.len(),
        guides_count: scene.doc.guides.len(),
        saved_views_count: scene.saved_views.len(),
        style: scene.dimension_style.clone(),
    }
}

/// The plain-text summary, the Copy to clipboard payload.
pub fn stats_to_text(s: &ModelStats) -> String {
    let style = &s.style;
    let t = |k: &str| tr(k, &[]);
    let mut lines = vec![
        format!("=== Xtrude CAD — {} ===", t("Model Info")),
        String::new(),
        format!("--- {} ---", t("Geometry")),
        format!("{}:  {}", t("Vertices"), fmt_count(s.total_verts)),
        format!("{}:     {}", t("Edges"), fmt_count(s.total_edges)),
        format!("{}:     {}", t("Faces"), fmt_count(s.total_faces)),
        format!("{}: {}", t("Triangles (est.)"), fmt_count(s.tri_count)),
        format!(
            "{}: {}  /  {}: {}",
            t("Groups (classic)"),
            fmt_count(s.classic_groups),
            t("Component instances"),
            fmt_count(s.instances)
        ),
        String::new(),
        format!("--- {} ---", t("Bounding box")),
        format!(
            "X: {}   Y: {}   Z: {}",
            fmt_len(s.bbox.width, style),
            fmt_len(s.bbox.depth, style),
            fmt_len(s.bbox.height, style)
        ),
        String::new(),
        format!("--- {} ---", t("Colors in use")),
    ];
    for m in &s.colors {
        lines.push(format!(
            "  {}: {} {}, {}",
            m.label(),
            m.faces,
            t("faces"),
            fmt_area(m.area)
        ));
    }
    if s.colors.is_empty() {
        lines.push(format!("  {}", t("(none)")));
    }

    lines.push(String::new());
    lines.push(format!("--- {} ({}) ---", t("Layers"), s.layers.len()));
    for ly in &s.layers {
        let vis = if ly.visible { "✓" } else { "✗" };
        let lock = if ly.locked { " 🔒" } else { "" };
        lines.push(format!("  [{vis}] {}{lock}", ly.name));
    }

    lines.extend([
        String::new(),
        format!("--- {} ---", t("Annotations")),
        format!(
            "{}: {}   {}: {}   {}: {}",
            t("Dimensions"),
            s.dims_count,
            t("Text labels"),
            s.text_count,
            t("Guides"),
            s.guides_count
        ),
        format!("{}: {}", t("Saved views"), s.saved_views_count),
    ]);
    lines.join("\n")
}

/// The Geometry tab's `(property, value)` rows; a row
/// with an empty value is a section header, drawn bold.
pub fn geometry_rows(s: &ModelStats) -> Vec<(String, String)> {
    let style = &s.style;
    let row = |k: &str, v: String| (tr(k, &[]), v);
    vec![
        row("TOTALS", String::new()),
        row("Vertices", fmt_count(s.total_verts)),
        row("Edges", fmt_count(s.total_edges)),
        row("Faces", fmt_count(s.total_faces)),
        row("Triangles (est.)", fmt_count(s.tri_count)),
        row("LOOSE GEOMETRY", String::new()),
        row("Vertices", fmt_count(s.loose_verts)),
        row("Edges", fmt_count(s.loose_edges)),
        row("Faces", fmt_count(s.loose_faces)),
        row("GROUPS / COMPONENTS", String::new()),
        row("Groups (classic)", fmt_count(s.classic_groups)),
        row("Component instances", fmt_count(s.instances)),
        row("Group vertices", fmt_count(s.group_verts)),
        row("Group faces", fmt_count(s.group_faces)),
        row("BOUNDING BOX", String::new()),
        row("Width (X)", fmt_len(s.bbox.width, style)),
        row("Depth (Y)", fmt_len(s.bbox.depth, style)),
        row("Height (Z)", fmt_len(s.bbox.height, style)),
        row("ANNOTATIONS", String::new()),
        row("Dimensions", s.dims_count.to_string()),
        row("Text labels", s.text_count.to_string()),
        row("Guides", s.guides_count.to_string()),
        row("Saved views", s.saved_views_count.to_string()),
    ]
}

/// One row of the Colors table.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorRow {
    pub label: String,
    /// The cell's background; `None` on the "(none)" row.
    pub swatch: Option<Rgb>,
    /// Black text over a light swatch, white over a dark one.
    pub dark_text: bool,
    pub faces: String,
    pub area: String,
}

/// The Colors table: named colors first,
/// biggest painted area first; a single "(none)" row when nothing is
/// painted.
pub fn color_rows(s: &ModelStats) -> Vec<ColorRow> {
    let mut mats: Vec<&ColorStat> = s.colors.iter().collect();
    mats.sort_by(|a, b| {
        let named = |m: &ColorStat| u8::from(m.mat.as_deref().unwrap_or("").is_empty());
        named(a).cmp(&named(b)).then(b.area.total_cmp(&a.area))
    });
    if mats.is_empty() {
        return vec![ColorRow {
            label: tr("(none)", &[]),
            swatch: None,
            dark_text: true,
            faces: String::new(),
            area: String::new(),
        }];
    }
    mats.into_iter()
        .map(|m| {
            let [r, g, b] = m.rgb;
            // Legible over any swatch colour.
            let lum = 0.299 * r + 0.587 * g + 0.114 * b;
            ColorRow {
                label: m.label(),
                swatch: Some(m.rgb),
                dark_text: lum > 0.5,
                faces: fmt_count(m.faces),
                area: fmt_area(m.area),
            }
        })
        .collect()
}
