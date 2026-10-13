// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! SketchUp `.skp` import: the single seam
//! between Xtrude CAD and how a `.skp` is read.
//!
//! **Parse then apply.** A backend *parses* a file into a plain
//! [`SkpPayload`] (world-space face loops, no `Scene` touched); the heavy
//! parse runs off the UI thread. [`apply_payload`] then adds the geometry
//! cheaply inside one undo step. When no backend produces geometry,
//! [`parse_skp`] fails with [`NeedsConverter`] before any mutation, so a
//! failed parse never leaves a half-applied edit.
//!
//! The one backend reads with the `openskp` crate ([`adapter`]).

pub mod adapter;

use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::fuse::{fuse_coplanar_loops, soften_smooth_edges, Loop};
use xtrude_core::math::DMat4;
use xtrude_core::topology::maximal_holes;
use xtrude_core::{DVec3, Mesh};
use xtrude_model::definitions::split_divergent_copies;
use xtrude_model::dimension::Dimension;
use xtrude_model::group::{next_group_name, Group};
use xtrude_model::ids::MeshId;
use xtrude_model::layers::Layer;
use xtrude_model::materials::{register, Material};
use xtrude_model::saved_views::{from_lookat, LookAt};
use xtrude_model::scene::Scene;
use xtrude_model::textlabel::TextLabel;

use crate::dae::Progress;
use crate::obj::add_fused;

pub use adapter::AdaptOptions;

// ---- The payload ---------------------------------------------------------------

/// What the back of a face shows (`attrs["back"]`).
#[derive(Clone, Debug, PartialEq)]
pub enum BackRef {
    /// `True`: the back mirrors the front.
    Mirror,
    /// The paint of another attrs entry (a material's own entry).
    Entry(usize),
}

/// One face-attrs entry of the payload, shared per material between every
/// face painted with it, so a rename on apply rewrites it once; its
/// identity is the entry's index in [`SkpPayload::attrs`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttrEntry {
    pub paint: Paint,
    pub back: Option<BackRef>,
    pub layer: Option<String>,
}

/// One payload polygon: its outer ring, holes and attrs (an index into
/// [`SkpPayload::attrs`]), in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct PayloadFace {
    pub outer: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    pub attrs: Option<usize>,
}

/// A group of flattened geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PayloadGroup {
    pub name: Option<String>,
    pub faces: Vec<PayloadFace>,
    /// The file's soft/smooth/hidden edges as segment endpoints; `None`
    /// for geometry without edge flags (image quads), which gets the
    /// DAE-style clean-up instead.
    pub soft_edges: Option<Vec<(DVec3, DVec3)>>,
    pub layer: Option<String>,
    /// The `.skp` instance's own axes; the mesh stays in world
    /// coordinates.
    pub axes: Option<DMat4>,
}

/// One placement of a prototype: a nested child, or (in a tree import) a
/// top-level instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub proto: usize,
    pub xform: DMat4,
    /// The placement's own name record: `None` when it has none (the
    /// prototype's name applies), `Some(None)` for an unnamed group ("Group
    /// N").
    pub name: Option<Option<String>>,
    /// Component (true) or group; `None` leaves the default.
    pub component: Option<bool>,
    pub hidden: bool,
    pub layer: Option<String>,
}

impl Placement {
    pub fn new(proto: usize, xform: DMat4) -> Self {
        Placement {
            proto,
            xform,
            name: None,
            component: None,
            hidden: false,
            layer: None,
        }
    }
}

/// A shared prototype: built once in local coordinates, placed by matrix.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proto {
    pub name: Option<String>,
    pub faces: Vec<PayloadFace>,
    pub soft_edges: Option<Vec<(DVec3, DVec3)>>,
    /// Prototypes this one places, in its local frame.
    pub children: Vec<Placement>,
    /// Top-level placements (local → world).
    pub instances: Vec<DMat4>,
    /// The layer of each placement, when any has one.
    pub instance_layers: Option<Vec<Option<String>>>,
}

/// A file layer (tag) with its visibility.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRecord {
    pub name: String,
    pub visible: bool,
}

/// A saved scene, in metres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneRecord {
    pub name: String,
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: Option<[f64; 3]>,
    pub fov: Option<f64>,
    pub parallel: bool,
    pub ortho_height: Option<f64>,
    pub hidden_layers: Vec<String>,
}

/// A linear dimension, in metres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimRecord {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub offset: f64,
    pub normal: Option<[f64; 3]>,
    pub text: String,
}

/// A leader text, in metres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextRecord {
    pub anchor: [f64; 3],
    pub text: String,
    pub hidden: bool,
    pub label: Option<[f64; 3]>,
}

/// A parsed `.skp`, ready for [`apply_payload`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkpPayload {
    pub backend: String,
    /// Face attrs, shared by index (see [`AttrEntry`]).
    pub attrs: Vec<AttrEntry>,
    pub groups: Vec<PayloadGroup>,
    pub protos: Vec<Proto>,
    /// A tree import's top-level instances, in file order.
    pub placements: Vec<Placement>,
    /// The file really is empty (a template).
    pub empty: bool,
    /// The file's named materials, for the scene registry.
    pub materials: Vec<Material>,
    pub layers: Vec<LayerRecord>,
    pub scenes: Vec<SceneRecord>,
    pub dimensions: Vec<DimRecord>,
    pub texts: Vec<TextRecord>,
    /// The style's back-face colour, 0–1.
    pub back_color: Option<[f64; 3]>,
}

impl SkpPayload {
    pub fn new(backend: &str) -> Self {
        SkpPayload {
            backend: backend.to_string(),
            ..Default::default()
        }
    }

    /// Add an attrs entry; returns its index.
    pub fn push_attrs(&mut self, entry: AttrEntry) -> usize {
        self.attrs.push(entry);
        self.attrs.len() - 1
    }

    /// The face attrs an entry stands for, back resolved.
    pub fn face_attrs(&self, index: Option<usize>) -> Option<FaceAttrs> {
        let e = self.attrs.get(index?)?;
        let back = match &e.back {
            None => None,
            Some(BackRef::Mirror) => Some(Back::Mirror),
            Some(BackRef::Entry(i)) => Some(Back::Own(self.attrs.get(*i)?.paint.clone())),
        };
        Some(FaceAttrs {
            paint: e.paint.clone(),
            back,
            layer: e.layer.clone(),
            ..Default::default()
        })
    }

    /// Whether the payload carries geometry (`groups or protos or empty`).
    pub fn has_content(&self) -> bool {
        !self.groups.is_empty() || !self.protos.is_empty() || self.empty
    }
}

// ---- The seam ------------------------------------------------------------------

/// `NeedsConverter`: no backend can read this `.skp` (the name is
/// historical: it once meant "use the external converter"). The caller
/// reports the file as unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsConverter {
    pub path: PathBuf,
    pub format: String,
}

impl fmt::Display for NeedsConverter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "No pure SKP backend for {} (format={})",
            self.path.display(),
            self.format
        )
    }
}

impl std::error::Error for NeedsConverter {}

/// `detect_format`: `"skp"` for a `.skp` document (a `PK` ZIP, or the
/// UTF-16 `Sketch…` model marker in the first 64 bytes), `"unknown"`
/// otherwise or when the file cannot be read.
pub fn detect_format(path: &Path) -> &'static str {
    let mut head = Vec::with_capacity(64);
    let read = std::fs::File::open(path).and_then(|f| f.take(64).read_to_end(&mut head));
    if read.is_err() {
        return "unknown";
    }
    if head.starts_with(b"PK\x03\x04") {
        return "skp";
    }
    let marker = b"S\x00k\x00e\x00t\x00c\x00h";
    if head.windows(marker.len()).any(|w| w == marker) {
        return "skp";
    }
    "unknown"
}

/// A way of reading a `.skp`.
pub trait SkpBackend {
    fn name(&self) -> &str;
    fn available(&self) -> bool;
    fn supports(&self, fmt: &str) -> bool;
    /// The payload, `Ok(None)` when no geometry came out, `Err` when the
    /// file could not be read.
    fn parse(
        &self,
        path: &Path,
        progress: Option<Progress<'_>>,
    ) -> Result<Option<SkpPayload>, String>;
}

/// The OpenSKP backend: any recognised `.skp`.
#[derive(Clone, Debug, Default)]
pub struct OpenSkpBackend {
    pub options: AdaptOptions,
}

impl SkpBackend for OpenSkpBackend {
    fn name(&self) -> &str {
        "openskp"
    }

    fn available(&self) -> bool {
        true
    }

    fn supports(&self, fmt: &str) -> bool {
        fmt == "skp"
    }

    fn parse(
        &self,
        path: &Path,
        progress: Option<Progress<'_>>,
    ) -> Result<Option<SkpPayload>, String> {
        adapter::parse(path, progress, &self.options)
    }
}

/// `backends_status`: `(name, available)` for each backend.
pub fn backends_status() -> Vec<(String, bool)> {
    let b = OpenSkpBackend::default();
    vec![(b.name().to_string(), b.available())]
}

/// `can_handle`: a backend is available and recognises `path`. Does not
/// promise a non-empty parse.
pub fn can_handle(path: &Path) -> bool {
    can_handle_with(path, &[&OpenSkpBackend::default()])
}

pub fn can_handle_with(path: &Path, backends: &[&dyn SkpBackend]) -> bool {
    let fmt = detect_format(path);
    backends.iter().any(|b| b.available() && b.supports(fmt))
}

/// `parse_skp`: the payload of the first backend that produces geometry,
/// or [`NeedsConverter`] (unrecognised format, parser error or an empty
/// parse). Touches no `Scene`.
pub fn parse_skp(
    path: &Path,
    progress: Option<Progress<'_>>,
) -> Result<SkpPayload, NeedsConverter> {
    parse_skp_with(path, progress, &[&OpenSkpBackend::default()])
}

/// [`parse_skp`] over an explicit backend list.
pub fn parse_skp_with(
    path: &Path,
    mut progress: Option<Progress<'_>>,
    backends: &[&dyn SkpBackend],
) -> Result<SkpPayload, NeedsConverter> {
    let fmt = detect_format(path);
    for backend in backends {
        if !(backend.available() && backend.supports(fmt)) {
            continue;
        }
        // A parser that chokes → try the next one.
        let reborrow: Option<Progress<'_>> = match progress.as_mut() {
            Some(p) => Some(&mut **p),
            None => None,
        };
        let payload = backend.parse(path, reborrow).ok().flatten();
        // Protos count as geometry: a file of components placed once has
        // no plain groups at all.
        if let Some(payload) = payload.filter(SkpPayload::has_content) {
            return Ok(payload);
        }
    }
    Err(NeedsConverter {
        path: path.to_path_buf(),
        format: fmt.to_string(),
    })
}

/// `load_skp`: [`parse_skp`] then [`apply_payload`]. The UI splits the two
/// so the heavy parse runs outside the undo history.
pub fn load_skp(
    scene: &mut Scene,
    path: &Path,
    progress: Option<Progress<'_>>,
) -> Result<String, String> {
    load_skp_with(scene, path, progress, &[&OpenSkpBackend::default()])
}

/// [`load_skp`] over an explicit backend list.
pub fn load_skp_with(
    scene: &mut Scene,
    path: &Path,
    progress: Option<Progress<'_>>,
    backends: &[&dyn SkpBackend],
) -> Result<String, String> {
    let payload = parse_skp_with(path, progress, backends).map_err(|e| e.to_string())?;
    apply_payload(scene, &payload)
}

// ---- Apply ---------------------------------------------------------------------

/// One payload face list as a mesh. With the file's edge
/// flags, the original polygons go in as they are (bulk-welded above 1024
/// corners) and exactly the flagged edges soften; without them, the
/// DAE-style coplanar fusion and angle-based softening run.
fn build_mesh(
    payload: &SkpPayload,
    faces: &[PayloadFace],
    soft_edges: Option<&[(DVec3, DVec3)]>,
) -> Mesh {
    let mut mesh = Mesh::new();
    let Some(soft_edges) = soft_edges else {
        // Faces with holes keep their explicit topology.
        let raw: Vec<Loop> = faces
            .iter()
            .filter(|f| f.holes.is_empty())
            .map(|f| (f.outer.clone(), payload.face_attrs(f.attrs)))
            .collect();
        for item in fuse_coplanar_loops(&raw, 0.99999, false) {
            add_fused(&mut mesh, vec![item]);
        }
        for f in faces.iter().filter(|f| !f.holes.is_empty()) {
            let id = mesh.add_face(&f.outer, &f.holes);
            if let Some(a) = payload.face_attrs(f.attrs) {
                mesh.face_mut(id).attrs.update(&a);
            }
        }
        soften_smooth_edges(&mut mesh, 0.85);
        return mesh;
    };

    let mut flat: Vec<DVec3> = Vec::new();
    let mut ring_sizes: Vec<usize> = Vec::new();
    let mut ring_counts: Vec<usize> = Vec::new();
    let mut attrs_list: Vec<Option<FaceAttrs>> = Vec::new();
    for f in faces {
        let mut holes = f.holes.clone();
        if holes.len() > 1 {
            // A hole ring of fewer than three corners skips the face.
            if holes.iter().any(|h| h.len() < 3) {
                continue;
            }
            holes = maximal_holes(&holes);
        }
        ring_counts.push(1 + holes.len());
        for ring in std::iter::once(&f.outer).chain(holes.iter()) {
            ring_sizes.push(ring.len());
            flat.extend_from_slice(ring);
        }
        attrs_list.push(payload.face_attrs(f.attrs));
    }
    if !ring_counts.is_empty() {
        if flat.len() >= 1024 {
            let (vobjs, ids) = mesh.bulk_weld(&flat);
            mesh.add_faces_welded(&vobjs, &ids, &ring_sizes, &ring_counts, &attrs_list);
        } else {
            // A small group: the plain per-face walk.
            let mut ci = 0;
            let mut ri = 0;
            for (fi, &nr) in ring_counts.iter().enumerate() {
                let mut rings: Vec<Vec<DVec3>> = Vec::with_capacity(nr);
                for k in 0..nr {
                    let n = ring_sizes[ri + k];
                    rings.push(flat[ci..ci + n].to_vec());
                    ci += n;
                }
                ri += nr;
                let id = mesh.add_face(&rings[0], &rings[1..]);
                if let Some(a) = attrs_list[fi].as_ref().filter(|a| !a.is_empty()) {
                    mesh.face_mut(id).attrs.update(a);
                }
            }
        }
    }
    // Soften through the weld itself: each flagged segment resolves to its
    // welded vertices, so float noise can never drop a flag.
    for &(a, b) in soft_edges {
        let (Some(va), Some(vb)) = (mesh.vertex_at(a), mesh.vertex_at(b)) else {
            continue;
        };
        if va == vb {
            continue;
        }
        if let Some(e) = mesh.find_edge(va, vb) {
            mesh.edge_mut(e).soft = true;
        }
    }
    mesh
}

/// `QVector3D::normalize()` (in place): left as it is when its squared
/// length is fuzzily 1 or fuzzily 0.
fn qt_normalize(v: DVec3) -> DVec3 {
    let len2 = v.length_squared();
    if (len2 - 1.0).abs() <= 1e-5 || len2 <= 1e-5 {
        v
    } else {
        v / len2.sqrt()
    }
}

/// A group named `name`, or "Group N" when the name is empty.
fn new_group(mesh: MeshId, name: Option<&str>) -> Group {
    Group::new(mesh, name.filter(|n| !n.is_empty()))
}

/// `apply_payload`: add a parsed payload to `scene` — materials into the
/// registry, layers, saved views, dimensions, texts, then the groups and
/// shared prototypes. The caller wraps it in one undo step. Returns the
/// backend name.
pub fn apply_payload(scene: &mut Scene, payload: &SkpPayload) -> Result<String, String> {
    let mut payload = payload.clone();

    // The file's named materials join the registry: an identical
    // re-import merges, a name taken by a DIFFERENT recipe lands as
    // "name (2)" and the faces follow the rename (each shared attrs entry
    // rewritten once).
    if !payload.materials.is_empty() {
        let mut remap: Vec<(String, String)> = Vec::new();
        for mat in &payload.materials {
            if mat.name.is_empty() {
                continue;
            }
            let fin = register(&mut scene.doc.materials, mat.clone());
            if fin != mat.name {
                remap.push((mat.name.clone(), fin));
            }
        }
        if !remap.is_empty() {
            let mut seen = vec![false; payload.attrs.len()];
            let used = payload
                .groups
                .iter()
                .flat_map(|g| &g.faces)
                .chain(payload.protos.iter().flat_map(|p| &p.faces))
                .filter_map(|f| f.attrs);
            for i in used {
                let Some(done) = seen.get_mut(i) else {
                    continue;
                };
                if *done {
                    continue;
                }
                *done = true;
                let entry = &mut payload.attrs[i];
                let renamed = entry
                    .paint
                    .mat
                    .as_ref()
                    .and_then(|m| remap.iter().find(|(from, _)| from == m))
                    .map(|(_, to)| to.clone());
                if let Some(to) = renamed {
                    entry.paint.mat = Some(to);
                }
            }
        }
    }

    // The file's layers join the scene's, with their visibility; a layer
    // already there is left as the user set it.
    for raw in &payload.layers {
        if !raw.name.is_empty() && !scene.doc.layers.iter().any(|l| l.name == raw.name) {
            let mut layer = Layer::new(&raw.name);
            layer.visible = raw.visible;
            scene.doc.layers.push(layer);
        }
    }

    // The file's scenes become saved views, once per name.
    for raw in &payload.scenes {
        if scene.saved_views.iter().any(|v| v.name == raw.name) {
            continue;
        }
        let view = from_lookat(
            &raw.name,
            LookAt {
                eye: raw.eye,
                target: raw.target,
                up: raw.up.unwrap_or([0.0, 0.0, 1.0]),
                fov_deg: raw.fov.unwrap_or(45.0),
                perspective: !raw.parallel,
                ortho_height: raw.ortho_height.filter(|h| *h != 0.0),
            },
            raw.hidden_layers.clone(),
        );
        scene.saved_views.push(view);
    }

    // Linear dimensions: the .skp offset distance becomes the in-plane
    // perpendicular × distance.
    for raw in &payload.dimensions {
        let a = DVec3::from(raw.a);
        let b = DVec3::from(raw.b);
        let seg = b - a;
        if seg.length() < 1e-9 {
            continue;
        }
        let n = raw.normal.map_or(DVec3::Z, DVec3::from);
        let mut perp = n.cross(seg);
        if perp.length() < 1e-9 {
            perp = DVec3::Z.cross(seg);
        }
        if perp.length() < 1e-9 {
            perp = DVec3::X.cross(seg);
        }
        let perp = qt_normalize(perp);
        scene
            .doc
            .dimensions
            .push(Dimension::new(a, b, perp * raw.offset));
    }

    // Leader texts: the label floats at its own point, or at the anchor.
    for raw in &payload.texts {
        if raw.hidden {
            continue;
        }
        let anchor = DVec3::from(raw.anchor);
        let label = raw.label.map_or(anchor, DVec3::from);
        scene
            .doc
            .text_labels
            .push(TextLabel::new(anchor, label - anchor, &raw.text));
    }

    for gp in &payload.groups {
        let mesh = build_mesh(&payload, &gp.faces, gp.soft_edges.as_deref());
        if mesh.faces.is_empty() {
            continue;
        }
        let id = scene.doc.add_mesh(mesh);
        let mut g = new_group(id, gp.name.as_deref());
        if gp.layer.as_deref().is_some_and(|l| !l.is_empty()) {
            g.layer = gp.layer.clone();
        }
        if let Some(axes) = gp.axes.filter(|m| *m != DMat4::IDENTITY) {
            g.axes = Some(axes);
        }
        scene.doc.groups.push(g);
    }

    // Shared prototypes: one mesh each (added on first use), one Group per
    // placement; a prototype's own placements become its children.
    let mut meshes: Vec<Option<Mesh>> = payload
        .protos
        .iter()
        .map(|pr| Some(build_mesh(&payload, &pr.faces, pr.soft_edges.as_deref())))
        .collect();
    let has_faces: Vec<bool> = meshes
        .iter()
        .map(|m| m.as_ref().is_some_and(|m| !m.faces.is_empty()))
        .collect();
    let mut ids: Vec<Option<MeshId>> = vec![None; meshes.len()];
    let mut placer = Placer {
        protos: &payload.protos,
        meshes: &mut meshes,
        ids: &mut ids,
        auto_names: HashMap::new(),
        scene,
    };
    let mut top: Vec<Group> = Vec::new();
    for (idx, pr) in payload.protos.iter().enumerate() {
        if !has_faces[idx] && pr.children.is_empty() {
            continue;
        }
        let layers = pr.instance_layers.as_deref().unwrap_or_default();
        for (i, xf) in pr.instances.iter().enumerate() {
            let layer = layers.get(i).cloned().flatten();
            top.push(placer.place(idx, *xf, layer, 0, None, None)?);
        }
    }
    // A tree import: the top-level instances in file order.
    for usep in &payload.placements {
        top.push(placer.place(
            usep.proto,
            usep.xform,
            usep.layer.clone(),
            0,
            Some(usep),
            None,
        )?);
    }
    // Only the new groups: the document's own already keep the invariant,
    // and one may be open for editing.
    split_divergent_copies(&mut scene.doc.meshes, &mut top);
    scene.doc.groups.extend(top);

    if let Some(back) = payload.back_color {
        if scene.back_face_color.is_none() {
            scene.back_face_color = Some(back);
        }
    }
    // The groups are in: dimensions hold their vertices.
    let mut dims = std::mem::take(&mut scene.doc.dimensions);
    for d in dims.iter_mut() {
        scene.bind_dimension(d);
    }
    scene.doc.dimensions = dims;
    scene.version += 1;
    Ok(payload.backend.clone())
}

/// Places the payload, with the prototype meshes entering the document on
/// first use.
struct Placer<'p, 's> {
    protos: &'p [Proto],
    meshes: &'p mut Vec<Option<Mesh>>,
    ids: &'p mut Vec<Option<MeshId>>,
    /// The "Group N" of each unnamed child placement, by (prototype, child
    /// index): every copy of a prototype gives that child the same name, so
    /// the copies keep their contents alike and stay one component.
    auto_names: HashMap<(usize, usize), String>,
    scene: &'s mut Scene,
}

impl Placer<'_, '_> {
    fn mesh_id(&mut self, idx: usize) -> Result<MeshId, String> {
        if let Some(id) = self.ids.get(idx).copied().flatten() {
            return Ok(id);
        }
        let mesh = self
            .meshes
            .get_mut(idx)
            .and_then(Option::take)
            .ok_or_else(|| format!("no prototype {idx}"))?;
        let id = self.scene.doc.add_mesh(mesh);
        self.ids[idx] = Some(id);
        Ok(id)
    }

    /// One placement of prototype `idx`. `usep` is the placement's own
    /// record when the file's instance tree was kept; `slot` is the
    /// (prototype, child index) a nested placement fills.
    fn place(
        &mut self,
        idx: usize,
        xf: DMat4,
        layer: Option<String>,
        depth: usize,
        usep: Option<&Placement>,
        slot: Option<(usize, usize)>,
    ) -> Result<Group, String> {
        let protos = self.protos;
        let pr = protos
            .get(idx)
            .ok_or_else(|| format!("no prototype {idx}"))?;
        let mesh = self.mesh_id(idx)?;
        let name = match usep.and_then(|u| u.name.as_ref()) {
            Some(own) => own.clone(),
            None => pr.name.clone(),
        }
        .filter(|n| !n.is_empty())
        .or_else(|| {
            slot.map(|s| {
                self.auto_names
                    .entry(s)
                    .or_insert_with(next_group_name)
                    .clone()
            })
        });
        let mut g = new_group(mesh, name.as_deref());
        g.xform = Some(xf);
        if layer.as_deref().is_some_and(|l| !l.is_empty()) {
            g.layer = layer;
        }
        if usep.is_some_and(|u| u.hidden) {
            g.hidden = true;
        }
        if let Some(component) = usep.and_then(|u| u.component) {
            g.component = component;
        }
        // A malformed file must not spin.
        if depth < 32 {
            let mut kids = Vec::with_capacity(pr.children.len());
            for (i, kid) in pr.children.iter().enumerate() {
                kids.push(self.place(
                    kid.proto,
                    kid.xform,
                    kid.layer.clone(),
                    depth + 1,
                    Some(kid),
                    Some((idx, i)),
                )?);
            }
            g.adopt(kids);
        }
        Ok(g)
    }
}
