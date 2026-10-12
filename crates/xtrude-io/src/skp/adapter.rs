// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An OpenSKP parse → an Xtrude CAD payload.
//!
//! The `.skp` format stores lengths in **inches** and is **Z-up**, as
//! Xtrude CAD is, so the adapter only scales (inches → metres). A model of
//! modest size keeps its instance tree, one prototype per definition; a
//! large one is flattened into groups with shared prototypes for the
//! definitions worth sharing (the DAE reference-import thresholds).
//! Materials become face colours (a textured one keeps its colour and
//! loses its image) and their names join the material registry.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
use openskp::{Definition, Face, SkpModel};
use xtrude_core::attrs::Paint;
use xtrude_core::math::{DMat4, MatExt};
use xtrude_core::DVec3;
use xtrude_model::materials::{register, Material};

use super::{
    AttrEntry, BackRef, DimRecord, LayerRecord, PayloadFace, PayloadGroup, Placement, Proto,
    SceneRecord, SkpPayload, TextRecord,
};
use crate::dae::{Progress, INST_MIN_POLYS, INST_MIN_SAVED};

/// The `.skp` internal unit, in metres.
pub const INCH: f64 = 0.0254;
/// Guard against pathological instance nesting.
const MAX_DEPTH: usize = 32;
/// Above this many placements a `.skp` is imported
/// flattened and shared; at or below it, it keeps its instance tree.
pub const TREE_MAX_PLACEMENTS: usize = 5000;

/// The adapter's size thresholds (the tests turn them down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdaptOptions {
    pub tree_max_placements: usize,
    /// A definition is shared when its subtree has at least this many
    /// polygons…
    pub inst_min_polys: usize,
    /// …and sharing saves at least this many.
    pub inst_min_saved: usize,
}

impl Default for AdaptOptions {
    fn default() -> Self {
        AdaptOptions {
            tree_max_placements: TREE_MAX_PLACEMENTS,
            inst_min_polys: INST_MIN_POLYS,
            inst_min_saved: INST_MIN_SAVED,
        }
    }
}

/// What the adapter reports when a parse cannot be adapted.
pub type AdaptError = String;

/// A definition's identity for memos and cycle guards.
type Key = *const Definition;

fn key(d: &Definition) -> Key {
    d as *const Definition
}

/// `material_id or inherited`: a zero id counts as none.
fn or_inherited(own: Option<i64>, inherited: Option<i64>) -> Option<i64> {
    own.filter(|&m| m != 0).or(inherited)
}

/// One `[(edge_id, sense), …]` loop as raw local corners in
/// INCHES, or `None` on a dangling reference. The ring follows the loop's
/// CONNECTIVITY (each coedge ends at the vertex it shares with the next),
/// not `sense`, whose meaning changed between OpenSKP releases; `sense`
/// only breaks the tie of a degenerate loop.
pub fn ring_raw(defn: &Definition, lp: &[(i64, i64)]) -> Option<Vec<DVec3>> {
    let edges: Vec<(i64, i64, i64)> = lp
        .iter()
        .map(|(eid, _)| defn.edges.get(eid).map(|e| (*eid, e.v1_id, e.v2_id)))
        .collect::<Option<_>>()?;
    let n = edges.len();
    let mut pts = Vec::with_capacity(n);
    for (i, &(eid, a, b)) in edges.iter().enumerate() {
        let (nid, na, nb) = edges[(i + 1) % n];
        let mut vid = None;
        if nid != eid {
            let a_shared = a == na || a == nb;
            let b_shared = b == na || b == nb;
            if a_shared != b_shared {
                vid = Some(if b_shared { b } else { a });
            }
        }
        // A degenerate loop: fall back to the flag.
        let vid = vid.unwrap_or(if lp[i].1 != 0 { a } else { b });
        let v = defn.vertices.get(&vid)?;
        pts.push(DVec3::new(v.x, v.y, v.z));
    }
    Some(pts)
}

/// An instance matrix (row-major 3×3 + translation in inches)
/// as a matrix whose translation is in metres. A matrix of fewer than 12
/// values is an error.
fn matrix(m: &[f64]) -> Result<DMat4, AdaptError> {
    if m.len() < 12 {
        return Err(format!("instance matrix of {} values", m.len()));
    }
    #[rustfmt::skip]
    let cols = [
        m[0], m[3], m[6], 0.0,
        m[1], m[4], m[7], 0.0,
        m[2], m[5], m[8], 0.0,
        m[9] * INCH, m[10] * INCH, m[11] * INCH, 1.0,
    ];
    Ok(DMat4::from_cols_array(&cols))
}

/// Material id → its attrs entry index (colour 0–1,
/// opacity below 0.999, registry name), and the file's named materials.
fn material_attrs(model: &SkpModel, payload: &mut SkpPayload) -> HashMap<i64, usize> {
    let mut attr_map: IndexMap<i64, usize> = IndexMap::new();
    for (mid, mat) in model.materials_by_id() {
        let c = mat.color;
        let mut paint = Paint {
            color: Some([
                c[0] as f64 / 255.0,
                c[1] as f64 / 255.0,
                c[2] as f64 / 255.0,
            ]),
            ..Default::default()
        };
        if mat.transparency < 0.999 {
            paint.opacity = Some(mat.transparency);
        }
        let ix = payload.push_attrs(AttrEntry {
            paint,
            ..Default::default()
        });
        attr_map.insert(mid, ix);
    }
    // Every named entry gets its identity: same name and recipe merge,
    // same name with another recipe becomes "name (2)".
    let mut registry: IndexMap<String, Material> = IndexMap::new();
    for (mid, mat) in model.materials_by_id() {
        let Some(&ix) = attr_map.get(&mid) else {
            continue;
        };
        let name = mat.name.trim();
        if name.is_empty() {
            continue;
        }
        let paint = &payload.attrs[ix].paint;
        let fin = register(
            &mut registry,
            Material::new(name, paint.color, paint.opacity),
        );
        payload.attrs[ix].paint.mat = Some(fin);
    }
    payload.materials = registry.into_values().collect();
    attr_map.into_iter().collect()
}

/// A face and the index ranges of its outer ring and holes in [`Geom::pos`].
type FaceRanges<'m> = (&'m Face, (usize, usize), Vec<(usize, usize)>);

/// A definition's resolved face rings, cached: every ring
/// corner in local METRES, and per face `(face, outer range, hole ranges)`.
struct Geom<'m> {
    pos: Vec<DVec3>,
    meta: Vec<FaceRanges<'m>>,
}

struct Adapter<'m> {
    model: &'m SkpModel,
    by_id: HashMap<i64, &'m Definition>,
    attr_map: HashMap<i64, usize>,
    geom: HashMap<Key, std::rc::Rc<Geom<'m>>>,
    soft: HashMap<Key, std::rc::Rc<Vec<DVec3>>>,
    payload: SkpPayload,
    options: AdaptOptions,
}

/// What a nested level of [`Adapter::collect`] inherits from the levels
/// above it.
struct Nesting<'s> {
    /// The placement of the level in the root's frame.
    xform: DMat4,
    depth: usize,
    /// The definitions on the path down (a cycle guard).
    stack: HashSet<Key>,
    /// The material a face without its own takes.
    inherited: Option<i64>,
    /// The layer of the nearest tagged instance above.
    layer: Option<&'s str>,
}

impl Nesting<'_> {
    /// The top level, placed by `xform`, with the material `inherited`
    /// from the placement.
    fn root(xform: DMat4, inherited: Option<i64>) -> Self {
        Nesting {
            xform,
            depth: 0,
            stack: HashSet::new(),
            inherited,
            layer: None,
        }
    }
}

/// A shared prototype placement: the transform and the inherited layer.
type ProtoUse = (DMat4, Option<String>);

/// The placements of shared prototypes, per `(definition id, inherited
/// material)`.
type ProtoUses = IndexMap<(i64, Option<i64>), Vec<ProtoUse>>;

/// An instance pulled out as its own group: definition, placement,
/// inherited material and layer.
type GroupUse<'m, L> = (&'m Definition, DMat4, Option<i64>, L);

/// Where [`Adapter::collect`] sends what it does not flatten.
#[derive(Default)]
struct Sinks<'m> {
    /// `(definition id, inherited material)` → placements of a shared
    /// prototype (`proto_uses`).
    proto_uses: Option<ProtoUses>,
    /// Image entities pulled out as their own groups.
    image_uses: Option<Vec<GroupUse<'m, Option<String>>>>,
    /// Tagged nested instances pulled out as their own layer groups.
    layer_uses: Option<Vec<GroupUse<'m, String>>>,
}

impl<'m> Adapter<'m> {
    fn child(&self, ref_idx: Option<i64>) -> Option<&'m Definition> {
        self.by_id.get(&ref_idx?).copied()
    }

    fn defn_geom(&mut self, defn: &'m Definition) -> std::rc::Rc<Geom<'m>> {
        if let Some(g) = self.geom.get(&key(defn)) {
            return g.clone();
        }
        let mut pos: Vec<DVec3> = Vec::new();
        let mut meta = Vec::new();
        for face in defn.faces.values() {
            let Some(first) = face.loops.first() else {
                continue;
            };
            let Some(raw) = ring_raw(defn, first).filter(|r| r.len() >= 3) else {
                continue;
            };
            let s0 = pos.len();
            pos.extend(raw.iter().map(|p| *p * INCH));
            let s1 = pos.len();
            let mut hs = Vec::new();
            for lp in &face.loops[1..] {
                if let Some(h) = ring_raw(defn, lp).filter(|h| h.len() >= 3) {
                    let start = pos.len();
                    pos.extend(h.iter().map(|p| *p * INCH));
                    hs.push((start, pos.len()));
                }
            }
            meta.push((face, (s0, s1), hs));
        }
        let g = std::rc::Rc::new(Geom { pos, meta });
        self.geom.insert(key(defn), g.clone());
        g
    }

    fn attr(&self, mid: Option<i64>) -> Option<usize> {
        self.attr_map.get(&mid?).copied()
    }

    /// One payload face, its corners already transformed.
    /// Precedence: the face's own front material, then its back material
    /// (flipping the face so the painted side fronts), then the enclosing
    /// instance's paint. A back painted differently travels as its own
    /// paint, a back painted the same makes the face two-sided.
    fn face_entry(
        &mut self,
        face: &Face,
        wl: &[DVec3],
        outer_range: (usize, usize),
        holes_sl: &[(usize, usize)],
        inherited: Option<i64>,
        layer: Option<&str>,
    ) -> PayloadFace {
        let mut outer = wl[outer_range.0..outer_range.1].to_vec();
        let mut flipped = false;
        let mut attrs = self.attr(face.material_id);
        if attrs.is_none() {
            if let Some(battrs) = self.attr(face.back_material_id) {
                attrs = Some(battrs);
                outer.reverse();
                flipped = true;
            } else if inherited.is_some() {
                attrs = self.attr(inherited);
            }
        }
        let holes: Vec<Vec<DVec3>> = holes_sl
            .iter()
            .map(|&(a, b)| {
                let mut h = wl[a..b].to_vec();
                if flipped {
                    h.reverse();
                }
                h
            })
            .collect();
        let front_src = attrs;
        if !flipped {
            let mut back_src = self.attr(face.back_material_id);
            if back_src.is_none() && inherited.is_some() {
                back_src = self.attr(inherited);
            }
            if let Some(back) = back_src {
                let mut base = attrs
                    .map(|i| self.payload.attrs[i].clone())
                    .unwrap_or_default();
                base.back = Some(if Some(back) == front_src {
                    BackRef::Mirror
                } else {
                    BackRef::Entry(back)
                });
                attrs = Some(self.payload.push_attrs(base));
            }
        }
        // A face's own layer, or the enclosing tagged instance's. OpenSKP
        // gives the face's own as a raw id, resolved to its name here (the
        // number itself names a layer that never existed). The file's
        // default layer is no layer of its own.
        let own_layer = face
            .layer
            .filter(|&l| l != 0)
            .and_then(|l| self.model.layer_names.get(&l))
            .filter(|n| !n.is_empty() && *n != "Layer0" && *n != "Untagged")
            .cloned();
        if let Some(lay) = own_layer.or_else(|| layer.filter(|l| !l.is_empty()).map(str::to_string))
        {
            let mut base = attrs
                .map(|i| self.payload.attrs[i].clone())
                .unwrap_or_default();
            base.layer = Some(lay);
            attrs = Some(self.payload.push_attrs(base));
        }
        PayloadFace {
            outer,
            holes,
            attrs,
        }
    }

    /// An Image entity's quad, a plain rectangle in
    /// its material's colour (the picture is not imported).
    fn image_quad_faces(
        &self,
        child: &Definition,
        placed: &DMat4,
        inherited: Option<i64>,
    ) -> Vec<PayloadFace> {
        let mut faces = Vec::new();
        for face in child.faces.values() {
            let raw = face.loops.first().and_then(|lp| ring_raw(child, lp));
            let Some(raw) = raw.filter(|r| r.len() >= 3) else {
                continue;
            };
            let outer = raw
                .iter()
                .map(|p| placed.project_point(*p * INCH))
                .collect();
            let mid = face.material_id.or(inherited);
            faces.push(PayloadFace {
                outer,
                holes: Vec::new(),
                attrs: self.attr(mid),
            });
        }
        faces
    }

    /// The transformed endpoints of `defn`'s soft,
    /// smooth and hidden edges (the file's own display flags).
    fn soft_edge_segments(
        &mut self,
        defn: &Definition,
        xform: &DMat4,
        out: &mut Vec<(DVec3, DVec3)>,
    ) {
        let arr = match self.soft.get(&key(defn)) {
            Some(a) => a.clone(),
            None => {
                let mut pairs = Vec::new();
                for e in defn.edges.values() {
                    if !(e.soft || e.smooth || e.hidden) {
                        continue;
                    }
                    let (Some(va), Some(vb)) =
                        (defn.vertices.get(&e.v1_id), defn.vertices.get(&e.v2_id))
                    else {
                        continue;
                    };
                    pairs.push(DVec3::new(va.x, va.y, va.z) * INCH);
                    pairs.push(DVec3::new(vb.x, vb.y, vb.z) * INCH);
                }
                let a = std::rc::Rc::new(pairs);
                self.soft.insert(key(defn), a.clone());
                a
            }
        };
        let wl = world_points(&arr, xform);
        for pair in wl.chunks_exact(2) {
            out.push((pair[0], pair[1]));
        }
    }

    /// The faces of `defn` (placed by `at.xform`) and,
    /// recursively, of every definition its instances place, into `out`.
    /// Shared prototypes, images and tagged instances go to `sinks`
    /// instead when the matching sink is open.
    fn collect(
        &mut self,
        defn: &'m Definition,
        out: &mut Vec<PayloadFace>,
        proto_ids: &HashSet<i64>,
        sinks: &mut Sinks<'m>,
        edges_out: Option<&mut Vec<(DVec3, DVec3)>>,
        at: Nesting<'_>,
    ) -> Result<(), AdaptError> {
        let Nesting {
            xform,
            depth,
            mut stack,
            inherited,
            layer,
        } = at;
        if depth > MAX_DEPTH || stack.contains(&key(defn)) {
            return Ok(());
        }
        stack.insert(key(defn));
        let geom = self.defn_geom(defn);
        if !geom.meta.is_empty() {
            let wl = world_points(&geom.pos, &xform);
            for (face, outer, hs) in &geom.meta {
                let entry = self.face_entry(face, &wl, *outer, hs, inherited, layer);
                out.push(entry);
            }
        }
        let mut edges_out = edges_out;
        if let Some(edges) = edges_out.as_deref_mut() {
            self.soft_edge_segments(defn, &xform, edges);
        }
        for ins in &defn.instances {
            let Some(child) = self.child(ins.ref_idx) else {
                continue;
            };
            let placed = xform * matrix(&ins.matrix)?;
            let child_inherited = or_inherited(ins.material_id, inherited);
            let child_layer = if ins.layer.is_empty() {
                layer.map(str::to_string)
            } else {
                Some(ins.layer.clone())
            };
            if child.is_image {
                match sinks.image_uses.as_mut() {
                    // An Image entity becomes its own group.
                    Some(uses) => uses.push((child, placed, child_inherited, child_layer)),
                    // Inside a prototype being flattened it stays part of it.
                    None => out.extend(self.image_quad_faces(child, &placed, child_inherited)),
                }
                continue;
            }
            if let Some(uses) = sinks.proto_uses.as_mut() {
                if proto_ids.contains(&child.id) {
                    uses.entry((child.id, child_inherited))
                        .or_default()
                        .push((placed, child_layer));
                    continue;
                }
            }
            if let Some(uses) = sinks.layer_uses.as_mut() {
                if !ins.layer.is_empty() {
                    // A tagged nested instance becomes its own layer group.
                    uses.push((child, placed, child_inherited, ins.layer.clone()));
                    continue;
                }
            }
            self.collect(
                child,
                out,
                proto_ids,
                sinks,
                edges_out.as_deref_mut(),
                Nesting {
                    xform: placed,
                    depth: depth + 1,
                    stack: stack.clone(),
                    inherited: child_inherited,
                    layer: child_layer.as_deref(),
                },
            )?;
        }
        Ok(())
    }

    /// The root's own faces (no instance recursion) as a group named after
    /// the file, with its soft edges.
    fn loose_group(&mut self, r: &'m Definition, name: &str) -> Option<PayloadGroup> {
        let geom = self.defn_geom(r);
        let mut loose = Vec::new();
        if !geom.meta.is_empty() {
            let wl = world_points(&geom.pos, &DMat4::IDENTITY);
            for (face, outer, hs) in &geom.meta {
                loose.push(self.face_entry(face, &wl, *outer, hs, None, None));
            }
        }
        if loose.is_empty() {
            return None;
        }
        let mut edges = Vec::new();
        self.soft_edge_segments(r, &DMat4::IDENTITY, &mut edges);
        Some(PayloadGroup {
            name: Some(name.to_string()),
            faces: loose,
            soft_edges: Some(edges),
            ..Default::default()
        })
    }
}

/// Local metres through `xform`.
fn world_points(pos: &[DVec3], xform: &DMat4) -> Vec<DVec3> {
    if *xform == DMat4::IDENTITY {
        return pos.to_vec();
    }
    pos.iter().map(|p| xform.transform_point3(*p)).collect()
}

/// The polygon count of `defn`'s subtree.
fn subtree_polys(
    defn: &Definition,
    by_id: &HashMap<i64, &Definition>,
    memo: &mut HashMap<Key, usize>,
    stack: &HashSet<Key>,
) -> usize {
    let k = key(defn);
    if stack.contains(&k) {
        return 0;
    }
    if let Some(&n) = memo.get(&k) {
        return n;
    }
    let mut stack = stack.clone();
    stack.insert(k);
    let mut n = defn.faces.len();
    for ins in &defn.instances {
        if let Some(child) = ins.ref_idx.and_then(|r| by_id.get(&r)) {
            n += subtree_polys(child, by_id, memo, &stack);
        }
    }
    memo.insert(k, n);
    n
}

/// How many times each definition id is placed in the tree
/// reachable from `defn`.
fn census(
    defn: &Definition,
    by_id: &HashMap<i64, &Definition>,
    uses: &mut IndexMap<i64, usize>,
    depth: usize,
    stack: &HashSet<Key>,
) {
    if depth > MAX_DEPTH || stack.contains(&key(defn)) {
        return;
    }
    let mut stack = stack.clone();
    stack.insert(key(defn));
    for ins in &defn.instances {
        let Some(child) = ins.ref_idx.and_then(|r| by_id.get(&r)) else {
            continue;
        };
        *uses.entry(child.id).or_insert(0) += 1;
        census(child, by_id, uses, depth + 1, &stack);
    }
}

/// Fold prototypes with identical content onto
/// one, repointing every reference; repeated until it settles, as folding
/// a child can make two parents equal.
fn merge_equal_protos(payload: &SkpPayload, mut protos: Vec<Proto>) -> Vec<Proto> {
    if protos.len() < 2 {
        return protos;
    }
    for _round in 0..8 {
        let mut buckets: IndexMap<(Option<String>, usize, usize, usize), Vec<usize>> =
            IndexMap::new();
        for (i, e) in protos.iter().enumerate() {
            let soft = e.soft_edges.as_ref().map_or(0, Vec::len);
            buckets
                .entry((e.name.clone(), e.faces.len(), soft, e.children.len()))
                .or_default()
                .push(i);
        }
        let mut remap: IndexMap<usize, usize> = IndexMap::new();
        for bucket in buckets.values() {
            if bucket.len() < 2 {
                continue;
            }
            let mut seen: HashMap<String, usize> = HashMap::new();
            for &i in bucket {
                let sig = signature(payload, &protos[i]);
                let first = *seen.entry(sig).or_insert(i);
                if first != i {
                    remap.insert(i, first);
                }
            }
        }
        if remap.is_empty() {
            break;
        }
        // Placements of a folded prototype move to the one that survives.
        for (&i, &keep) in &remap {
            if protos[i].instances.is_empty() {
                continue;
            }
            let src_instances = protos[i].instances.clone();
            let src_layers = protos[i].instance_layers.clone();
            if src_layers.is_some() || protos[keep].instance_layers.is_some() {
                let n_dst = protos[keep].instances.len();
                let dst = protos[keep]
                    .instance_layers
                    .get_or_insert_with(|| vec![None; n_dst]);
                dst.extend(
                    src_layers
                        .filter(|l| !l.is_empty())
                        .unwrap_or_else(|| vec![None; src_instances.len()]),
                );
            }
            protos[keep].instances.extend(src_instances);
        }
        let keep_ix: Vec<usize> = (0..protos.len())
            .filter(|i| !remap.contains_key(i))
            .collect();
        let new_of: HashMap<usize, usize> = keep_ix
            .iter()
            .enumerate()
            .map(|(new_i, &old_i)| (old_i, new_i))
            .collect();
        let mut out = Vec::with_capacity(keep_ix.len());
        for &i in &keep_ix {
            let mut e = std::mem::take(&mut protos[i]);
            for k in &mut e.children {
                let target = remap.get(&k.proto).copied().unwrap_or(k.proto);
                k.proto = new_of.get(&target).copied().unwrap_or(target);
            }
            out.push(e);
        }
        protos = out;
    }
    protos
}

/// The content of a prototype as a string (faces, soft edges, children):
/// exact float values, attrs resolved.
fn signature(payload: &SkpPayload, p: &Proto) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for f in &p.faces {
        let _ = write!(
            s,
            "F{:?}{:?}{:?};",
            f.outer,
            f.holes,
            payload.face_attrs(f.attrs)
        );
    }
    let _ = write!(s, "S{:?};", p.soft_edges);
    for k in &p.children {
        let _ = write!(s, "C{}{:?}{:?};", k.proto, k.xform.to_cols_array(), k.layer);
    }
    s
}

/// `file_layer_records`: the file's layers (tags) with their visibility.
/// The model's default layer never travels (it IS Xtrude CAD's default).
pub fn file_layer_records(model: &SkpModel) -> Vec<LayerRecord> {
    model
        .layers
        .iter()
        .filter(|ly| !ly.name.is_empty() && ly.name != "Layer0" && ly.name != "Untagged")
        .map(|ly| LayerRecord {
            name: ly.name.clone(),
            visible: !ly.hidden,
        })
        .collect()
}

/// The file's era: classic (pre-2021) files have a version below 21.
fn legacy_era(version: &str) -> bool {
    let digits: String = version
        .trim_start_matches('{')
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let major = digits.split('.').next().unwrap_or_default();
    !major.is_empty() && major.parse::<u128>().is_ok_and(|v| v < 21)
}

/// Whether the file is a `.skp` with nothing in it (as
/// opposed to one whose geometry the parser missed). A 2021+ file carries
/// its authoring program's render of the model, one flat colour when
/// there is no geometry; legacy files, and any doubt, answer false.
fn is_empty_model(model: &SkpModel, skp_path: Option<&Path>, legacy: bool) -> bool {
    let Some(skp_path) = skp_path else {
        return false;
    };
    if legacy || !model.definitions.is_empty() {
        return false;
    }
    let r = &model.root;
    if !(r.faces.is_empty()
        && r.edges.is_empty()
        && r.instances.is_empty()
        && r.texts.is_empty()
        && r.dimensions.is_empty()
        && r.construction_lines.is_empty()
        && r.construction_points.is_empty())
    {
        return false;
    }
    let Some(png) = read_thumbnail(skp_path) else {
        return false;
    };
    let Ok(img) = image::load_from_memory(&png) else {
        return false;
    };
    let px = img.to_rgba8();
    let mut it = px.pixels();
    match it.next() {
        Some(first) => it.all(|p| p == first),
        None => false,
    }
}

/// `meta/model_thumbnail.png` of the ZIP inside the file (with the size
/// guard the parser uses).
fn read_thumbnail(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let data = std::fs::read(path).ok()?;
    let start = data.windows(4).position(|w| w == b"PK\x03\x04")?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&data[start..])).ok()?;
    let mut entry = zip.by_name("meta/model_thumbnail.png").ok()?;
    openskp::container::validate_zip_entry_size(
        "meta/model_thumbnail.png",
        entry.size(),
        entry.compressed_size(),
    )
    .ok()?;
    let mut out = Vec::new();
    entry.read_to_end(&mut out).ok()?;
    Some(out)
}

/// A SketchUp GROUP, not a component: its definition is anonymous, named
/// `Group#N` by SketchUp itself (`^Group#\d+$`, whose `$` also matches
/// before a final newline).
fn is_group_definition(defn: &Definition) -> bool {
    let name = defn.name.strip_suffix('\n').unwrap_or(&defn.name);
    name.strip_prefix("Group#")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl<'m> Adapter<'m> {
    /// Every definition ONE prototype in local
    /// coordinates (per inherited material), every instance a placement
    /// with its own name, hidden flag and kind; returns the root groups,
    /// the prototypes and the root's placements in file order.
    fn tree_structure(&mut self, roots: &[&'m Definition], name: &str) -> Result<(), AdaptError> {
        let every: HashSet<i64> = self.by_id.keys().copied().collect();
        let mut tree = Tree {
            protos: Vec::new(),
            index_of: HashMap::new(),
            every,
            name: name.to_string(),
        };
        let mut groups = Vec::new();
        let mut top = Vec::new();
        for &r in roots {
            if let Some(g) = self.loose_group(r, name) {
                groups.push(g);
            }
            for ins in &r.instances {
                if let Some(kid) = tree.placement(self, ins, None, 0)? {
                    top.push(kid);
                }
            }
        }
        // Empty entries stay: children point at prototypes by index.
        let protos = tree
            .protos
            .into_iter()
            .map(|p| {
                p.unwrap_or_else(|| Proto {
                    name: Some(name.to_string()),
                    soft_edges: Some(Vec::new()),
                    ..Default::default()
                })
            })
            .collect();
        self.payload.groups = groups;
        self.payload.protos = protos;
        self.payload.placements = top;
        Ok(())
    }
}

/// The state of [`Adapter::tree_structure`].
struct Tree {
    protos: Vec<Option<Proto>>,
    index_of: HashMap<(i64, Option<i64>), Option<usize>>,
    every: HashSet<i64>,
    name: String,
}

impl Tree {
    fn placement<'m>(
        &mut self,
        ad: &mut Adapter<'m>,
        ins: &'m openskp::Instance,
        inh: Option<i64>,
        depth: usize,
    ) -> Result<Option<Placement>, AdaptError> {
        let Some(child) = ad.child(ins.ref_idx) else {
            return Ok(None);
        };
        let Some(ci) = self.build(ad, child, or_inherited(ins.material_id, inh), depth)? else {
            return Ok(None);
        };
        let own = Some(ins.name.clone()).filter(|n| !n.is_empty());
        let name = own.or_else(|| (!is_group_definition(child)).then(|| child.name.clone()));
        Ok(Some(Placement {
            proto: ci,
            xform: matrix(&ins.matrix)?,
            name: Some(name),
            component: Some(!is_group_definition(child)),
            hidden: ins.hidden,
            layer: Some(ins.layer.clone()).filter(|l| !l.is_empty()),
        }))
    }

    /// Index of `(d, inh)`'s prototype, or `None` when it holds nothing at
    /// all. The slot is reserved before recursing, so a cyclic file cannot
    /// spin.
    fn build<'m>(
        &mut self,
        ad: &mut Adapter<'m>,
        d: &'m Definition,
        inh: Option<i64>,
        depth: usize,
    ) -> Result<Option<usize>, AdaptError> {
        let k = (d.id, inh);
        if let Some(&ix) = self.index_of.get(&k) {
            return Ok(ix);
        }
        if depth > MAX_DEPTH {
            return Ok(None);
        }
        let idx = self.protos.len();
        self.index_of.insert(k, Some(idx));
        self.protos.push(None);
        let mut local = Vec::new();
        let mut local_edges = Vec::new();
        // Every definition is a prototype here, so `collect` keeps only the
        // definition's own faces; its instances are walked below, in order.
        let mut sinks = Sinks {
            proto_uses: Some(IndexMap::new()),
            ..Default::default()
        };
        let every = self.every.clone();
        ad.collect(
            d,
            &mut local,
            &every,
            &mut sinks,
            Some(&mut local_edges),
            Nesting::root(DMat4::IDENTITY, inh),
        )?;
        let mut children = Vec::new();
        for ins in &d.instances {
            if let Some(kid) = self.placement(ad, ins, inh, depth + 1)? {
                children.push(kid);
            }
        }
        let empty = local.is_empty() && children.is_empty();
        self.protos[idx] = Some(Proto {
            name: Some(if d.name.is_empty() {
                self.name.clone()
            } else {
                d.name.clone()
            }),
            faces: local,
            soft_edges: Some(local_edges),
            children,
            instances: Vec::new(),
            instance_layers: None,
        });
        if empty {
            self.index_of.insert(k, None);
            return Ok(None);
        }
        Ok(Some(idx))
    }
}

/// An [`SkpModel`] → a payload, or `None` when it yields no
/// geometry (the seam reports the file as unreadable). `skp_path` is the
/// file itself (the empty-model check reads its thumbnail).
pub fn adapt(
    model: &SkpModel,
    name: &str,
    skp_path: Option<&Path>,
    options: &AdaptOptions,
) -> Result<Option<SkpPayload>, AdaptError> {
    let mut payload = SkpPayload::new("openskp");
    let attr_map = material_attrs(model, &mut payload);
    let mut by_id: HashMap<i64, &Definition> = HashMap::new();
    for d in model.definitions.values() {
        by_id.insert(d.id, d);
    }
    let legacy = legacy_era(&model.version);
    let roots: Vec<&Definition> = vec![&model.root];
    let mut ad = Adapter {
        model,
        by_id,
        attr_map,
        geom: HashMap::new(),
        soft: HashMap::new(),
        payload,
        options: *options,
    };

    // Shared-prototype census over the reachable tree.
    let mut uses: IndexMap<i64, usize> = IndexMap::new();
    for &r in &roots {
        census(r, &ad.by_id, &mut uses, 0, &HashSet::new());
    }

    // A model of modest size keeps the file's instance tree, one Group per
    // placement, named and hidden as the author left them.
    let total: usize = uses.values().sum();
    if total <= ad.options.tree_max_placements
        && !roots
            .iter()
            .any(|r| ad.subtree_has_image(r, &HashSet::new()))
    {
        ad.tree_structure(&roots, name)?;
        return Ok(ad.finish(skp_path, legacy));
    }

    let mut memo: HashMap<Key, usize> = HashMap::new();
    let mut proto_ids: HashSet<i64> = HashSet::new();
    for (&did, &cnt) in &uses {
        let Some(&d) = ad.by_id.get(&did) else {
            continue;
        };
        if cnt < 2 || !ad.shareable(d) {
            continue;
        }
        let polys = subtree_polys(d, &ad.by_id, &mut memo, &HashSet::new());
        if polys >= ad.options.inst_min_polys && polys * (cnt - 1) >= ad.options.inst_min_saved {
            proto_ids.insert(did);
        }
    }
    // A definition placed ONCE at the top level is still a definition: it
    // keeps its placement (and so its own axes), unless its subtree places
    // shared prototypes it would swallow.
    for &r in &roots {
        for ins in &r.instances {
            let Some(child) = ad.child(ins.ref_idx) else {
                continue;
            };
            if uses.get(&child.id) != Some(&1) || child.is_image || !ad.shareable(child) {
                continue;
            }
            if ad.subtree_uses(child, &proto_ids, &HashSet::new()) {
                continue;
            }
            proto_ids.insert(child.id);
        }
    }

    let mut groups: Vec<PayloadGroup> = Vec::new();
    let mut sinks = Sinks {
        proto_uses: Some(IndexMap::new()),
        image_uses: Some(Vec::new()),
        layer_uses: Some(Vec::new()),
    };
    for &r in &roots {
        if let Some(g) = ad.loose_group(r, name) {
            groups.push(g);
        }
        // Each top-level instance: its own group, or a shared-prototype use.
        for ins in &r.instances {
            let Some(child) = ad.child(ins.ref_idx) else {
                continue;
            };
            let placed = matrix(&ins.matrix)?;
            let inh = ins.material_id;
            let lay = Some(ins.layer.clone()).filter(|l| !l.is_empty());
            if child.is_image {
                if let Some(u) = sinks.image_uses.as_mut() {
                    u.push((child, placed, inh, lay));
                }
                continue;
            }
            if proto_ids.contains(&child.id) {
                if let Some(u) = sinks.proto_uses.as_mut() {
                    u.entry((child.id, inh)).or_default().push((placed, lay));
                }
                continue;
            }
            let mut sub = Vec::new();
            let mut sub_edges = Vec::new();
            ad.collect(
                child,
                &mut sub,
                &proto_ids,
                &mut sinks,
                Some(&mut sub_edges),
                Nesting::root(placed, inh),
            )?;
            if !sub.is_empty() {
                groups.push(PayloadGroup {
                    name: Some(if child.name.is_empty() {
                        name.to_string()
                    } else {
                        child.name.clone()
                    }),
                    faces: sub,
                    soft_edges: Some(sub_edges),
                    layer: lay,
                    // The instance's own axes.
                    axes: Some(placed),
                });
            }
        }
    }

    // Tagged nested instances → their own groups carrying the layer. The
    // list grows while it is walked: a tagged instance inside a tagged
    // instance extracts recursively.
    let mut i = 0;
    while let Some((child, placed, inh, lay)) =
        sinks.layer_uses.as_ref().and_then(|u| u.get(i)).cloned()
    {
        i += 1;
        let mut sub = Vec::new();
        let mut sub_edges = Vec::new();
        ad.collect(
            child,
            &mut sub,
            &proto_ids,
            &mut sinks,
            Some(&mut sub_edges),
            Nesting::root(placed, inh),
        )?;
        if !sub.is_empty() {
            groups.push(PayloadGroup {
                name: Some(if child.name.is_empty() {
                    name.to_string()
                } else {
                    child.name.clone()
                }),
                faces: sub,
                soft_edges: Some(sub_edges),
                layer: Some(lay),
                axes: Some(placed),
            });
        }
    }

    // Image entities → their own static groups.
    for (child, placed, inh, lay) in sinks.image_uses.take().unwrap_or_default() {
        let faces = ad.image_quad_faces(child, &placed, inh);
        if faces.is_empty() {
            continue;
        }
        groups.push(PayloadGroup {
            name: Some(if child.name.is_empty() {
                name.to_string()
            } else {
                child.name.clone()
            }),
            faces,
            soft_edges: None,
            layer: lay,
            axes: None,
        });
    }

    // Each shared prototype ONCE, in local coordinates, per inherited
    // material; a prototype keeps the prototypes its subtree places as
    // children.
    let mut build = ProtoBuild {
        protos: Vec::new(),
        index_of: HashMap::new(),
        proto_ids,
        name: name.to_string(),
    };
    let proto_uses = sinks.proto_uses.take().unwrap_or_default();
    for ((did, inh), places) in proto_uses {
        if places.is_empty() {
            continue;
        }
        let Some(idx) = build.build(&mut ad, did, inh)? else {
            continue;
        };
        let Some(Some(p)) = build.protos.get_mut(idx) else {
            continue;
        };
        p.instances = places.iter().map(|(xf, _)| *xf).collect();
        if places.iter().any(|(_, ly)| ly.is_some()) {
            // Layer per PLACEMENT: copies can sit on different layers.
            p.instance_layers = Some(places.into_iter().map(|(_, ly)| ly).collect());
        }
    }
    // NOT filtered: children reference prototypes by index.
    let protos: Vec<Proto> = build
        .protos
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect();
    let protos = merge_equal_protos(&ad.payload, protos);
    ad.payload.groups = groups;
    ad.payload.protos = protos;
    Ok(ad.finish(skp_path, legacy))
}

/// The state of a prototype build.
struct ProtoBuild {
    protos: Vec<Option<Proto>>,
    index_of: HashMap<(i64, Option<i64>), usize>,
    proto_ids: HashSet<i64>,
    name: String,
}

impl ProtoBuild {
    /// Index of the prototype for `(did, inh)`, building it (and any
    /// prototype it places) on the way. The slot is reserved before
    /// recursing so a cycle cannot spin.
    fn build<'m>(
        &mut self,
        ad: &mut Adapter<'m>,
        did: i64,
        inh: Option<i64>,
    ) -> Result<Option<usize>, AdaptError> {
        let k = (did, inh);
        if let Some(&ix) = self.index_of.get(&k) {
            return Ok(Some(ix));
        }
        let Some(&d) = ad.by_id.get(&did) else {
            return Ok(None);
        };
        let idx = self.protos.len();
        self.index_of.insert(k, idx);
        self.protos.push(None);
        let mut local = Vec::new();
        let mut local_edges = Vec::new();
        let mut sinks = Sinks {
            proto_uses: Some(IndexMap::new()),
            ..Default::default()
        };
        let ids = self.proto_ids.clone();
        ad.collect(
            d,
            &mut local,
            &ids,
            &mut sinks,
            Some(&mut local_edges),
            Nesting::root(DMat4::IDENTITY, inh),
        )?;
        let mut children = Vec::new();
        for ((cid, cinh), places) in sinks.proto_uses.unwrap_or_default() {
            let Some(ci) = self.build(ad, cid, cinh)? else {
                continue;
            };
            for (xf, ly) in places {
                let mut kid = Placement::new(ci, xf);
                kid.layer = ly;
                children.push(kid);
            }
        }
        self.protos[idx] = Some(Proto {
            name: Some(if d.name.is_empty() {
                self.name.clone()
            } else {
                d.name.clone()
            }),
            faces: local,
            soft_edges: Some(local_edges),
            children,
            instances: Vec::new(),
            instance_layers: None,
        });
        Ok(Some(idx))
    }
}

impl<'m> Adapter<'m> {
    /// Whether `d`'s subtree holds an Image entity.
    fn subtree_has_image(&self, d: &Definition, stack: &HashSet<Key>) -> bool {
        if d.is_image {
            return true;
        }
        if stack.contains(&key(d)) {
            return false;
        }
        let mut stack = stack.clone();
        stack.insert(key(d));
        d.instances.iter().any(|ins| {
            self.child(ins.ref_idx)
                .is_some_and(|c| self.subtree_has_image(c, &stack))
        })
    }

    /// Whether `d`'s subtree holds a tagged instance.
    fn subtree_has_tagged(&self, d: &Definition, stack: &HashSet<Key>) -> bool {
        if stack.contains(&key(d)) {
            return false;
        }
        let mut stack = stack.clone();
        stack.insert(key(d));
        d.instances.iter().any(|ins| {
            !ins.layer.is_empty()
                || self
                    .child(ins.ref_idx)
                    .is_some_and(|c| self.subtree_has_tagged(c, &stack))
        })
    }

    /// Image- and tagged-instance-carrying subtrees are not
    /// shared (each copy's image or tagged part extracts on its own).
    fn shareable(&self, d: &Definition) -> bool {
        !(self.subtree_has_image(d, &HashSet::new()) || self.subtree_has_tagged(d, &HashSet::new()))
    }

    /// Whether `d`'s subtree places a definition in
    /// `wanted`.
    fn subtree_uses(&self, d: &Definition, wanted: &HashSet<i64>, stack: &HashSet<Key>) -> bool {
        if stack.contains(&key(d)) {
            return false;
        }
        let mut stack = stack.clone();
        stack.insert(key(d));
        d.instances.iter().any(|ins| {
            ins.ref_idx.is_some_and(|c| wanted.contains(&c))
                || self
                    .child(ins.ref_idx)
                    .is_some_and(|c| self.subtree_uses(c, wanted, &stack))
        })
    }

    /// Emptiness, layers, scenes, dimensions, texts and
    /// the style's back-face colour around the geometry.
    fn finish(mut self, skp_path: Option<&Path>, legacy: bool) -> Option<SkpPayload> {
        let model = self.model;
        let nothing = self.payload.groups.is_empty()
            && !self
                .payload
                .protos
                .iter()
                .any(|e| !e.faces.is_empty() || !e.children.is_empty());
        if nothing {
            // The parser missed the geometry (None → reported unreadable),
            // or the file really is empty — a template.
            if !is_empty_model(model, skp_path, legacy) {
                return None;
            }
            self.payload.empty = true;
        }
        self.payload.layers = file_layer_records(model);
        for pg in &model.pages {
            let (Some(eye), Some(target)) = (pg.eye, pg.target) else {
                continue;
            };
            let n = self.payload.scenes.len();
            self.payload.scenes.push(SceneRecord {
                name: if pg.name.is_empty() {
                    format!("Scene {}", n + 1)
                } else {
                    pg.name.clone()
                },
                eye: eye.map(|c| c * INCH),
                target: target.map(|c| c * INCH),
                up: Some(pg.up.unwrap_or([0.0, 0.0, 1.0])),
                fov: Some(pg.fov),
                parallel: pg.parallel,
                ortho_height: Some(pg.ortho_height * INCH),
                hidden_layers: pg.hidden_layers.clone(),
            });
        }
        for dm in &model.dimensions {
            let (Some(a), Some(b)) = (dm.a, dm.b) else {
                continue;
            };
            self.payload.dimensions.push(DimRecord {
                a: a.map(|c| c * INCH),
                b: b.map(|c| c * INCH),
                offset: dm.offset * INCH,
                normal: dm.normal,
                text: dm.text.clone(),
            });
        }
        // Leader texts: only point-anchored ones carry a resolved anchor.
        for tx in &model.root.texts {
            let body = tx.text.trim();
            let Some(pt) = tx.point.filter(|_| !body.is_empty()) else {
                continue;
            };
            self.payload.texts.push(TextRecord {
                anchor: pt.map(|c| c * INCH),
                text: body.to_string(),
                hidden: tx.hidden,
                label: tx.label_point.map(|lp| lp.map(|c| c * INCH)),
            });
        }
        // The style's back-face colour, so unpainted backs read as they did
        // for the author.
        if let Some(back) = model.styles.iter().find_map(|st| st.back_color) {
            self.payload.back_color = Some(back.map(|c| f64::from(c) / 255.0));
        }
        Some(self.payload)
    }
}

/// `parse`: read `path` with OpenSKP and adapt it, or `None` when no
/// geometry comes out. A file OpenSKP cannot read is an error.
pub fn parse(
    path: &Path,
    mut progress: Option<Progress<'_>>,
    options: &AdaptOptions,
) -> Result<Option<SkpPayload>, AdaptError> {
    if let Some(p) = progress.as_mut() {
        p(0.1, "Parsing .skp (OpenSKP)…");
    }
    let model = openskp::SkpFile::open(path)
        .and_then(|f| f.parse())
        .map_err(|e| e.to_string())?;
    if let Some(p) = progress.as_mut() {
        p(0.6, "Building geometry…");
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    adapt(&model, &stem, Some(path), options)
}
