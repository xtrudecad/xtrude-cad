// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3MF export — the package slicers open with its parts, units and colours.
//!
//! The package is a zip holding `[Content_Types].xml`, `_rels/.rels` and
//! `3D/3dmodel.model`. The model is in millimetres (the document is stored
//! in metres). Each top-level group is one `<object>` named after it, and
//! the loose geometry one more. The instances of a component share one
//! object, placed by one `<build><item>` each with the instance's matrix;
//! a mirrored instance, or a group holding nested placements, is written
//! in world coordinates as an object of its own. Vertices are shared
//! within an object and triangles wind counter-clockwise seen from the
//! side the face's normal points to. The paint colours in use form one
//! `<basematerials>`: an object of one colour points at it as a whole, an
//! object of several colours per triangle. [`colour_slots`] gives those
//! colours in the same order, with their face areas: a slicer's filament
//! slot n is the n-th.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use xtrude_core::attrs::{FaceAttrs, Paint, Rgb};
use xtrude_core::math::{DMat3, DMat4, MatExt};
use xtrude_core::measure::face_net_area;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{effective_material, iter_placements, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

use crate::meshexport::{export_names, material_key, MaterialInfo, MaterialKey};

/// Millimetres per model unit (metres).
const MM: f64 = 1000.0;

/// The id of the one `<basematerials>` resource; objects follow it.
const MATERIALS_ID: usize = 1;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

#[derive(Debug, thiserror::Error)]
pub enum ThreeMfError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Format(#[from] std::fmt::Error),
}

/// One `<object>`: shared vertices (millimetres) and triangles, each with
/// its material index, and the faces they come from.
#[derive(Default)]
struct Object {
    name: String,
    vertices: Vec<DVec3>,
    index: HashMap<[u64; 3], usize>,
    triangles: Vec<([usize; 3], usize)>,
    faces: Vec<FaceArea>,
}

/// A face of an object: its material index, unit normal and net area
/// (square model units), in the object's coordinates.
struct FaceArea {
    mat: usize,
    normal: DVec3,
    area: f64,
}

/// What `collect` builds: the triangles for the file, or only the faces
/// (enough for the colours and their areas).
#[derive(Clone, Copy, PartialEq)]
enum Build {
    Triangles,
    Faces,
}

impl Object {
    fn new(name: &str) -> Object {
        Object {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn vertex(&mut self, p: DVec3) -> usize {
        // -0.0 and 0.0 are the same point.
        let key = (p + DVec3::ZERO).to_array().map(f64::to_bits);
        *self.index.entry(key).or_insert_with(|| {
            self.vertices.push(p);
            self.vertices.len() - 1
        })
    }

    /// Whether the object holds nothing to write.
    fn is_empty(&self, build: Build) -> bool {
        match build {
            Build::Triangles => self.triangles.is_empty(),
            Build::Faces => self.faces.is_empty(),
        }
    }

    /// Every face of `mesh` kept by `keep`, mapped through `m` (`None`:
    /// as they are), painted with its own paint, else `owner`'s.
    fn add_mesh(
        &mut self,
        mesh: &Mesh,
        m: Option<DMat4>,
        owner: Option<&Paint>,
        keep: &dyn Fn(xtrude_core::FaceId) -> bool,
        materials: &mut Materials,
        build: Build,
    ) {
        let map = |p: DVec3| m.map_or(p, |mm| mm.project_point(p)) * MM;
        // A mirroring matrix turns every triangle inside out.
        let mirrored = m.is_some_and(|mm| mm.determinant() < 0.0);
        for f in mesh.face_ids() {
            if !keep(f) {
                continue;
            }
            let mat = materials.index(&mesh.face(f).attrs, owner);
            let normal = mesh.face_normal(f);
            let (n, area) = mapped_area(m.as_ref(), normal, face_net_area(mesh, f));
            self.faces.push(FaceArea {
                mat,
                normal: n,
                area,
            });
            if build == Build::Faces {
                continue;
            }
            for [a, b, c] in mesh.face_triangulate(f, None) {
                let ccw = (b - a).cross(c - a).dot(normal) >= 0.0;
                let [a, b, c] = if ccw == mirrored {
                    [a, c, b]
                } else {
                    [a, b, c]
                };
                let t = [
                    self.vertex(map(a)),
                    self.vertex(map(b)),
                    self.vertex(map(c)),
                ];
                if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                    self.triangles.push((t, mat));
                }
            }
        }
    }
}

/// The unit normal and area of a planar face of unit normal `n` and area
/// `area` once mapped through `m` (`None`: as it is). The area scales by
/// the length of the cofactor matrix times `n`, which holds for any linear
/// map, mirroring or not.
fn mapped_area(m: Option<&DMat4>, n: DVec3, area: f64) -> (DVec3, f64) {
    let Some(m) = m else {
        return (n, area);
    };
    let l = DMat3::from_mat4(*m);
    let (a, b, c) = (l.x_axis, l.y_axis, l.z_axis);
    let cof = b.cross(c) * n.x + c.cross(a) * n.y + a.cross(b) * n.z;
    let len = cof.length();
    if len < 1e-30 {
        return (n, 0.0);
    }
    (cof / len, area * len)
}

/// The paint colours in use, in first-seen order.
#[derive(Default)]
struct Materials(IndexMap<MaterialKey, MaterialInfo>);

impl Materials {
    fn index(&mut self, attrs: &FaceAttrs, owner: Option<&Paint>) -> usize {
        let paint = match owner {
            Some(p) if attrs.paint.color.is_none() => p,
            _ => &attrs.paint,
        };
        let key = material_key(&paint_attrs(paint));
        let entry = self.0.entry(key.clone());
        let i = entry.index();
        entry.or_insert_with(|| MaterialInfo {
            color: key.color,
            opacity: key.opacity,
            mat: paint.mat.clone().filter(|m| !m.is_empty()),
        });
        i
    }
}

/// An `<item>`: the object it builds and its placement (`None`: where the
/// object is).
struct Item {
    object: usize,
    transform: Option<DMat4>,
}

/// Whether `m` is a placement a 3MF item can carry: affine and not
/// mirroring.
fn item_matrix(m: &DMat4) -> bool {
    let affine = m.row(3).abs_diff_eq(glam::DVec4::W, 1e-12);
    affine && m.determinant() > 0.0
}

/// Collect the scene's objects and build items. The colours are numbered
/// in the order this walk meets them, whatever `build` asks for.
fn collect(scene: &Scene, build: Build) -> (Vec<Object>, Vec<Item>, Materials) {
    let mut materials = Materials::default();
    let mut objects = Vec::new();
    let mut items = Vec::new();
    let loose = scene.loose_mesh();
    let visible = |f| scene.entity_visible(&EntityRef::Face(MeshId::LOOSE, f));
    let mut obj = Object::new(&xtrude_model::i18n::tr("Loose geometry", &[]));
    obj.add_mesh(loose, None, None, &visible, &mut materials, build);
    if !obj.is_empty(build) {
        items.push(Item {
            object: objects.len(),
            transform: None,
        });
        objects.push(obj);
    }
    // Component instances share an object per (mesh, paint).
    let mut shared: HashMap<(MeshId, Option<MaterialKey>), usize> = HashMap::new();
    let all = |_| true;
    for g in &scene.doc.groups {
        if !scene.entity_visible(&EntityRef::Group(g.id)) {
            continue;
        }
        let own = effective_material(g.material.as_ref(), None);
        if let Some(m) = g.xform.filter(|m| g.children.is_empty() && item_matrix(m)) {
            let key = (g.mesh, own.map(|p| material_key(&paint_attrs(p))));
            let object = match shared.get(&key) {
                Some(&i) => i,
                None => {
                    let mut obj = Object::new(&g.name);
                    obj.add_mesh(
                        scene.doc.mesh(g.mesh),
                        None,
                        own,
                        &all,
                        &mut materials,
                        build,
                    );
                    if obj.is_empty(build) {
                        continue;
                    }
                    objects.push(obj);
                    shared.insert(key, objects.len() - 1);
                    objects.len() - 1
                }
            };
            items.push(Item {
                object,
                transform: Some(m),
            });
            continue;
        }
        let mut obj = Object::new(&g.name);
        add_placements(scene, g, &mut obj, &mut materials, build);
        if !obj.is_empty(build) {
            items.push(Item {
                object: objects.len(),
                transform: None,
            });
            objects.push(obj);
        }
    }
    (objects, items, materials)
}

/// `g` and its visible nested placements, in world coordinates.
fn add_placements(
    scene: &Scene,
    g: &Group,
    obj: &mut Object,
    materials: &mut Materials,
    build: Build,
) {
    let all = |_| true;
    for (pg, m) in iter_placements(g, None) {
        if pg.id != g.id && !scene.entity_visible(&EntityRef::Group(pg.id)) {
            continue;
        }
        let Some(mesh) = scene.doc.meshes.get(&pg.mesh) else {
            continue;
        };
        let paint = effective_material(pg.material.as_ref(), g.material.as_ref());
        obj.add_mesh(mesh, m, paint, &all, materials, build);
    }
}

/// One colour the export writes, as a slicer's filament slot.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourSlot {
    /// The colour and opacity it is written with (unpainted faces: the
    /// default colour).
    pub key: MaterialKey,
    /// The registry name of the first face met with one.
    pub mat: Option<String>,
    /// Whether it is the colour of unpainted faces.
    pub unpainted: bool,
    /// The net area of its faces, every placed copy counted (square
    /// model units).
    pub area: f64,
}

/// The colours a 3MF export of `scene` writes, in its `<basematerials>`
/// order: slot 1 first. Only the faces are walked, not triangulated.
pub fn colour_slots(scene: &Scene) -> Vec<ColourSlot> {
    let (objects, items, materials) = collect(scene, Build::Faces);
    let mut areas = vec![0.0; materials.0.len()];
    for item in &items {
        for f in &objects[item.object].faces {
            areas[f.mat] += mapped_area(item.transform.as_ref(), f.normal, f.area).1;
        }
    }
    let unpainted = material_key(&FaceAttrs::default());
    materials
        .0
        .into_iter()
        .zip(areas)
        .map(|((key, info), area)| ColourSlot {
            unpainted: key == unpainted && info.mat.is_none(),
            key,
            mat: info.mat,
            area,
        })
        .collect()
}

fn paint_attrs(p: &Paint) -> FaceAttrs {
    FaceAttrs {
        paint: p.clone(),
        ..Default::default()
    }
}

/// XML-escape an attribute value.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// `#RRGGBB`, or `#RRGGBBAA` when translucent.
fn display_color(color: Rgb, opacity: Option<f64>) -> String {
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut s = format!(
        "#{:02X}{:02X}{:02X}",
        byte(color[0]),
        byte(color[1]),
        byte(color[2])
    );
    if let Some(o) = opacity {
        s.push_str(&format!("{:02X}", byte(o)));
    }
    s
}

/// A number as the file writes it: shortest round-trip form, no `-0`.
fn num(v: f64) -> String {
    format!("{}", v + 0.0)
}

/// The 3MF row-vector matrix of `m` (model units → millimetres):
/// the three axes, then the translation.
fn transform_attr(m: &DMat4) -> String {
    let (x, y, z, w) = (m.x_axis, m.y_axis, m.z_axis, m.w_axis * MM);
    [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, w.x, w.y, w.z]
        .iter()
        .map(|&v| num(v))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `3D/3dmodel.model` document of `scene`.
pub fn model_xml(scene: &Scene) -> Result<String, ThreeMfError> {
    let (objects, items, materials) = collect(scene, Build::Triangles);
    let mut x = String::new();
    writeln!(x, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        x,
        r#"<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">"#
    )?;
    writeln!(
        x,
        r#" <metadata name="Application">Xtrude CAD {}</metadata>"#,
        xtrude_model::VERSION
    )?;
    writeln!(x, " <resources>")?;
    if !materials.0.is_empty() {
        let names = export_names(&materials.0);
        writeln!(x, r#"  <basematerials id="{MATERIALS_ID}">"#)?;
        for (key, info) in &materials.0 {
            writeln!(
                x,
                r#"   <base name="{}" displaycolor="{}"/>"#,
                escape(&names[key]),
                display_color(info.color, info.opacity)
            )?;
        }
        writeln!(x, "  </basematerials>")?;
    }
    for (i, obj) in objects.iter().enumerate() {
        let id = MATERIALS_ID + 1 + i;
        let first = obj.triangles.first().map_or(0, |t| t.1);
        let mixed = obj.triangles.iter().any(|t| t.1 != first);
        writeln!(
            x,
            r#"  <object id="{id}" type="model" name="{}" pid="{MATERIALS_ID}" pindex="{first}">"#,
            escape(&obj.name)
        )?;
        writeln!(x, "   <mesh>")?;
        writeln!(x, "    <vertices>")?;
        for p in &obj.vertices {
            writeln!(
                x,
                r#"     <vertex x="{}" y="{}" z="{}"/>"#,
                num(p.x),
                num(p.y),
                num(p.z)
            )?;
        }
        writeln!(x, "    </vertices>")?;
        writeln!(x, "    <triangles>")?;
        for ([a, b, c], mat) in &obj.triangles {
            if mixed {
                writeln!(
                    x,
                    r#"     <triangle v1="{a}" v2="{b}" v3="{c}" pid="{MATERIALS_ID}" p1="{mat}"/>"#
                )?;
            } else {
                writeln!(x, r#"     <triangle v1="{a}" v2="{b}" v3="{c}"/>"#)?;
            }
        }
        writeln!(x, "    </triangles>")?;
        writeln!(x, "   </mesh>")?;
        writeln!(x, "  </object>")?;
    }
    writeln!(x, " </resources>")?;
    writeln!(x, " <build>")?;
    for item in &items {
        let id = MATERIALS_ID + 1 + item.object;
        match &item.transform {
            Some(m) => writeln!(
                x,
                r#"  <item objectid="{id}" transform="{}"/>"#,
                transform_attr(m)
            )?,
            None => writeln!(x, r#"  <item objectid="{id}"/>"#)?,
        }
    }
    writeln!(x, " </build>")?;
    writeln!(x, "</model>")?;
    Ok(x)
}

/// Write `scene` to `path` as a 3MF package.
pub fn save_3mf(scene: &Scene, path: &Path) -> Result<(), ThreeMfError> {
    let model = model_xml(scene)?;
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", RELS),
        ("3D/3dmodel.model", model.as_str()),
    ] {
        zip.start_file(name, opts)?;
        zip.write_all(body.as_bytes())?;
    }
    zip.finish()?.flush()?;
    Ok(())
}
