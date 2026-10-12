// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What `full_parse` returns (`openskp/_core.py full_parse`,
//! `openskp/legacy.py full_parse_legacy`): the dict both walkers fill and
//! [`crate::SkpModel::from_parsed`] turns into the public model.
//!
//! Python spells this as nested dicts; here every dict with fixed keys is
//! a struct. Keys a test may leave out (`layer_hidden`, `units`, …) are
//! fields with the default `.get()` gave.

use indexmap::IndexMap;

use crate::model::{AttrDicts, Point, Style, Texture};

/// One face of a `_GeometryBuilder.faces` entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawFace {
    /// Loops of `(edge_id, orientation)`, orientation `+1`/`-1`.
    pub loops: Vec<Vec<(i64, i64)>>,
    pub normal: Option<Point>,
    pub material_id: Option<i64>,
    pub back_material_id: Option<i64>,
    pub uv_transform: Option<[f64; 9]>,
    pub uv_transform_back: Option<[f64; 9]>,
    pub uv_projected: bool,
    pub uv_projected_back: bool,
    pub hidden: bool,
    /// The raw layer id (`D207`, or the classic drawbase).
    pub layer: Option<i64>,
}

/// One `_GeometryBuilder.instances` entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawInstance {
    pub name: Option<String>,
    pub ref_idx: Option<i64>,
    pub ref_guid: Option<String>,
    pub matrix: Vec<f64>,
    pub material_id: Option<i64>,
    pub hidden: bool,
    /// Unresolved layer id; `SkpModel::from_parsed` resolves the name.
    pub layer_id: Option<i64>,
    pub properties: IndexMap<String, String>,
    pub attribute_dictionaries: AttrDicts,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawSectionPlane {
    pub plane: Vec<f64>,
    pub name: String,
    pub label: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawText {
    pub text: String,
    pub hidden: bool,
    pub p: Option<Point>,
    pub lp: Option<Point>,
}

/// A dimension: per definition (`builder.dimensions`, classic files), or
/// model-level (`parsed["dimensions"]`) when `a`/`b` are resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawDimension {
    pub text: String,
    pub hidden: bool,
    pub offset: f64,
    pub a: Option<Point>,
    pub b: Option<Point>,
    pub plane_x: Option<Point>,
    pub normal: Option<Point>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawConstructionLine {
    pub point: Point,
    pub direction: Point,
    pub start: Option<Point>,
    pub end: Option<Point>,
}

/// A classic `CArcCurve` frame (`legacy._read_arccurve` params).
#[derive(Debug, Clone, PartialEq)]
pub struct ArcParams {
    pub center: Point,
    pub normal: Point,
    pub x_axis: Point,
    pub start_angle: f64,
    pub end_angle: f64,
    pub y_axis: Point,
}

/// `_GeometryBuilder` / `legacy._Builder`: one definition's geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeometryBuilder {
    pub vertices: IndexMap<i64, Point>,
    /// Edge id → (start vertex id, end vertex id); VFF edges may lack one.
    pub edges: IndexMap<i64, (Option<i64>, Option<i64>)>,
    /// Edge id → display flag byte (`D307`: 0x01 hidden, 0x08 soft, 0x10 smooth).
    pub edge_flags: IndexMap<i64, u8>,
    /// Edge id → raw layer id.
    pub edge_layers: IndexMap<i64, i64>,
    /// Edge id → the parent Curve's id (`Edge#curve`).
    pub edge_curves: IndexMap<i64, i64>,
    /// Classic files only: Curve slot → analytic arc frame.
    pub arc_curves: IndexMap<i64, ArcParams>,
    pub faces: IndexMap<i64, RawFace>,
    pub instances: Vec<RawInstance>,
    pub section_planes: Vec<RawSectionPlane>,
    pub texts: Vec<RawText>,
    pub dimensions: Vec<RawDimension>,
    pub construction_lines: Vec<RawConstructionLine>,
    pub construction_points: Vec<Point>,
}

/// One `defs_dict` entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawDefinition {
    pub guid: Option<String>,
    pub name: Option<String>,
    pub always_faces_camera: bool,
    pub shadows_face_sun: bool,
    pub is_image: bool,
    pub builder: GeometryBuilder,
}

/// A parsed `material.xml` (or classic `CMaterial`).
#[derive(Debug, Clone, PartialEq)]
pub struct RawMaterial {
    pub name: String,
    /// `color` `r`, `g`, `b`, and `a` when the file stores one.
    pub color: [i64; 3],
    pub alpha: Option<i64>,
    pub transparency: f64,
    pub colorized: bool,
    pub colorize_type: i64,
    pub texture: Option<Texture>,
}

/// A saved scene (`_parse_pages` / `legacy._scan_pages`).
#[derive(Debug, Clone, PartialEq)]
pub struct RawPage {
    pub name: String,
    pub eye: Option<Point>,
    pub target: Option<Point>,
    pub up: Option<Point>,
    pub fov: f64,
    pub parallel: bool,
    pub ortho_height: f64,
    pub hidden_layer_ids: Vec<i64>,
}

impl Default for RawPage {
    fn default() -> Self {
        RawPage {
            name: String::new(),
            eye: None,
            target: None,
            up: None,
            fov: 35.0,
            parallel: false,
            ortho_height: 0.0,
            hidden_layer_ids: Vec::new(),
        }
    }
}

/// The `full_parse` result.
///
/// `materials` and `materials_by_folder` index into `material_data`: in
/// Python both dicts hold the same material dict objects, and the
/// material-id join compares them by identity.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub version: String,
    pub units: Option<String>,
    pub layer_colors: IndexMap<String, (i64, i64, i64)>,
    pub layer_hidden: IndexMap<String, bool>,
    pub layer_id_to_name: IndexMap<i64, String>,
    pub pages: Vec<RawPage>,
    pub dimensions: Vec<RawDimension>,
    pub material_id_to_name: IndexMap<i64, String>,
    pub material_data: Vec<RawMaterial>,
    pub materials: IndexMap<String, usize>,
    pub materials_by_folder: IndexMap<String, usize>,
    /// Definitions by entity id; `None` for a VFF definition without one.
    pub defs: IndexMap<Option<i64>, RawDefinition>,
    /// The `"ROOT"` entry, when present.
    pub root: Option<RawDefinition>,
    pub styles: Vec<Style>,
}

impl Default for Parsed {
    fn default() -> Self {
        Parsed {
            version: "unknown".into(),
            units: None,
            layer_colors: IndexMap::new(),
            layer_hidden: IndexMap::new(),
            layer_id_to_name: IndexMap::new(),
            pages: Vec::new(),
            dimensions: Vec::new(),
            material_id_to_name: IndexMap::new(),
            material_data: Vec::new(),
            materials: IndexMap::new(),
            materials_by_folder: IndexMap::new(),
            defs: IndexMap::new(),
            root: None,
            styles: Vec::new(),
        }
    }
}

/// The `defs_dict["ROOT"]` entry both walkers add last.
pub fn root_definition(builder: GeometryBuilder) -> RawDefinition {
    RawDefinition {
        guid: Some("ROOT".into()),
        name: Some("ROOT_MODEL".into()),
        builder,
        ..Default::default()
    }
}
