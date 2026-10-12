// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Attribute commands: paint, layers, hide, material registry, purge, and
//! the annotation objects (dimensions, texts, guides, section planes).

use serde_json::Value;
use xtrude_core::attrs::{Back, Paint, Rgb};
use xtrude_core::math::{DVec3, MatExt, VecExt};
use xtrude_core::mesh::FaceId;

use crate::dimension::Dimension;
use crate::entity::EntityRef;
use crate::guide::Guide;
use crate::history::{CmdResult, Command};
use crate::ids::{GroupId, MeshId, ObjId};
use crate::layers::{label_for, layer_name, Layer, DEFAULT_LAYER};
use crate::materials::Material;
use crate::scene::Scene;
use crate::section::SectionPlane;
use crate::textlabel::TextLabel;

use super::mesh::rotation_matrix;

/// Replace one extension's document data (`None` removes the key).
pub struct SetPluginDataCommand {
    pub key: String,
    pub value: Option<Value>,
}

impl Command for SetPluginDataCommand {
    fn name(&self) -> &str {
        "SetPluginDataCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        match &self.value {
            None => {
                scene.doc.plugin_data.remove(&self.key);
            }
            Some(v) => {
                scene.doc.plugin_data.insert(self.key.clone(), v.clone());
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Faces addressed by mesh.
pub type FaceRefs = Vec<(MeshId, FaceId)>;

fn each_face(
    scene: &mut Scene,
    faces: &FaceRefs,
    mut f: impl FnMut(&mut xtrude_core::attrs::FaceAttrs),
) {
    for &(mid, fid) in faces {
        let m = scene.doc.mesh_mut(mid);
        if m.has_face(fid) {
            f(&mut m.face_mut(fid).attrs);
        }
    }
}

/// Paint a set of faces with an RGB colour (or clear it with `None`).
pub struct SetFaceColorCommand {
    pub faces: FaceRefs,
    pub color: Option<Rgb>,
}

impl Command for SetFaceColorCommand {
    fn name(&self) -> &str {
        "SetFaceColorCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let c = self.color;
        each_face(scene, &self.faces, |a| a.paint.color = c);
        scene.version += 1;
        Ok(())
    }
}

/// Set (or clear) the translucency of a set of faces.
pub struct SetFaceOpacityCommand {
    pub faces: FaceRefs,
    pub opacity: Option<f64>,
}

impl Command for SetFaceOpacityCommand {
    fn name(&self) -> &str {
        "SetFaceOpacityCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let o = self.opacity;
        each_face(scene, &self.faces, |a| a.paint.opacity = o);
        scene.version += 1;
        Ok(())
    }
}

/// Set (or clear) the registry identity `mat` on a set of faces; registers
/// the material when given and absent.
pub struct SetFaceMaterialTagCommand {
    pub faces: FaceRefs,
    pub name: Option<String>,
    pub material: Option<Material>,
}

impl Command for SetFaceMaterialTagCommand {
    fn name(&self) -> &str {
        "SetFaceMaterialTagCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let (Some(m), Some(n)) = (&self.material, &self.name) {
            if !n.is_empty() && !scene.doc.materials.contains_key(n) {
                scene.doc.materials.insert(n.clone(), m.clone());
            }
        }
        let name = self.name.clone();
        each_face(scene, &self.faces, |a| a.paint.mat = name.clone());
        scene.version += 1;
        Ok(())
    }
}

/// Paint the BACK side of faces (`None` = the style's default back).
pub struct SetFaceBackCommand {
    pub faces: FaceRefs,
    pub backs: Vec<Option<Back>>,
}

impl SetFaceBackCommand {
    pub fn uniform(faces: FaceRefs, back: Option<Back>) -> Self {
        let n = faces.len();
        SetFaceBackCommand {
            faces,
            backs: vec![back; n],
        }
    }
}

impl Command for SetFaceBackCommand {
    fn name(&self) -> &str {
        "SetFaceBackCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        for (i, &(mid, fid)) in self.faces.iter().enumerate() {
            let back = self.backs.get(i).cloned().flatten();
            let m = scene.doc.mesh_mut(mid);
            if m.has_face(fid) {
                m.face_mut(fid).attrs.back = back;
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Edit a registry material and RESTAMP every face (and every container
/// painted whole) that wears it.
pub struct RestampMaterialCommand {
    pub name: String,
    pub new: Material,
}

impl Command for RestampMaterialCommand {
    fn name(&self) -> &str {
        "RestampMaterialCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let stamp = self.new.face_paint();
        scene
            .doc
            .materials
            .insert(self.name.clone(), self.new.clone());
        let mut meshes = vec![MeshId::LOOSE];
        for g in scene.doc.all_groups() {
            if !meshes.contains(&g.mesh) {
                meshes.push(g.mesh);
            }
        }
        for mid in meshes {
            let targets: Vec<FaceId> = scene
                .doc
                .mesh(mid)
                .faces
                .iter()
                .filter(|(_, f)| f.attrs.paint.mat.as_deref() == Some(self.name.as_str()))
                .map(|(id, _)| id)
                .collect();
            if targets.is_empty() {
                continue;
            }
            let m = scene.doc.mesh_mut(mid);
            for f in targets {
                let a = &mut m.face_mut(f).attrs;
                a.paint.color = stamp.color;
                a.paint.opacity = stamp.opacity;
            }
        }
        let painted: Vec<GroupId> = scene
            .doc
            .all_groups()
            .into_iter()
            .filter(|g| {
                g.material.as_ref().and_then(|p| p.mat.as_deref()) == Some(self.name.as_str())
            })
            .map(|g| g.id)
            .collect();
        for gid in painted {
            if let Some(g) = scene.doc.find_group_mut(gid) {
                g.material = Some(stamp.clone());
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Purge Unused, for layers and/or materials. What goes is decided once.
pub struct PurgeUnusedCommand {
    pub layers: bool,
    pub materials: bool,
    surveyed: Option<(Vec<String>, Vec<String>)>,
}

impl PurgeUnusedCommand {
    pub fn new(layers: bool, materials: bool) -> Self {
        PurgeUnusedCommand {
            layers,
            materials,
            surveyed: None,
        }
    }

    fn survey(&mut self, scene: &Scene) {
        if self.surveyed.is_none() {
            let ly = if self.layers {
                crate::purge::unused_layers(scene)
            } else {
                Vec::new()
            };
            let mt = if self.materials {
                crate::purge::unused_materials(scene)
            } else {
                Vec::new()
            };
            self.surveyed = Some((ly, mt));
        }
    }

    /// The names a run would remove — `(layers, materials)`.
    pub fn names(&mut self, scene: &Scene) -> (Vec<String>, Vec<String>) {
        self.survey(scene);
        self.surveyed.clone().unwrap_or_default()
    }

    /// What a run would remove — `(layers, materials)`.
    pub fn counts(&mut self, scene: &Scene) -> (usize, usize) {
        self.survey(scene);
        let (l, m) = self
            .surveyed
            .as_ref()
            .map_or((0, 0), |(l, m)| (l.len(), m.len()));
        (l, m)
    }
}

impl Command for PurgeUnusedCommand {
    fn name(&self) -> &str {
        "PurgeUnusedCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        self.survey(scene);
        if let Some((ly, mt)) = &self.surveyed {
            scene.doc.layers.retain(|l| !ly.contains(&l.name));
            for n in mt {
                scene.doc.materials.shift_remove(n);
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Move entities onto a layer (created when it does not exist yet).
pub struct AssignLayerCommand {
    pub entities: Vec<EntityRef>,
    pub layer: String,
}

impl Command for AssignLayerCommand {
    fn name(&self) -> &str {
        "AssignLayerCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if scene.doc.layer(&self.layer).is_none() {
            scene.doc.layers.push(Layer::new(&self.layer));
        }
        let label = label_for(&self.layer);
        for e in &self.entities {
            assign_layer(scene, e, label.clone());
        }
        scene.version += 1;
        Ok(())
    }
}

/// Label one entity with a layer (`None` = the default layer).
pub fn assign_layer(scene: &mut Scene, e: &EntityRef, label: Option<String>) {
    // the default layer is no tag at all
    let label = label.filter(|n| n != crate::layers::DEFAULT_LAYER);
    match *e {
        EntityRef::Face(m, f) => {
            let mesh = scene.doc.mesh_mut(m);
            if mesh.has_face(f) {
                mesh.face_mut(f).attrs.layer = label;
            }
        }
        EntityRef::Edge(m, ed) => {
            let mesh = scene.doc.mesh_mut(m);
            if mesh.has_edge(ed) {
                mesh.edge_mut(ed).layer = label;
            }
        }
        EntityRef::Group(g) => {
            if let Some(g) = scene.doc.find_group_mut(g) {
                g.layer = label;
            }
        }
        EntityRef::Dimension(d) => {
            if let Some(d) = scene.doc.dimensions.iter_mut().find(|x| x.id == d) {
                d.layer = label;
            }
        }
        EntityRef::TextLabel(t) => {
            if let Some(t) = scene.doc.text_labels.iter_mut().find(|x| x.id == t) {
                t.layer = label;
            }
        }
        _ => {}
    }
}

/// The layer name an entity carries (default when unlabelled).
pub fn layer_of(scene: &Scene, e: &EntityRef) -> String {
    layer_name(scene.entity_layer(e).as_deref()).to_string()
}

/// Hide (or unhide) objects, faces and edges.
pub struct HideCommand {
    pub entities: Vec<EntityRef>,
    pub hidden: bool,
}

impl Command for HideCommand {
    fn name(&self) -> &str {
        "HideCommand"
    }

    fn hides(&self) -> Vec<EntityRef> {
        if self.hidden {
            self.entities.clone()
        } else {
            Vec::new()
        }
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        for e in self.entities.clone() {
            match e {
                EntityRef::Face(m, f) => {
                    let mesh = scene.doc.mesh_mut(m);
                    if mesh.has_face(f) {
                        mesh.face_mut(f).attrs.hidden = self.hidden;
                    }
                }
                EntityRef::Edge(m, ed) => {
                    let mesh = scene.doc.mesh_mut(m);
                    if mesh.has_edge(ed) {
                        mesh.edge_mut(ed).hidden = self.hidden;
                    }
                }
                EntityRef::Group(g) => {
                    if let Some(g) = scene.doc.find_group_mut(g) {
                        g.hidden = self.hidden;
                    }
                }
                _ => continue,
            }
            if self.hidden {
                scene.discard(&e);
            }
        }
        scene.version += 1;
        Ok(())
    }
}

// ---- Annotations --------------------------------------------------------------

/// Add a dimension, anchored to the vertices under its endpoints.
pub struct AddDimensionCommand {
    pub dimension: Option<Dimension>,
    pub id: ObjId,
}

impl AddDimensionCommand {
    pub fn new(d: Dimension) -> Self {
        let id = d.id;
        AddDimensionCommand {
            dimension: Some(d),
            id,
        }
    }
}

impl Command for AddDimensionCommand {
    fn name(&self) -> &str {
        "AddDimensionCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(mut d) = self.dimension.take() {
            scene.bind_dimension(&mut d);
            scene.doc.dimensions.push(d);
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct DeleteDimensionsCommand {
    pub ids: Vec<ObjId>,
}

impl Command for DeleteDimensionsCommand {
    fn name(&self) -> &str {
        "DeleteDimensionsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.doc.dimensions.retain(|d| !self.ids.contains(&d.id));
        for &id in &self.ids {
            scene.discard(&EntityRef::Dimension(id));
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct AddTextLabelCommand {
    pub label: Option<TextLabel>,
}

impl Command for AddTextLabelCommand {
    fn name(&self) -> &str {
        "AddTextLabelCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(l) = self.label.take() {
            scene.doc.text_labels.push(l);
        }
        scene.version += 1;
        Ok(())
    }
}

/// Translate the floating label of leader texts (the anchor stays).
pub struct MoveTextLabelsCommand {
    pub ids: Vec<ObjId>,
    pub delta: DVec3,
}

impl Command for MoveTextLabelsCommand {
    fn name(&self) -> &str {
        "MoveTextLabelsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        for t in scene.doc.text_labels.iter_mut() {
            if self.ids.contains(&t.id) {
                t.offset += self.delta;
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// How the Move tool moves a dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimMoveMode {
    /// Anchored at both ends: slide the LINE (parallel), endpoints hold.
    Line,
    /// A free endpoint moves rigidly.
    Rigid,
}

/// Apply one step of a dimension move (the tool's preview uses it too).
pub fn shift_dimension(scene: &Scene, dim: &mut Dimension, mode: DimMoveMode, step: DVec3) {
    let dv = scene.doc_view();
    match mode {
        DimMoveMode::Line => {
            let ab = dim.b(&dv) - dim.a(&dv);
            let length = ab.length();
            let mut s = step;
            if length > 1e-9 {
                let d = ab / length;
                s = step - d * step.dot(d);
            }
            dim.offset += s;
        }
        DimMoveMode::Rigid => {
            if dim.anchor_a.is_none() {
                dim.a0 += step;
            }
            if dim.anchor_b.is_none() {
                dim.b0 += step;
            }
        }
    }
}

pub struct MoveDimensionsCommand {
    pub modes: Vec<(ObjId, DimMoveMode)>,
    pub delta: DVec3,
}

impl Command for MoveDimensionsCommand {
    fn name(&self) -> &str {
        "MoveDimensionsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mut dims = std::mem::take(&mut scene.doc.dimensions);
        for d in dims.iter_mut() {
            if let Some(&(_, mode)) = self.modes.iter().find(|(id, _)| *id == d.id) {
                shift_dimension(scene, d, mode, self.delta);
            }
        }
        scene.doc.dimensions = dims;
        scene.version += 1;
        Ok(())
    }
}

pub struct EditTextLabelCommand {
    pub id: ObjId,
    pub text: String,
}

impl Command for EditTextLabelCommand {
    fn name(&self) -> &str {
        "EditTextLabelCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(t) = scene.doc.text_labels.iter_mut().find(|t| t.id == self.id) {
            t.text = self.text.clone();
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct EditDimensionTextCommand {
    pub id: ObjId,
    pub text: Option<String>,
}

impl Command for EditDimensionTextCommand {
    fn name(&self) -> &str {
        "EditDimensionTextCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(d) = scene.doc.dimensions.iter_mut().find(|d| d.id == self.id) {
            d.text = self.text.clone().filter(|t| !t.is_empty());
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct DeleteTextLabelsCommand {
    pub ids: Vec<ObjId>,
}

impl Command for DeleteTextLabelsCommand {
    fn name(&self) -> &str {
        "DeleteTextLabelsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.doc.text_labels.retain(|t| !self.ids.contains(&t.id));
        for &id in &self.ids {
            scene.discard(&EntityRef::TextLabel(id));
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct AddGuideCommand {
    pub guide: Option<Guide>,
    pub id: ObjId,
}

impl AddGuideCommand {
    pub fn new(g: Guide) -> Self {
        let id = g.id;
        AddGuideCommand { guide: Some(g), id }
    }
}

impl Command for AddGuideCommand {
    fn name(&self) -> &str {
        "AddGuideCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(g) = self.guide.take() {
            scene.doc.guides.push(g);
        }
        scene.version += 1;
        Ok(())
    }
}

/// Re-aim an existing guide line.
pub struct ChangeGuideCommand {
    pub id: ObjId,
    pub direction: DVec3,
}

impl Command for ChangeGuideCommand {
    fn name(&self) -> &str {
        "ChangeGuideCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(g) = scene.doc.guides.iter_mut().find(|g| g.id == self.id) {
            g.direction = Some(self.direction.unit_or_zero());
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct DeleteGuidesCommand {
    pub ids: Vec<ObjId>,
}

impl Command for DeleteGuidesCommand {
    fn name(&self) -> &str {
        "DeleteGuidesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.doc.guides.retain(|g| !self.ids.contains(&g.id));
        for &id in &self.ids {
            scene.discard(&EntityRef::Guide(id));
        }
        scene.version += 1;
        Ok(())
    }
}

/// Place a section plane; it becomes the ACTIVE cut and is selected.
pub struct PlaceSectionPlaneCommand {
    pub plane: Option<SectionPlane>,
    pub id: ObjId,
}

impl PlaceSectionPlaneCommand {
    pub fn new(p: SectionPlane) -> Self {
        let id = p.id;
        PlaceSectionPlaneCommand { plane: Some(p), id }
    }
}

impl Command for PlaceSectionPlaneCommand {
    fn name(&self) -> &str {
        "PlaceSectionPlaneCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(p) = self.plane.take() {
            scene.doc.section_planes.push(p);
        }
        scene.set_active_section(Some(self.id));
        scene.selection.clear();
        scene.selection.insert(EntityRef::SectionPlane(self.id));
        scene.version += 1;
        Ok(())
    }
}

pub struct DeleteSectionPlanesCommand {
    pub ids: Vec<ObjId>,
}

impl Command for DeleteSectionPlanesCommand {
    fn name(&self) -> &str {
        "DeleteSectionPlanesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene
            .doc
            .section_planes
            .retain(|p| !self.ids.contains(&p.id));
        for &id in &self.ids {
            scene.discard(&EntityRef::SectionPlane(id));
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct ReverseSectionPlaneCommand {
    pub id: ObjId,
}

impl Command for ReverseSectionPlaneCommand {
    fn name(&self) -> &str {
        "ReverseSectionPlaneCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if let Some(p) = scene
            .doc
            .section_planes
            .iter_mut()
            .find(|p| p.id == self.id)
        {
            p.flip();
        }
        scene.version += 1;
        Ok(())
    }
}

pub struct SetActiveSectionCommand {
    pub id: Option<ObjId>,
}

impl Command for SetActiveSectionCommand {
    fn name(&self) -> &str {
        "SetActiveSectionCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.set_active_section(self.id);
        scene.version += 1;
        Ok(())
    }
}

pub struct MoveSectionPlanesCommand {
    pub ids: Vec<ObjId>,
    pub delta: DVec3,
}

impl Command for MoveSectionPlanesCommand {
    fn name(&self) -> &str {
        "MoveSectionPlanesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        for p in scene.doc.section_planes.iter_mut() {
            if self.ids.contains(&p.id) {
                p.point += self.delta;
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// Rotate section planes about `centre`/`axis`.
pub struct RotateSectionPlanesCommand {
    pub ids: Vec<ObjId>,
    pub centre: DVec3,
    pub axis: DVec3,
    pub deg: f64,
}

impl Command for RotateSectionPlanesCommand {
    fn name(&self) -> &str {
        "RotateSectionPlanesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let m = rotation_matrix(self.centre, self.axis, self.deg);
        for p in scene.doc.section_planes.iter_mut() {
            if self.ids.contains(&p.id) {
                p.point = m.project_point(p.point);
                let n = m.transform_vector(p.normal);
                if n.length() > 1e-12 {
                    p.normal = n.unit_or_zero();
                }
            }
        }
        scene.version += 1;
        Ok(())
    }
}

/// The default layer's name (re-export for callers).
pub const DEFAULT: &str = DEFAULT_LAYER;

/// A paint dict from a material (container paint).
pub fn paint_of(m: &Material) -> Paint {
    m.face_paint()
}
