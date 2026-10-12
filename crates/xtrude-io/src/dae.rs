// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! COLLADA (.dae) import + export — the bridge to other modellers.
//!
//! Import scope (what common exporters emit): `library_geometries` meshes
//! (`triangles` / `polylist` / `polygons`), the `visual_scene` node tree
//! with baked transforms (`matrix` / `translate` / `rotate` / `scale`),
//! component instancing via `library_nodes` / `instance_node`,
//! lambert/phong diffuse colours, `up_axis` (Y_UP → Z-up) and the `unit`
//! metre scale. A textured material imports as its image's representative
//! colour; the image itself is not imported. Small models are fused back
//! into editable polygons; big ones become reference groups split along
//! the file's own assemblies, with reused components shared.

use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node, NodeId};
use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::fuse::{fuse_coplanar_loops, soften_smooth_edges, Loop};
use xtrude_core::math::{rotation_deg, DMat4, MatExt};
use xtrude_core::orient::{orient_outward, DEFAULT_SEED};
use xtrude_core::stitch::run_stitch;
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::group::Group;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::Scene;

use crate::meshexport::{collect_geometry, export_names};
use crate::obj::add_fused;
use crate::stl::MAX_FUSE_LOOPS;

const NS: &str = "http://www.collada.org/2005/11/COLLADASchema";

/// A reused component is pulled out as a shared instance only when sharing
/// saves at least this many polygons ((copies − 1) × polys)…
pub const INST_MIN_SAVED: usize = 400;
/// …and the component itself is at least this big.
pub const INST_MIN_POLYS: usize = 60;
/// Reference imports split into at most this many groups…
const MAX_GROUPS: usize = 250;
/// …and never split an assembly this small.
const SPLIT_MIN: usize = 4000;

/// The import's size thresholds (the tests turn them down to exercise the
/// reference-import path on tiny files).
#[derive(Clone, Copy, Debug)]
pub struct DaeLimits {
    /// Above this many polygons the import is reference geometry.
    pub max_fuse_loops: usize,
    /// Never split an assembly this small.
    pub split_min: usize,
}

impl Default for DaeLimits {
    fn default() -> Self {
        DaeLimits {
            max_fuse_loops: MAX_FUSE_LOOPS,
            split_min: SPLIT_MIN,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DaeError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Xml(#[from] roxmltree::Error),
    #[error("{0}")]
    Invalid(String),
}

/// Progress callback: `(fraction, text)`.
pub type Progress<'a> = &'a mut dyn FnMut(f64, &str);

// ---- Export --------------------------------------------------------------------

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The current UTC time as `YYYY-MM-DDTHH:MM:SS+00:00` (ISO 8601, no
/// fractional seconds).
pub fn utc_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn round_key(p: DVec3, digits: usize) -> (u64, u64, u64) {
    let r = |x: f64| xtrude_core::stable_json::round_exact(x, digits).to_bits();
    (r(p.x), r(p.y), r(p.z))
}

/// Write the scene as COLLADA (Z-up, metres, per-face lambert colours).
pub fn save_dae(scene: &Scene, path: &Path, project_name: &str) -> Result<(), DaeError> {
    let (materials, prims) = collect_geometry(scene);
    let names = export_names(&materials);
    let keys: Vec<_> = prims.keys().cloned().collect();
    let mut positions: Vec<DVec3> = Vec::new();
    let mut normals: Vec<DVec3> = Vec::new();
    let mut pidx: HashMap<(u64, u64, u64), usize> = HashMap::new();
    let mut nidx: HashMap<(u64, u64, u64), usize> = HashMap::new();
    let mut tri_groups: Vec<Vec<String>> = Vec::new();
    for key in &keys {
        let mut tokens = Vec::new();
        for (n, verts) in &prims[key] {
            let ni = *nidx.entry(round_key(*n, 5)).or_insert_with(|| {
                normals.push(*n);
                normals.len() - 1
            });
            for p in verts {
                let pi = *pidx.entry(round_key(*p, 6)).or_insert_with(|| {
                    positions.push(*p);
                    positions.len() - 1
                });
                tokens.push(pi.to_string());
                tokens.push(ni.to_string());
            }
        }
        tri_groups.push(tokens);
    }
    let now = utc_now_iso();
    let mut out: Vec<String> = vec![
        r#"<?xml version="1.0" encoding="utf-8"?>"#.into(),
        r#"<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">"#.into(),
        "<asset>".into(),
        "<contributor><authoring_tool>Xtrude CAD</authoring_tool></contributor>".into(),
        format!("<created>{now}</created><modified>{now}</modified>"),
        r#"<unit name="meter" meter="1"/>"#.into(),
        "<up_axis>Z_UP</up_axis>".into(),
        "</asset>".into(),
        "<library_effects>".into(),
    ];
    for (i, key) in keys.iter().enumerate() {
        let [r, g, b] = materials[key].color;
        out.push(format!(r#"<effect id="eff{i}"><profile_COMMON>"#));
        out.push(format!(
            r#"<technique sid="common"><lambert><diffuse><color>{r:.4} {g:.4} {b:.4} 1</color></diffuse></lambert></technique>"#
        ));
        out.push("</profile_COMMON></effect>".into());
    }
    out.push("</library_effects>".into());
    out.push("<library_materials>".into());
    for (i, key) in keys.iter().enumerate() {
        out.push(format!(
            r##"<material id="mat{i}" name="{}"><instance_effect url="#eff{i}"/></material>"##,
            names[key]
        ));
    }
    out.push("</library_materials>".into());
    out.push("<library_geometries>".into());
    out.push(r#"<geometry id="geom0" name="Xtrude CAD"><mesh>"#.into());
    let pos_floats: Vec<String> = positions
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .map(|c| format!("{c:.6}"))
        .collect();
    out.push(format!(
        r##"<source id="geom0-pos"><float_array id="geom0-pos-array" count="{}">{}</float_array><technique_common><accessor source="#geom0-pos-array" count="{}" stride="3"><param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/></accessor></technique_common></source>"##,
        positions.len() * 3,
        pos_floats.join(" "),
        positions.len()
    ));
    let nrm_floats: Vec<String> = normals
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .map(|c| format!("{c:.5}"))
        .collect();
    out.push(format!(
        r##"<source id="geom0-nrm"><float_array id="geom0-nrm-array" count="{}">{}</float_array><technique_common><accessor source="#geom0-nrm-array" count="{}" stride="3"><param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/></accessor></technique_common></source>"##,
        normals.len() * 3,
        nrm_floats.join(" "),
        normals.len()
    ));
    out.push(r##"<vertices id="geom0-vertices"><input semantic="POSITION" source="#geom0-pos"/></vertices>"##.into());
    for (i, tokens) in tri_groups.iter().enumerate() {
        out.push(format!(
            r#"<triangles material="sym{i}" count="{}">"#,
            tokens.len() / 6
        ));
        out.push(r##"<input semantic="VERTEX" source="#geom0-vertices" offset="0"/>"##.into());
        out.push(r##"<input semantic="NORMAL" source="#geom0-nrm" offset="1"/>"##.into());
        out.push(format!("<p>{}</p>", tokens.join(" ")));
        out.push("</triangles>".into());
    }
    out.push("</mesh></geometry>".into());
    out.push("</library_geometries>".into());
    out.push("<library_visual_scenes>".into());
    out.push(format!(
        r#"<visual_scene id="scene"><node id="model" name="{}">"#,
        xml_escape(project_name)
    ));
    out.push(r##"<instance_geometry url="#geom0"><bind_material><technique_common>"##.into());
    for i in 0..tri_groups.len() {
        out.push(format!(
            r##"<instance_material symbol="sym{i}" target="#mat{i}"/>"##
        ));
    }
    out.push("</technique_common></bind_material></instance_geometry>".into());
    out.push("</node></visual_scene></library_visual_scenes>".into());
    out.push(r##"<scene><instance_visual_scene url="#scene"/></scene>"##.into());
    out.push("</COLLADA>".into());
    std::fs::write(path, out.join("\n"))?;
    Ok(())
}

// ---- Import --------------------------------------------------------------------

fn is_col(n: &Node, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(NS)
}

fn tag<'a>(n: &Node<'a, '_>) -> &'a str {
    n.tag_name().name()
}

fn find<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is_col(c, name))
}

fn findall<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a
where
    'i: 'a,
{
    n.children().filter(move |c| is_col(c, name))
}

fn iter_named<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a
where
    'i: 'a,
{
    n.descendants().filter(move |c| is_col(c, name))
}

fn text<'a>(n: Option<Node<'a, '_>>) -> &'a str {
    n.and_then(|n| n.text()).unwrap_or("")
}

fn floats(t: &str) -> Result<Vec<f64>, DaeError> {
    t.split_whitespace()
        .map(|s| {
            s.parse::<f64>()
                .map_err(|_| DaeError::Invalid(format!("could not convert string to float: {s:?}")))
        })
        .collect()
}

fn ints(t: &str) -> Result<Vec<i64>, DaeError> {
    t.split_whitespace()
        .map(|s| {
            s.parse::<i64>()
                .map_err(|_| DaeError::Invalid(format!("invalid literal for int(): {s:?}")))
        })
        .collect()
}

/// A row-major 4×4 (COLLADA's `<matrix>` order) as a matrix.
fn row_major(v: &[f64]) -> DMat4 {
    let mut a = [0.0; 16];
    a.copy_from_slice(&v[..16]);
    DMat4::from_cols_array(&a).transpose()
}

struct Dae<'a, 'i> {
    by_id: HashMap<&'a str, Node<'a, 'i>>,
    base_dir: Option<PathBuf>,
    img_colors: HashMap<String, [f64; 3]>,
    src_cache: HashMap<NodeId, (Vec<f64>, usize)>,
    /// By node only, the first depth asked wins.
    polys_cache: HashMap<NodeId, usize>,
    inst_counts: HashMap<&'a str, usize>,
    scale: f64,
    up: String,
}

impl<'a, 'i> Dae<'a, 'i> {
    fn new(root: Node<'a, 'i>, base_dir: Option<PathBuf>) -> Result<Self, DaeError> {
        let mut inst_counts = HashMap::new();
        for el in iter_named(root, "instance_node") {
            *inst_counts
                .entry(el.attribute("url").unwrap_or(""))
                .or_insert(0) += 1;
        }
        let mut by_id = HashMap::new();
        for el in root.descendants().filter(|n| n.is_element()) {
            if let Some(i) = el.attribute("id") {
                by_id.insert(i, el);
            }
        }
        let mut scale = 1.0;
        let mut up = "Z_UP".to_string();
        if let Some(asset) = find(root, "asset") {
            if let Some(m) = find(asset, "unit")
                .and_then(|u| u.attribute("meter"))
                .filter(|m| !m.is_empty())
            {
                scale = m
                    .parse()
                    .map_err(|_| DaeError::Invalid(format!("bad unit {m:?}")))?;
            }
            if let Some(t) = find(asset, "up_axis").and_then(|u| u.text()) {
                up = t.trim().to_string();
            }
        }
        Ok(Dae {
            by_id,
            base_dir,
            img_colors: HashMap::new(),
            src_cache: HashMap::new(),
            polys_cache: HashMap::new(),
            inst_counts,
            scale,
            up,
        })
    }

    fn reference(&self, url: Option<&str>) -> Option<Node<'a, 'i>> {
        self.by_id
            .get(url.unwrap_or("").trim_start_matches('#'))
            .copied()
    }

    fn zup_matrix(&self) -> DMat4 {
        let s = self.scale;
        match self.up.as_str() {
            "Y_UP" => row_major(&[s, 0., 0., 0., 0., 0., -s, 0., 0., s, 0., 0., 0., 0., 0., 1.]),
            "X_UP" => row_major(&[0., s, 0., 0., s, 0., 0., 0., 0., 0., s, 0., 0., 0., 0., 1.]),
            _ => DMat4::from_scale(DVec3::splat(s)),
        }
    }

    fn to_zup(&self, p: DVec3) -> DVec3 {
        let s = self.scale;
        match self.up.as_str() {
            "Y_UP" => DVec3::new(p.x * s, -p.z * s, p.y * s),
            "X_UP" => DVec3::new(p.y * s, p.x * s, p.z * s),
            _ => p * s,
        }
    }

    fn polys_of(&mut self, node: Node<'a, 'i>, depth: usize) -> usize {
        if let Some(&n) = self.polys_cache.get(&node.id()) {
            return n;
        }
        let n = self.subtree_count(node, depth);
        self.polys_cache.insert(node.id(), n);
        n
    }

    fn source_floats(&mut self, src: Node<'a, 'i>) -> Result<(Vec<f64>, usize), DaeError> {
        if let Some(c) = self.src_cache.get(&src.id()) {
            return Ok(c.clone());
        }
        let data = floats(text(find(src, "float_array")))?;
        let mut stride = 3;
        if let Some(acc) = find(src, "technique_common").and_then(|tc| find(tc, "accessor")) {
            if let Some(s) = acc.attribute("stride").filter(|s| !s.is_empty()) {
                stride = s
                    .parse()
                    .map_err(|_| DaeError::Invalid(format!("bad stride {s:?}")))?;
            }
        }
        let stride = stride.max(1);
        self.src_cache.insert(src.id(), (data.clone(), stride));
        Ok((data, stride))
    }

    fn positions(&mut self, mesh_el: Node<'a, 'i>) -> Result<Vec<DVec3>, DaeError> {
        let Some(verts) = find(mesh_el, "vertices") else {
            return Ok(Vec::new());
        };
        for inp in findall(verts, "input") {
            if inp.attribute("semantic") == Some("POSITION") {
                let Some(src) = self.reference(inp.attribute("source")) else {
                    return Ok(Vec::new());
                };
                let (data, stride) = self.source_floats(src)?;
                let mut out = Vec::new();
                let mut i = 0;
                while i + 2 < data.len() {
                    out.push(DVec3::new(data[i], data[i + 1], data[i + 2]));
                    i += stride;
                }
                return Ok(out);
            }
        }
        Ok(Vec::new())
    }

    fn direct_count(&self, node: Node<'a, 'i>) -> usize {
        let mut n = 0;
        for el in node
            .children()
            .filter(|c| c.is_element() && tag(c) == "instance_geometry")
        {
            if let Some(mesh_el) = self
                .reference(el.attribute("url"))
                .and_then(|g| find(g, "mesh"))
            {
                n += mesh_el
                    .children()
                    .filter(|c| c.is_element())
                    .map(|p| prim_count(&p))
                    .sum::<usize>();
            }
        }
        n
    }

    fn subtree_count(&self, node: Node<'a, 'i>, depth: usize) -> usize {
        if depth > 32 {
            return 0;
        }
        let mut n = self.direct_count(node);
        for el in node.children().filter(|c| c.is_element()) {
            match tag(&el) {
                "node" => n += self.subtree_count(el, depth),
                "instance_node" => {
                    if let Some(t) = self.reference(el.attribute("url")) {
                        n += self.subtree_count(t, depth + 1);
                    }
                }
                _ => {}
            }
        }
        n
    }

    /// Representative RGB of a texture image: the average colour when the
    /// file exists next to the .dae, else a stable light tint from its path.
    fn image_color(&mut self, image_el: Node<'a, 'i>) -> Option<[f64; 3]> {
        let r = text(find(image_el, "init_from")).trim().to_string();
        if r.is_empty() {
            return None;
        }
        if let Some(c) = self.img_colors.get(&r) {
            return Some(*c);
        }
        let mut color = None;
        if let Some(base) = &self.base_dir {
            let candidate = base.join(percent_decode(&r));
            if candidate.is_file() {
                if let Ok(img) = image::open(&candidate) {
                    let rgb = img.to_rgb32f();
                    let n = (rgb.width() as f64 * rgb.height() as f64).max(1.0);
                    let mut acc = [0.0f64; 3];
                    for px in rgb.pixels() {
                        for (a, c) in acc.iter_mut().zip(px.0) {
                            *a += c as f64;
                        }
                    }
                    color = Some(acc.map(|v| v / n));
                }
            }
        }
        let color = color.unwrap_or_else(|| {
            // FNV-1a: stable across runs
            let h = r.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
            });
            [0, 4, 8].map(|s| 0.78 + ((h >> s) & 15) as f64 / 100.0)
        });
        self.img_colors.insert(r, color);
        Some(color)
    }

    fn effect_diffuse(
        &self,
        material: Node<'a, 'i>,
    ) -> Result<(Option<[f64; 3]>, Option<Node<'a, 'i>>), DaeError> {
        let Some(effect) =
            find(material, "instance_effect").and_then(|ie| self.reference(ie.attribute("url")))
        else {
            return Ok((None, None));
        };
        for shader in ["lambert", "phong", "blinn", "constant"] {
            for el in iter_named(effect, shader) {
                let Some(diffuse) = find(el, "diffuse") else {
                    continue;
                };
                if let Some(col) = find(diffuse, "color") {
                    let vals = floats(text(Some(col)))?;
                    if vals.len() >= 3 {
                        return Ok((Some([vals[0], vals[1], vals[2]]), None));
                    }
                }
                if let Some(tex) = find(diffuse, "texture") {
                    let sid = tex.attribute("texture").unwrap_or("");
                    let mut target = self.by_id.get(sid).copied();
                    if target.is_none_or(|t| tag(&t) != "image") {
                        if let Some(np) =
                            iter_named(effect, "newparam").find(|n| n.attribute("sid") == Some(sid))
                        {
                            let surf_sid = find(np, "sampler2D")
                                .and_then(|s| find(s, "source"))
                                .and_then(|s| s.text())
                                .unwrap_or("")
                                .trim();
                            for np2 in iter_named(effect, "newparam")
                                .filter(|n| n.attribute("sid") == Some(surf_sid))
                            {
                                if let Some(init) = find(np2, "surface")
                                    .and_then(|s| find(s, "init_from"))
                                    .and_then(|i| i.text())
                                {
                                    target = self.by_id.get(init.trim()).copied();
                                }
                            }
                        }
                    }
                    if let Some(t) = target.filter(|t| tag(t) == "image") {
                        return Ok((None, Some(t)));
                    }
                }
            }
        }
        Ok((None, None))
    }

    fn material_map(
        &mut self,
        inst_geom: Node<'a, 'i>,
    ) -> Result<HashMap<String, [f64; 3]>, DaeError> {
        let mut out = HashMap::new();
        for im in iter_named(inst_geom, "instance_material") {
            let Some(mat) = self.reference(im.attribute("target")) else {
                continue;
            };
            let (mut color, img) = self.effect_diffuse(mat)?;
            if color.is_none() {
                if let Some(img) = img {
                    color = self.image_color(img);
                }
            }
            if let Some(c) = color {
                out.insert(im.attribute("symbol").unwrap_or("").to_string(), c);
            }
        }
        Ok(out)
    }

    /// The `instance_geometry` carried directly by `node` (no recursion),
    /// transformed by the already-composed `m`.
    fn collect_direct(
        &mut self,
        node: Node<'a, 'i>,
        m: &DMat4,
        out: &mut Vec<(Vec<DVec3>, Option<[f64; 3]>)>,
    ) -> Result<(), DaeError> {
        for el in node
            .children()
            .filter(|c| c.is_element() && tag(c) == "instance_geometry")
        {
            let Some(geom) = self.reference(el.attribute("url")) else {
                continue;
            };
            let mats = self.material_map(el)?;
            let Some(mesh_el) = find(geom, "mesh") else {
                continue;
            };
            let positions = self.positions(mesh_el)?;
            let world: Vec<DVec3> = positions.iter().map(|&p| m.project_point(p)).collect();
            for prim in mesh_el.children().filter(|c| c.is_element()) {
                if !matches!(tag(&prim), "triangles" | "polylist" | "polygons") {
                    continue;
                }
                let color = prim
                    .attribute("material")
                    .and_then(|s| mats.get(s))
                    .copied();
                for lp in prim_loops(&prim, world.len())? {
                    out.push((lp.iter().map(|&i| world[i]).collect(), color));
                }
            }
        }
        Ok(())
    }

    /// Walk a node tree, baking transforms; with `instances` given, a reused
    /// component worth sharing is pulled out instead of baked.
    fn collect(
        &mut self,
        node: Node<'a, 'i>,
        xform: &DMat4,
        out: &mut Vec<(Vec<DVec3>, Option<[f64; 3]>)>,
        depth: usize,
        mut instances: Option<&mut Vec<Instance<'a, 'i>>>,
    ) -> Result<(), DaeError> {
        if depth > 32 {
            return Ok(());
        }
        let m = *xform * node_matrix(node)?;
        self.collect_direct(node, &m, out)?;
        for el in node.children().filter(|c| c.is_element()) {
            let (target, d2) = match tag(&el) {
                "node" => (Some(el), depth),
                "instance_node" => {
                    let target = self.reference(el.attribute("url"));
                    let d2 = depth + 1;
                    if let (Some(t), Some(inst)) = (target, instances.as_deref_mut()) {
                        let cnt = self
                            .inst_counts
                            .get(el.attribute("url").unwrap_or(""))
                            .copied()
                            .unwrap_or(1);
                        let polys = self.polys_of(t, d2);
                        if cnt >= 2
                            && polys >= INST_MIN_POLYS
                            && polys * (cnt - 1) >= INST_MIN_SAVED
                        {
                            let name = el
                                .attribute("name")
                                .map(str::to_string)
                                .unwrap_or_else(|| node_label(&t));
                            inst.push(Instance {
                                name,
                                node: t,
                                xform: m,
                                depth: d2,
                            });
                            continue;
                        }
                    }
                    (target, d2)
                }
                _ => (None, depth),
            };
            if let Some(t) = target {
                self.collect(t, &m, out, d2, instances.as_deref_mut())?;
            }
        }
        Ok(())
    }
}

struct Instance<'a, 'i> {
    name: String,
    node: Node<'a, 'i>,
    xform: DMat4,
    depth: usize,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn node_label(n: &Node) -> String {
    n.attribute("name")
        .or_else(|| n.attribute("id"))
        .unwrap_or("node")
        .to_string()
}

fn prim_count(prim: &Node) -> usize {
    match tag(prim) {
        "triangles" | "polylist" => prim.attribute("count").unwrap_or("0").parse().unwrap_or(0),
        "polygons" => prim.children().filter(|c| is_col(c, "p")).count(),
        _ => 0,
    }
}

/// `(vertex_offset, stride)` of a primitive's interleaved `<p>` stream.
fn prim_inputs(prim: &Node) -> Result<(usize, usize), DaeError> {
    let (mut v_off, mut max_off) = (0usize, 0usize);
    for inp in prim.children().filter(|c| is_col(c, "input")) {
        let off: usize = inp
            .attribute("offset")
            .unwrap_or("0")
            .parse()
            .map_err(|_| DaeError::Invalid("bad input offset".into()))?;
        max_off = max_off.max(off);
        if inp.attribute("semantic") == Some("VERTEX") {
            v_off = off;
        }
    }
    Ok((v_off, max_off + 1))
}

/// The vertex-index loops of one `triangles` / `polylist` / `polygons`.
fn prim_loops(prim: &Node, n_positions: usize) -> Result<Vec<Vec<usize>>, DaeError> {
    let (v_off, stride) = prim_inputs(prim)?;
    let mut loops: Vec<Vec<i64>> = Vec::new();
    let emit = |idx: &[i64], start: usize, nverts: usize, loops: &mut Vec<Vec<i64>>| {
        loops.push(
            (0..nverts)
                .map(|j| idx[start + j * stride + v_off])
                .collect(),
        );
    };
    match tag(prim) {
        "triangles" => {
            let idx = ints(text(find(*prim, "p")))?;
            let per = 3 * stride;
            let mut k = 0;
            while k + per <= idx.len() {
                emit(&idx, k, 3, &mut loops);
                k += per;
            }
        }
        "polylist" => {
            let counts = ints(text(find(*prim, "vcount")))?;
            let idx = ints(text(find(*prim, "p")))?;
            let mut pos = 0usize;
            for c in counts {
                let c = c.max(0) as usize;
                if pos + c * stride <= idx.len() {
                    emit(&idx, pos, c, &mut loops);
                }
                pos += c * stride;
            }
        }
        "polygons" => {
            for p in prim.children().filter(|c| is_col(c, "p")) {
                let idx = ints(text(Some(p)))?;
                emit(&idx, 0, idx.len() / stride, &mut loops);
            }
        }
        _ => {}
    }
    Ok(loops
        .into_iter()
        .filter(|vl| vl.len() >= 3 && vl.iter().all(|&i| 0 <= i && (i as usize) < n_positions))
        .map(|vl| vl.into_iter().map(|i| i as usize).collect())
        .collect())
}

fn node_matrix(node: Node) -> Result<DMat4, DaeError> {
    let mut m = DMat4::IDENTITY;
    for el in node.children().filter(|c| c.is_element()) {
        let v = || floats(el.text().unwrap_or(""));
        match tag(&el) {
            "matrix" => {
                let vals = v()?;
                if vals.len() == 16 {
                    m *= row_major(&vals);
                }
            }
            "translate" => {
                let t = v()?;
                if t.len() >= 3 {
                    m *= DMat4::from_translation(DVec3::new(t[0], t[1], t[2]));
                }
            }
            "rotate" => {
                let r = v()?;
                if r.len() >= 4 {
                    m *= rotation_deg(DVec3::new(r[0], r[1], r[2]), r[3]);
                }
            }
            "scale" => {
                let s = v()?;
                if s.len() >= 3 {
                    m *= DMat4::from_scale(DVec3::new(s[0], s[1], s[2]));
                }
            }
            _ => {}
        }
    }
    Ok(m)
}

/// Face attrs of an imported polygon: its diffuse colour, shown from both
/// sides (a mesh format paints a triangle, not a side).
fn face_attrs(color: Option<[f64; 3]>) -> Option<FaceAttrs> {
    color.map(|c| FaceAttrs {
        paint: Paint {
            color: Some(c),
            ..Default::default()
        },
        back: Some(Back::Mirror),
        ..Default::default()
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Subtree,
    Direct,
}

struct Entry<'a, 'i> {
    tris: usize,
    tie: usize,
    kind: Kind,
    node: Node<'a, 'i>,
    xform: DMat4,
    depth: usize,
    name: String,
    via: bool,
}

impl PartialEq for Entry<'_, '_> {
    fn eq(&self, o: &Self) -> bool {
        self.tris == o.tris && self.tie == o.tie
    }
}
impl Eq for Entry<'_, '_> {}
impl PartialOrd for Entry<'_, '_> {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entry<'_, '_> {
    /// Max-heap on (tris, then the EARLIER tie).
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.tris.cmp(&o.tris).then(o.tie.cmp(&self.tie))
    }
}

type Loops = Vec<(Vec<DVec3>, Option<[f64; 3]>)>;

/// Split the visual scene into `(name, loops)` buckets along the node
/// hierarchy, largest assemblies first, at most [`MAX_GROUPS`].
fn bucketize<'a, 'i>(
    dae: &mut Dae<'a, 'i>,
    top: &[Node<'a, 'i>],
    instances: &mut Vec<Instance<'a, 'i>>,
    split_min: usize,
) -> Result<Vec<(String, Loops)>, DaeError> {
    let mut tie = 0usize;
    let mut next = || {
        tie += 1;
        tie
    };
    let mut heap: BinaryHeap<Entry> = BinaryHeap::new();
    for &nd in top {
        let n = dae.subtree_count(nd, 0);
        if n > 0 {
            heap.push(Entry {
                tris: n,
                tie: next(),
                kind: Kind::Subtree,
                node: nd,
                xform: DMat4::IDENTITY,
                depth: 0,
                name: node_label(&nd),
                via: false,
            });
        }
    }
    let mut finals: Vec<Entry> = Vec::new();
    while !heap.is_empty() && finals.len() + heap.len() < MAX_GROUPS {
        let Some(e) = heap.pop() else { break };
        if e.tris <= split_min {
            heap.push(e);
            break;
        }
        let kids: Vec<Node> = e
            .node
            .children()
            .filter(|c| c.is_element() && tag(c) == "node")
            .collect();
        let insts: Vec<Node> = e
            .node
            .children()
            .filter(|c| c.is_element() && tag(c) == "instance_node")
            .collect();
        if e.kind == Kind::Direct || e.depth > 32 || (kids.is_empty() && insts.is_empty()) {
            finals.push(e);
            continue;
        }
        let m = e.xform * node_matrix(e.node)?;
        let dg = dae.direct_count(e.node);
        if dg > 0 {
            heap.push(Entry {
                tris: dg,
                tie: next(),
                kind: Kind::Direct,
                node: e.node,
                xform: m,
                depth: e.depth,
                name: e.name.clone(),
                via: false,
            });
        }
        for el in kids {
            let n = dae.subtree_count(el, e.depth);
            if n > 0 {
                heap.push(Entry {
                    tris: n,
                    tie: next(),
                    kind: Kind::Subtree,
                    node: el,
                    xform: m,
                    depth: e.depth,
                    name: node_label(&el),
                    via: false,
                });
            }
        }
        for el in insts {
            let Some(target) = dae.reference(el.attribute("url")) else {
                continue;
            };
            let n = dae.subtree_count(target, e.depth + 1);
            if n > 0 {
                let nm = el
                    .attribute("name")
                    .map(str::to_string)
                    .unwrap_or_else(|| node_label(&target));
                heap.push(Entry {
                    tris: n,
                    tie: next(),
                    kind: Kind::Subtree,
                    node: target,
                    xform: m,
                    depth: e.depth + 1,
                    name: nm,
                    via: true,
                });
            }
        }
    }
    let mut entries: Vec<Entry> = finals.into_iter().chain(heap).collect();
    entries.sort_by(|a, b| b.cmp(a)); // sorted((-tris, tie))
    let mut bucket_inst: HashMap<NodeId, usize> = HashMap::new();
    for e in &entries {
        if e.via && e.kind == Kind::Subtree {
            *bucket_inst.entry(e.node.id()).or_insert(0) += 1;
        }
    }
    let mut buckets = Vec::new();
    for e in entries {
        if e.via
            && e.kind == Kind::Subtree
            && bucket_inst.get(&e.node.id()).copied().unwrap_or(0) >= 2
        {
            instances.push(Instance {
                name: e.name,
                node: e.node,
                xform: e.xform,
                depth: e.depth,
            });
            continue;
        }
        let mut loops = Vec::new();
        match e.kind {
            Kind::Direct => dae.collect_direct(e.node, &e.xform, &mut loops)?,
            Kind::Subtree => dae.collect(e.node, &e.xform, &mut loops, e.depth, Some(instances))?,
        }
        if !loops.is_empty() {
            buckets.push((e.name, loops));
        }
    }
    Ok(buckets)
}

fn build_mesh(dae: &Dae, loops: &Loops) -> Mesh {
    let raw: Vec<Loop> = loops
        .iter()
        .map(|(lp, c)| (lp.iter().map(|&p| dae.to_zup(p)).collect(), face_attrs(*c)))
        .collect();
    let mut target = Mesh::new();
    for item in fuse_coplanar_loops(&raw, 0.99999, false) {
        add_fused(&mut target, vec![item]);
    }
    soften_smooth_edges(&mut target, 0.85);
    target
}

/// Add the geometry of a COLLADA file to `scene` (the caller wraps it for
/// undo).
pub fn load_dae(
    scene: &mut Scene,
    path: &Path,
    progress: Option<Progress<'_>>,
) -> Result<(), DaeError> {
    load_dae_with(scene, path, progress, DaeLimits::default())
}

/// [`load_dae`] with explicit size thresholds.
pub fn load_dae_with(
    scene: &mut Scene,
    path: &Path,
    mut progress: Option<Progress<'_>>,
    limits: DaeLimits,
) -> Result<(), DaeError> {
    let mut tick = |f: f64, t: &str| {
        if let Some(p) = progress.as_mut() {
            p(f, t);
        }
    };
    tick(0.02, "Reading file…");
    let text_src = std::fs::read_to_string(path)?;
    let doc = Document::parse_with_options(
        &text_src,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )?;
    let root = doc.root_element();
    let mut dae = Dae::new(root, path.parent().map(Path::to_path_buf))?;
    let top: Vec<Node> = iter_named(root, "visual_scene")
        .flat_map(|vs| findall(vs, "node").collect::<Vec<_>>())
        .collect();
    let total: usize = top.iter().map(|&n| dae.subtree_count(n, 0)).sum();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    if total > limits.max_fuse_loops {
        tick(0.1, "Collecting geometry…");
        let mut instances = Vec::new();
        let buckets = bucketize(&mut dae, &top, &mut instances, limits.split_min)?;
        if buckets.is_empty() && instances.is_empty() {
            return Err(DaeError::Invalid(
                "No geometry found in the COLLADA file".into(),
            ));
        }
        let zm = dae.zup_matrix();
        let zm_inv = zm.try_inverse().unwrap_or(DMat4::IDENTITY);
        let mut n_polys: usize = buckets.iter().map(|(_, l)| l.len()).sum();
        for inst in &instances {
            n_polys += dae.polys_of(inst.node, inst.depth);
        }
        let total_loops = n_polys.max(1) as f64;
        let mut done = 0usize;
        for (name, loops) in &buckets {
            tick(
                0.15 + 0.8 * done as f64 / total_loops,
                &format!("Building {name}…"),
            );
            let target = build_mesh(&dae, loops);
            if !target.faces.is_empty() {
                let id = scene.doc.add_mesh(target);
                scene.doc.groups.push(Group::new(id, Some(name)));
            }
            done += loops.len();
        }
        let mut protos: HashMap<NodeId, Option<MeshId>> = HashMap::new();
        for inst in instances {
            tick(
                0.15 + 0.8 * done as f64 / total_loops,
                &format!("Building {}…", inst.name),
            );
            done += dae.polys_of(inst.node, inst.depth);
            let proto = match protos.get(&inst.node.id()) {
                Some(p) => *p,
                None => {
                    let mut local = Vec::new();
                    dae.collect(inst.node, &DMat4::IDENTITY, &mut local, inst.depth, None)?;
                    let mesh = build_mesh(&dae, &local);
                    let p = if mesh.faces.is_empty() {
                        None
                    } else {
                        Some(scene.doc.add_mesh(mesh))
                    };
                    protos.insert(inst.node.id(), p);
                    p
                }
            };
            if let Some(pid) = proto {
                let mut g = Group::new(pid, Some(&inst.name));
                g.xform = Some(zm * inst.xform * zm_inv);
                scene.doc.groups.push(g);
            }
        }
        scene.version += 1;
        tick(1.0, "Done");
        return Ok(());
    }

    let mut pending: Loops = Vec::new();
    for &n in &top {
        dae.collect(n, &DMat4::IDENTITY, &mut pending, 0, None)?;
    }
    if pending.is_empty() {
        // No visual scene (bare geometry library): import it un-instanced.
        for geom in iter_named(root, "geometry") {
            let Some(mesh_el) = find(geom, "mesh") else {
                continue;
            };
            let positions = dae.positions(mesh_el)?;
            for prim in mesh_el.children().filter(|c| c.is_element()) {
                if matches!(tag(&prim), "triangles" | "polylist" | "polygons") {
                    for lp in prim_loops(&prim, positions.len())? {
                        pending.push((lp.iter().map(|&i| positions[i]).collect(), None));
                    }
                }
            }
        }
    }
    if pending.is_empty() {
        return Err(DaeError::Invalid(
            "No geometry found in the COLLADA file".into(),
        ));
    }
    if pending.len() > limits.max_fuse_loops {
        tick(0.35, "Transforming…");
        let raw: Vec<Loop> = pending
            .iter()
            .map(|(lp, c)| (lp.iter().map(|&p| dae.to_zup(p)).collect(), face_attrs(*c)))
            .collect();
        tick(0.5, "Merging coplanar faces…");
        let fused = fuse_coplanar_loops(&raw, 0.99999, false);
        let n = fused.len().max(1);
        let mut target = Mesh::new();
        for (k, item) in fused.into_iter().enumerate() {
            if k % 8192 == 0 {
                tick(0.6 + 0.3 * k as f64 / n as f64, "Building the model…");
            }
            add_fused(&mut target, vec![item]);
        }
        tick(0.92, "Smoothing edges…");
        soften_smooth_edges(&mut target, 0.85);
        let id = scene.doc.add_mesh(target);
        scene.doc.groups.push(Group::new(id, Some(&stem)));
        scene.version += 1;
        tick(1.0, "Done");
        return Ok(());
    }
    let mut seed: Vec<DVec3> = Vec::new();
    let mut new_faces: HashSet<FaceId> = HashSet::new();
    let target = scene.mesh_mut();
    for (lp, color) in &pending {
        let pts: Vec<DVec3> = lp.iter().map(|&p| dae.to_zup(p)).collect();
        let f = target.add_face(&pts, &[]);
        new_faces.insert(f);
        if let Some(a) = face_attrs(*color) {
            target.face_mut(f).attrs.update(&a);
        }
        seed.extend(pts);
    }
    run_stitch(target, &seed, Some(&mut new_faces), true, true);
    orient_outward(target, DEFAULT_SEED, None);
    scene.version += 1;
    Ok(())
}
