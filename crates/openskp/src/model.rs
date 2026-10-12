// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The parsed SketchUp model (`openskp/model.py`): plain data with public
//! fields, and the conversion `SkpFile.parse` runs over `full_parse`'s
//! result ([`SkpModel::from_parsed`]).
//!
//! Lengths are inches, SketchUp's internal unit, Z up. Python dicts keep
//! insertion order; the maps here are [`IndexMap`]s for the same reason.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::parsed::{Parsed, RawDefinition};
use crate::py;

/// A point or vector `(x, y, z)`.
pub type Point = [f64; 3];

/// One attribute value of an attribute dictionary, with its real type.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Null,
    Str(String),
    Int(i64),
    Float(f64),
    /// A `Point3d` or `Vector3d` (a Python 3-tuple).
    Triple(Point),
    List(Vec<AttrValue>),
}

impl AttrValue {
    /// `_stringify_vff_attr_value` / `legacy._stringify_attr_value`: the
    /// `str()` of the value, lists and tuples joined with commas.
    pub fn stringify(&self) -> String {
        match self {
            AttrValue::Null => String::new(),
            AttrValue::Str(s) => s.clone(),
            AttrValue::Int(i) => i.to_string(),
            AttrValue::Float(f) => py::float_repr(*f),
            AttrValue::Triple(t) => t
                .iter()
                .map(|v| py::float_repr(*v))
                .collect::<Vec<_>>()
                .join(","),
            AttrValue::List(items) => items
                .iter()
                .map(AttrValue::stringify)
                .collect::<Vec<_>>()
                .join(","),
        }
    }
}

/// Attribute dictionaries by their declared name.
pub type AttrDicts = IndexMap<String, IndexMap<String, AttrValue>>;

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub id: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A directed edge between two vertices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edge {
    pub id: i64,
    pub v1_id: i64,
    pub v2_id: i64,
    /// Soft edge (merges the faces it borders into one surface).
    pub soft: bool,
    /// Smooth edge (normals interpolate across it).
    pub smooth: bool,
    pub hidden: bool,
    /// The edge's own layer id, when the file stores one.
    pub layer: Option<i64>,
    /// SketchUp's `Edge#curve`: the id of the Curve this edge belongs to.
    pub curve_id: Option<i64>,
}

/// A planar face bounded by edge loops: the first loop is the outer
/// boundary, the rest are holes. Each loop is `(edge_id, orientation)`,
/// orientation `1` along the edge and `-1` against it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Face {
    pub id: i64,
    pub loops: Vec<Vec<(i64, i64)>>,
    pub normal: Option<Point>,
    /// Material of the FRONT side.
    pub material_id: Option<i64>,
    /// Material of the BACK side.
    pub back_material_id: Option<i64>,
    /// A positioned texture's 3×3 row-major texture → face-plane matrix.
    pub uv_transform: Option<[f64; 9]>,
    pub uv_transform_back: Option<[f64; 9]>,
    pub uv_projected: bool,
    pub uv_projected_back: bool,
    pub hidden: bool,
    /// The face's own layer id, when the file stores one.
    pub layer: Option<i64>,
}

/// A layer (tag).
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub color_r: i64,
    pub color_g: i64,
    pub color_b: i64,
    pub hidden: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Layer {
            name: name.into(),
            color_r: 200,
            color_g: 200,
            color_b: 200,
            hidden: false,
        }
    }
}

/// A rendering style bundled in the file: the default front and back
/// face colours unpainted faces show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub name: String,
    pub front_color: Option<[u8; 3]>,
    pub back_color: Option<[u8; 3]>,
}

/// A material's texture image.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Texture {
    pub filename: String,
    /// Tile width in inches; `0.0` when the file does not say.
    pub width: f64,
    pub height: f64,
    /// The image bytes as stored, `None` when the entry is missing.
    pub data: Option<Vec<u8>>,
}

impl Texture {
    /// `Texture.save`: write the image bytes to `path`.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
        let Some(data) = &self.data else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Texture {:?} has no image data", self.filename),
            ));
        };
        std::fs::write(path.as_ref(), data)?;
        Ok(path.as_ref().to_path_buf())
    }
}

/// A surface material.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// RGBA, 0–255.
    pub color: [i64; 4],
    /// Opacity: 0 is fully transparent, 1 fully opaque.
    pub transparency: f64,
    /// The first TLV id that resolves to this material.
    pub id: Option<i64>,
    pub texture: Option<Texture>,
    /// A colourized copy of a textured material (`[Name]1`).
    pub colorized: bool,
    pub colorize_type: i64,
}

impl Material {
    pub fn new(name: impl Into<String>) -> Self {
        Material {
            name: name.into(),
            color: [200, 200, 200, 255],
            transparency: 1.0,
            id: None,
            texture: None,
            colorized: false,
            colorize_type: 0,
        }
    }
}

/// A placed component or group.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub name: String,
    /// The placed definition's id (`None` when the file names none).
    pub ref_idx: Option<i64>,
    pub guid: Option<String>,
    /// 13 floats: the row-major 3×3 at 0–8, the translation at 9–11 and a
    /// scale at 12 that nothing reads. Empty when the file stores none.
    pub matrix: Vec<f64>,
    /// The instance's own layer override, `""` when it has none.
    pub layer: String,
    /// `attribute_dictionaries["dynamic_attributes"]`, stringified.
    pub properties: IndexMap<String, String>,
    pub attribute_dictionaries: AttrDicts,
    /// Material painted on the instance; faces without one inherit it.
    pub material_id: Option<i64>,
    pub hidden: bool,
}

impl Default for Instance {
    fn default() -> Self {
        #[rustfmt::skip]
        let identity = vec![
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        Instance {
            name: String::new(),
            ref_idx: Some(-1),
            guid: Some(String::new()),
            matrix: identity,
            layer: String::new(),
            properties: IndexMap::new(),
            attribute_dictionaries: IndexMap::new(),
            material_id: None,
            hidden: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionPlane {
    pub plane: Vec<f64>,
    pub name: String,
    pub label: String,
    pub hidden: bool,
}

impl Default for SectionPlane {
    fn default() -> Self {
        SectionPlane {
            plane: vec![0.0, 0.0, 1.0, 0.0],
            name: String::new(),
            label: String::new(),
            hidden: false,
        }
    }
}

/// A leader-text label.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEntity {
    pub text: String,
    pub hidden: bool,
    /// The anchor, when the text points at a free point.
    pub point: Option<Point>,
    /// Where the label floats; `None` for screen texts.
    pub label_point: Option<Point>,
}

/// A linear dimension.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dimension {
    /// The displayed text; empty when it shows the measured value.
    pub text: String,
    pub hidden: bool,
    pub a: Option<Point>,
    pub b: Option<Point>,
    /// How far the dimension line sits from the `a`–`b` segment (inches).
    pub offset: f64,
    pub plane_x: Option<Point>,
    pub normal: Option<Point>,
}

/// A saved scene ("page").
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub name: String,
    pub eye: Option<Point>,
    pub target: Option<Point>,
    pub up: Option<Point>,
    /// Field of view in degrees.
    pub fov: f64,
    /// Parallel (orthographic) projection.
    pub parallel: bool,
    /// Visible height in inches when `parallel`.
    pub ortho_height: f64,
    pub hidden_layers: Vec<String>,
}

impl Default for Page {
    fn default() -> Self {
        Page {
            name: String::new(),
            eye: None,
            target: None,
            up: None,
            fov: 35.0,
            parallel: false,
            ortho_height: 0.0,
            hidden_layers: Vec::new(),
        }
    }
}

/// A construction (guide) line; `start`/`end` are `None` where it is
/// unbounded.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionLine {
    pub point: Point,
    pub direction: Point,
    pub start: Option<Point>,
    pub end: Option<Point>,
}

impl Default for ConstructionLine {
    fn default() -> Self {
        ConstructionLine {
            point: [0.0; 3],
            direction: [1.0, 0.0, 0.0],
            start: None,
            end: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConstructionPoint {
    pub position: Point,
}

/// A component definition: reusable geometry and the instances it places.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub id: i64,
    pub guid: Option<String>,
    pub name: String,
    pub vertices: IndexMap<i64, Vertex>,
    pub edges: IndexMap<i64, Edge>,
    pub faces: IndexMap<i64, Face>,
    pub instances: Vec<Instance>,
    pub section_planes: Vec<SectionPlane>,
    pub texts: Vec<TextEntity>,
    pub dimensions: Vec<Dimension>,
    pub construction_lines: Vec<ConstructionLine>,
    pub construction_points: Vec<ConstructionPoint>,
    /// "Always face camera" (2D people and tree cut-outs).
    pub always_faces_camera: bool,
    pub shadows_face_sun: bool,
    /// Backs an Image entity: one textured quad.
    pub is_image: bool,
}

impl Default for Definition {
    fn default() -> Self {
        Definition {
            id: 0,
            guid: Some(String::new()),
            name: String::new(),
            vertices: IndexMap::new(),
            edges: IndexMap::new(),
            faces: IndexMap::new(),
            instances: Vec::new(),
            section_planes: Vec::new(),
            texts: Vec::new(),
            dimensions: Vec::new(),
            construction_lines: Vec::new(),
            construction_points: Vec::new(),
            always_faces_camera: false,
            shadows_face_sun: false,
            is_image: false,
        }
    }
}

/// A whole parsed `.skp`.
#[derive(Debug, Clone, PartialEq)]
pub struct SkpModel {
    pub version: String,
    /// Named definitions by entity id (`None` for a VFF definition that
    /// carries no id). The implicit top-level model is [`SkpModel::root`].
    pub definitions: IndexMap<Option<i64>, Definition>,
    /// The implicit top-level model: what sits directly in the model.
    pub root: Definition,
    pub layers: Vec<Layer>,
    pub pages: Vec<Page>,
    /// Model-level dimensions with world-space endpoints.
    pub dimensions: Vec<Dimension>,
    pub materials: Vec<Material>,
    /// TLV material id → index into `materials`; several ids may alias
    /// one material.
    pub materials_by_id: IndexMap<i64, usize>,
    pub styles: Vec<Style>,
    /// The unit system (`"Millimeter"`); VFF files only.
    pub units: Option<String>,
    /// Layer id → layer name (`full_parse`'s `layer_id_to_name`), for the
    /// raw ids of `Face.layer` and `Edge.layer`. An addition of this crate:
    /// Python OpenSKP keeps the table inside `full_parse`.
    pub layer_names: IndexMap<i64, String>,
}

impl Default for SkpModel {
    fn default() -> Self {
        SkpModel {
            version: "unknown".into(),
            definitions: IndexMap::new(),
            root: Definition::default(),
            layers: Vec::new(),
            pages: Vec::new(),
            dimensions: Vec::new(),
            materials: Vec::new(),
            materials_by_id: IndexMap::new(),
            styles: Vec::new(),
            units: None,
            layer_names: IndexMap::new(),
        }
    }
}

impl SkpModel {
    /// The material a face or instance `material_id` refers to.
    pub fn material(&self, id: i64) -> Option<&Material> {
        self.materials.get(*self.materials_by_id.get(&id)?)
    }

    /// `materials_by_id.items()`: each id with its material.
    pub fn materials_by_id(&self) -> impl Iterator<Item = (i64, &Material)> {
        self.materials_by_id
            .iter()
            .filter_map(|(&id, &i)| Some((id, self.materials.get(i)?)))
    }

    /// The conversion half of `SkpFile.parse`: `full_parse`'s result → the
    /// public model.
    pub fn from_parsed(parsed: Parsed) -> SkpModel {
        let mut model = SkpModel {
            version: parsed.version,
            units: parsed.units,
            ..Default::default()
        };
        let layer_id_to_name = &parsed.layer_id_to_name;

        for (def_id, d) in parsed.defs {
            let defn = convert_definition(def_id.unwrap_or(0), d, layer_id_to_name);
            model.definitions.insert(def_id, defn);
        }
        if let Some(d) = parsed.root {
            // Python's key is the string "ROOT", so the id falls back to 0.
            model.root = convert_definition(0, d, layer_id_to_name);
        }

        for (name, (r, g, b)) in &parsed.layer_colors {
            model.layers.push(Layer {
                name: name.clone(),
                color_r: *r,
                color_g: *g,
                color_b: *b,
                hidden: parsed.layer_hidden.get(name).copied().unwrap_or(false),
            });
        }

        // Hidden layer ids resolve to names; unknown (stale) ids drop.
        for pg in parsed.pages {
            model.pages.push(Page {
                name: pg.name,
                eye: pg.eye,
                target: pg.target,
                up: pg.up,
                fov: pg.fov,
                parallel: pg.parallel,
                ortho_height: pg.ortho_height,
                hidden_layers: pg
                    .hidden_layer_ids
                    .iter()
                    .filter_map(|i| layer_id_to_name.get(i).cloned())
                    .collect(),
            });
        }

        for dm in parsed.dimensions {
            model.dimensions.push(Dimension {
                a: dm.a,
                b: dm.b,
                offset: dm.offset,
                plane_x: dm.plane_x,
                normal: dm.normal,
                text: dm.text,
                hidden: false,
            });
        }

        // One Material per `materials` value, remembered by the identity of
        // its raw dict for the id join below.
        let mut mat_for_data: HashMap<usize, usize> = HashMap::new();
        for &data_ix in parsed.materials.values() {
            let Some(m) = parsed.material_data.get(data_ix) else {
                continue;
            };
            model.materials.push(Material {
                name: m.name.clone(),
                color: [m.color[0], m.color[1], m.color[2], m.alpha.unwrap_or(255)],
                transparency: m.transparency,
                id: None,
                texture: m.texture.clone(),
                colorized: m.colorized,
                colorize_type: m.colorize_type,
            });
            mat_for_data.insert(data_ix, model.materials.len() - 1);
        }

        // Join the TLV material ids onto the materials: by name, then by
        // ZIP folder.
        for (&m_id, m_name) in &parsed.material_id_to_name {
            let data_ix = parsed
                .materials
                .get(m_name)
                .or_else(|| parsed.materials_by_folder.get(m_name));
            let Some(&mi) = data_ix.and_then(|d| mat_for_data.get(d)) else {
                continue;
            };
            let mat = &mut model.materials[mi];
            if mat.id.is_none() {
                mat.id = Some(m_id);
            }
            model.materials_by_id.insert(m_id, mi);
        }

        model.styles = parsed.styles;
        model.layer_names = parsed.layer_id_to_name;
        model
    }
}

fn convert_definition(
    id: i64,
    d: RawDefinition,
    layer_id_to_name: &IndexMap<i64, String>,
) -> Definition {
    let b = d.builder;
    let mut defn = Definition {
        id,
        guid: d.guid,
        name: d.name.unwrap_or_default(),
        always_faces_camera: d.always_faces_camera,
        shadows_face_sun: d.shadows_face_sun,
        is_image: d.is_image,
        ..Default::default()
    };
    for (v_id, [x, y, z]) in b.vertices {
        defn.vertices.insert(v_id, Vertex { id: v_id, x, y, z });
    }
    for (e_id, (v1, v2)) in b.edges {
        let flags = b.edge_flags.get(&e_id).copied().unwrap_or(0);
        defn.edges.insert(
            e_id,
            Edge {
                id: e_id,
                // `v1 or 0`
                v1_id: v1.unwrap_or(0),
                v2_id: v2.unwrap_or(0),
                soft: flags & 0x08 != 0,
                smooth: flags & 0x10 != 0,
                hidden: flags & 0x01 != 0,
                layer: b.edge_layers.get(&e_id).copied(),
                curve_id: b.edge_curves.get(&e_id).copied(),
            },
        );
    }
    for (f_id, f) in b.faces {
        defn.faces.insert(
            f_id,
            Face {
                id: f_id,
                loops: f.loops,
                normal: f.normal,
                material_id: f.material_id,
                back_material_id: f.back_material_id,
                uv_transform: f.uv_transform,
                uv_transform_back: f.uv_transform_back,
                uv_projected: f.uv_projected,
                uv_projected_back: f.uv_projected_back,
                hidden: f.hidden,
                layer: f.layer,
            },
        );
    }
    for inst in b.instances {
        let layer = inst
            .layer_id
            .and_then(|id| layer_id_to_name.get(&id).cloned())
            .unwrap_or_default();
        defn.instances.push(Instance {
            name: inst.name.unwrap_or_default(),
            ref_idx: inst.ref_idx,
            guid: inst.ref_guid,
            matrix: inst.matrix,
            material_id: inst.material_id,
            hidden: inst.hidden,
            layer,
            properties: inst.properties,
            attribute_dictionaries: inst.attribute_dictionaries,
        });
    }
    for sp in b.section_planes {
        defn.section_planes.push(SectionPlane {
            plane: sp.plane,
            name: sp.name,
            label: sp.label,
            hidden: sp.hidden,
        });
    }
    for txt in b.texts {
        defn.texts.push(TextEntity {
            text: txt.text,
            hidden: txt.hidden,
            point: txt.p,
            label_point: txt.lp,
        });
    }
    // Per-definition dimensions keep only their text and hidden flag.
    for dim in b.dimensions {
        defn.dimensions.push(Dimension {
            text: dim.text,
            hidden: dim.hidden,
            ..Default::default()
        });
    }
    for cl in b.construction_lines {
        defn.construction_lines.push(ConstructionLine {
            point: cl.point,
            direction: cl.direction,
            start: cl.start,
            end: cl.end,
        });
    }
    for position in b.construction_points {
        defn.construction_points
            .push(ConstructionPoint { position });
    }
    defn
}
