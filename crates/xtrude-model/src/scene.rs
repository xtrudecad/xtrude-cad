// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Scene container: the loose mesh, groups, layers, materials, guides,
//! dimensions, saved views, units — plus the session state around them
//! (selection, the group-edit context, view toggles).
//!
//! Design notes:
//! - The undoable document is the [`Doc`] (cheap to clone: meshes are
//!   copy-on-write `Arc`s). `History` snapshots it per command.
//! - `scene.mesh` pointing at the edited group's mesh becomes
//!   [`Scene::active_mesh_id`], derived from the edit stack.
//! - `version` bumps on every mutation; caches key on it.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Value};
use xtrude_core::math::{DMat4, DVec3, MatExt};
use xtrude_core::mesh::Mesh;

use crate::dimension::{Dimension, DimensionStyle, DocView};
use crate::entity::EntityRef;
use crate::group::{add_mesh, group_frame, iter_placements, transformed_mesh, Group, MeshTable};
use crate::guide::Guide;
use crate::ids::{GroupId, MeshId, ObjId};
use crate::layers::{layer_name, Layer, DEFAULT_LAYER};
use crate::materials::Material;
use crate::saved_views::{CameraHome, SavedView};
use crate::section::SectionPlane;
use crate::style::Style;
use crate::textlabel::TextLabel;
use crate::units::{self, Units};

/// The undoable document state.
#[derive(Clone)]
pub struct Doc {
    pub meshes: MeshTable,
    pub groups: Vec<Group>,
    pub dimensions: Vec<Dimension>,
    pub text_labels: Vec<TextLabel>,
    pub guides: Vec<Guide>,
    pub layers: Vec<Layer>,
    pub materials: IndexMap<String, Material>,
    pub section_planes: Vec<SectionPlane>,
    /// Extensions' own document data (kept, saved, never read).
    pub plugin_data: Map<String, Value>,
}

impl Default for Doc {
    fn default() -> Self {
        let mut meshes = HashMap::new();
        meshes.insert(MeshId::LOOSE, Arc::new(Mesh::new()));
        Doc {
            meshes,
            groups: Vec::new(),
            dimensions: Vec::new(),
            text_labels: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer::new(DEFAULT_LAYER)],
            materials: IndexMap::new(),
            section_planes: Vec::new(),
            plugin_data: Map::new(),
        }
    }
}

impl Doc {
    pub fn mesh(&self, id: MeshId) -> &Mesh {
        static EMPTY: std::sync::OnceLock<Mesh> = std::sync::OnceLock::new();
        self.meshes
            .get(&id)
            .map(|m| &**m)
            .unwrap_or_else(|| EMPTY.get_or_init(Mesh::new))
    }

    /// Mutable access to a mesh (copy-on-write when a snapshot shares it).
    pub fn mesh_mut(&mut self, id: MeshId) -> &mut Mesh {
        let arc = self
            .meshes
            .entry(id)
            .or_insert_with(|| Arc::new(Mesh::new()));
        Arc::make_mut(arc)
    }

    pub fn add_mesh(&mut self, mesh: Mesh) -> MeshId {
        add_mesh(&mut self.meshes, mesh)
    }

    pub fn find_group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find_map(|g| g.find(id))
    }

    pub fn find_group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find_map(|g| g.find_mut(id))
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name == name)
    }

    pub fn layer_mut(&mut self, name: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.name == name)
    }

    /// Every group, nested placements included, each once (depth first).
    pub fn all_groups(&self) -> Vec<&Group> {
        let mut out = Vec::new();
        for g in &self.groups {
            g.walk(&mut out);
        }
        out
    }

    /// `uid → group id` over the whole tree.
    pub fn groups_by_uid(&self) -> IndexMap<String, GroupId> {
        self.all_groups()
            .into_iter()
            .map(|g| (g.uid.clone(), g.id))
            .collect()
    }

    /// The list `group` lives in: the top level, or its container's
    /// children. Returns the parent id (`None` for the top level).
    pub fn parent_of(&self, id: GroupId) -> Option<Option<GroupId>> {
        if self.groups.iter().any(|g| g.id == id) {
            return Some(None);
        }
        for g in self.all_groups() {
            if g.children.iter().any(|c| c.id == id) {
                return Some(Some(g.id));
            }
        }
        None
    }

    /// The sibling list a parent id names (`None` = the top level).
    pub fn siblings_mut(&mut self, parent: Option<GroupId>) -> Option<&mut Vec<Group>> {
        match parent {
            None => Some(&mut self.groups),
            Some(p) => self.find_group_mut(p).map(|g| &mut g.children),
        }
    }

    /// Remove a group from wherever it lives; returns it with its index.
    pub fn take_group(&mut self, id: GroupId) -> Option<(Option<GroupId>, usize, Group)> {
        let parent = self.parent_of(id)?;
        let list = self.siblings_mut(parent)?;
        let idx = list.iter().position(|g| g.id == id)?;
        Some((parent, idx, list.remove(idx)))
    }

    pub fn doc_view(&self, loose: MeshId) -> DocView<'_> {
        DocView {
            meshes: &self.meshes,
            groups: &self.groups,
            loose,
        }
    }

    pub fn dimension(&self, id: ObjId) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.id == id)
    }
}

/// One open group-edit level.
#[derive(Clone, Debug)]
pub struct EditLevel {
    pub group: GroupId,
    /// The mesh the context pointed at before entering.
    pub prev_mesh: MeshId,
    /// The open copy's pending share-back, if it is a copy of a
    /// definition.
    pub share: Option<EditShare>,
    /// The context's own axes.
    pub frame: Option<DMat4>,
    /// The history's length when the level opened, set by whoever keeps
    /// the history: the session's steps fold into the level's share-back.
    pub undo_mark: usize,
}

/// An open copy of a definition, to write back on leaving.
#[derive(Clone, Debug)]
pub struct EditShare {
    pub group: GroupId,
    pub proto: MeshId,
    pub xform: DMat4,
    /// The temp mesh at entry (to tell an edit from a look).
    pub state0: Arc<Mesh>,
    /// A container's contents on entering; `None` for a copy without
    /// children.
    pub contents: Option<EntryContents>,
}

/// What an open container held on entering, in its local coordinates.
#[derive(Clone, Debug)]
pub struct EntryContents {
    pub children: Vec<Group>,
    pub axes: Option<DMat4>,
    /// A component copy, whose edits reach its copies; otherwise a group
    /// inside an open component, whose edits reach the copies through it.
    pub component: bool,
}

impl EditShare {
    /// Put the open copy back as it was on entering, in `doc` (the
    /// `before` state of the session's undo step).
    pub fn restore_entry(&self, doc: &mut Doc) {
        if let Some(b) = doc.find_group_mut(self.group) {
            b.mesh = self.proto;
            b.xform = Some(self.xform);
            if let Some(c) = &self.contents {
                b.children = c.children.clone();
                b.axes = c.axes;
            }
        }
    }
}

/// How a click combines with the current selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Replace,
    Add,
    Toggle,
    Remove,
}

pub struct Scene {
    pub doc: Doc,
    pub selection: IndexSet<EntityRef>,
    pub version: u64,
    /// How many of `version`'s bumps changed only what is SHOWN.
    pub view_version: u64,
    pub saved_views: Vec<SavedView>,
    pub camera_home: Option<CameraHome>,
    pub units: Units,
    pub dimension_style: DimensionStyle,
    pub back_face_color: Option<[f64; 3]>,
    pub display_style: Style,
    pub show_section_planes: bool,
    pub show_section_cuts: bool,
    pub show_hidden_objects: bool,
    pub show_hidden_geometry: bool,
    /// The open group-edit contexts, outermost first.
    pub edit_stack: Vec<EditLevel>,
    /// Entities left out of the last load (non-finite coordinates).
    pub load_repairs: usize,
    bounds_cache: Option<(u64, Option<(DVec3, DVec3)>)>,
}

impl Deref for Scene {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.doc
    }
}

impl DerefMut for Scene {
    fn deref_mut(&mut self) -> &mut Doc {
        &mut self.doc
    }
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new()
    }
}

impl Scene {
    pub fn new() -> Scene {
        let u = units::default_model_units();
        units::bind(&u);
        Scene {
            doc: Doc::default(),
            selection: IndexSet::new(),
            version: 0,
            view_version: 0,
            saved_views: Vec::new(),
            camera_home: None,
            units: u,
            dimension_style: DimensionStyle::default(),
            back_face_color: None,
            display_style: Style::default(),
            show_section_planes: true,
            show_section_cuts: true,
            show_hidden_objects: false,
            show_hidden_geometry: false,
            edit_stack: Vec::new(),
            load_repairs: 0,
            bounds_cache: None,
        }
    }

    // ---- Units ----------------------------------------------------------------
    /// Give the scene these units; the dimension style follows them.
    pub fn apply_units(&mut self, u: &Units) {
        let chosen = units::validated(u);
        self.dimension_style.units = chosen.length.clone();
        self.dimension_style.decimals = chosen.precision;
        self.units = chosen;
        units::bind(&self.units);
    }

    /// Re-bind this scene's units for the formatters (after a load).
    pub fn bind_units(&self) {
        units::bind(&self.units);
    }

    // ---- Active mesh ------------------------------------------------------------
    /// The mesh tools and commands edit: the innermost open group's, or the
    /// loose mesh.
    pub fn active_mesh_id(&self) -> MeshId {
        match self.edit_stack.last() {
            None => MeshId::LOOSE,
            Some(level) => self
                .doc
                .find_group(level.group)
                .map_or(MeshId::LOOSE, |g| g.mesh),
        }
    }

    pub fn mesh(&self) -> &Mesh {
        self.doc.mesh(self.active_mesh_id())
    }

    pub fn mesh_mut(&mut self) -> &mut Mesh {
        let id = self.active_mesh_id();
        self.doc.mesh_mut(id)
    }

    pub fn loose_mesh(&self) -> &Mesh {
        self.doc.mesh(MeshId::LOOSE)
    }

    pub fn edit_group(&self) -> Option<GroupId> {
        self.edit_stack.last().map(|l| l.group)
    }

    /// The axes drawing happens on: the world's at the top level, the open
    /// group's own axes inside it.
    pub fn drawing_frame(&self) -> Option<DMat4> {
        self.edit_stack.last().and_then(|l| l.frame)
    }

    /// Whether `g` is a component copy. An open copy without children has
    /// lost its matrix while open; its share-back still holds it.
    pub fn is_component(&self, g: &Group) -> bool {
        g.is_component()
            || (g.component
                && self
                    .edit_stack
                    .iter()
                    .any(|l| l.group == g.id && l.share.is_some()))
    }

    /// The mesh `g` shares with its copies: an open copy edits a world
    /// copy of it until it is left.
    pub fn shared_mesh(&self, g: &Group) -> MeshId {
        self.edit_stack
            .iter()
            .find(|l| l.group == g.id)
            .and_then(|l| l.share.as_ref())
            .map_or(g.mesh, |s| s.proto)
    }

    /// Whether the open group is a component or inside one: what is made
    /// in it is part of the component's contents.
    pub fn in_definition(&self) -> bool {
        self.edit_stack.iter().any(|l| {
            self.doc
                .find_group(l.group)
                .is_some_and(|g| self.is_component(g))
        })
    }

    pub fn doc_view(&self) -> DocView<'_> {
        self.doc.doc_view(MeshId::LOOSE)
    }

    // ---- Layers / visibility ------------------------------------------------------
    fn layer_state(&self, label: Option<&str>) -> (bool, bool) {
        match self.doc.layer(layer_name(label)) {
            None => (true, false),
            Some(l) => (l.visible, l.locked),
        }
    }

    /// The layer label an entity carries.
    pub fn entity_layer(&self, e: &EntityRef) -> Option<String> {
        match *e {
            EntityRef::Edge(m, id) => self.doc.mesh(m).edges.get(id).and_then(|x| x.layer.clone()),
            EntityRef::Face(m, id) => self
                .doc
                .mesh(m)
                .faces
                .get(id)
                .and_then(|f| f.attrs.layer.clone()),
            EntityRef::Group(g) => self.doc.find_group(g).and_then(|g| g.layer.clone()),
            EntityRef::Dimension(d) => self
                .doc
                .dimensions
                .iter()
                .find(|x| x.id == d)
                .and_then(|x| x.layer.clone()),
            EntityRef::TextLabel(t) => self
                .doc
                .text_labels
                .iter()
                .find(|x| x.id == t)
                .and_then(|x| x.layer.clone()),
            EntityRef::Guide(_) | EntityRef::SectionPlane(_) => None,
        }
    }

    fn object_hidden(&self, e: &EntityRef) -> bool {
        match *e {
            EntityRef::Group(g) => self.doc.find_group(g).is_some_and(|g| g.hidden),
            _ => false,
        }
    }

    fn face_hidden(&self, e: &EntityRef) -> bool {
        match *e {
            EntityRef::Face(m, f) => self
                .doc
                .mesh(m)
                .faces
                .get(f)
                .is_some_and(|f| f.attrs.hidden),
            _ => false,
        }
    }

    /// Hidden by Hide (object or face).
    pub fn entity_hidden(&self, e: &EntityRef) -> bool {
        self.object_hidden(e) || self.face_hidden(e)
    }

    pub fn entity_visible(&self, e: &EntityRef) -> bool {
        if self.entity_hidden(e) {
            return false;
        }
        self.layer_state(self.entity_layer(e).as_deref()).0
    }

    pub fn entity_selectable(&self, e: &EntityRef) -> bool {
        if self.object_hidden(e) && !self.show_hidden_objects {
            return false;
        }
        if self.face_hidden(e) && !self.show_hidden_geometry {
            return false;
        }
        let (visible, locked) = self.layer_state(self.entity_layer(e).as_deref());
        visible && !locked
    }

    /// Whether an entity still resolves in the document.
    pub fn entity_exists(&self, e: &EntityRef) -> bool {
        match *e {
            EntityRef::Edge(m, id) => {
                self.doc.meshes.contains_key(&m) && self.doc.mesh(m).has_edge(id)
            }
            EntityRef::Face(m, id) => {
                self.doc.meshes.contains_key(&m) && self.doc.mesh(m).has_face(id)
            }
            EntityRef::Group(g) => self.doc.find_group(g).is_some(),
            EntityRef::Dimension(d) => self.doc.dimensions.iter().any(|x| x.id == d),
            EntityRef::TextLabel(t) => self.doc.text_labels.iter().any(|x| x.id == t),
            EntityRef::Guide(g) => self.doc.guides.iter().any(|x| x.id == g),
            EntityRef::SectionPlane(p) => self.doc.section_planes.iter().any(|x| x.id == p),
        }
    }

    // ---- Sections -------------------------------------------------------------------
    pub fn active_section(&self) -> Option<&SectionPlane> {
        self.doc.section_planes.iter().find(|p| p.active)
    }

    pub fn set_active_section(&mut self, plane: Option<ObjId>) {
        for sp in self.doc.section_planes.iter_mut() {
            sp.active = Some(sp.id) == plane;
        }
    }

    // ---- Group-edit context ------------------------------------------------------------
    /// Enter a group: tools and commands now edit ITS mesh. Entering a child
    /// of the open group pushes a level; anything else closes what is open.
    /// Entering a component INSTANCE edits a world copy of its definition;
    /// leaving shares the edit back to every copy. A container component's
    /// matrix goes down into its children while it is open, and comes back
    /// on leaving.
    pub fn begin_group_edit(&mut self, group: GroupId) {
        let nesting = self
            .edit_group()
            .and_then(|eg| self.doc.find_group(eg))
            .is_some_and(|eg| eg.children.iter().any(|c| c.id == group));
        if self.edit_group().is_some() && !nesting {
            self.end_group_edit();
        }
        let prev = self.active_mesh_id();
        let Some(g) = self.doc.find_group(group) else {
            return;
        };
        let frame = group_frame(g);
        let has_children = !g.children.is_empty();
        // A group inside an open component is part of its contents: it
        // comes back as it was, so a look changes nothing in the copies.
        let in_definition = self.edit_stack.iter().any(|l| {
            self.doc
                .find_group(l.group)
                .is_some_and(Group::is_component)
        });
        let shared = g.is_component() || in_definition;
        let xform = g.xform;
        let mut share = None;
        if has_children {
            if shared {
                share = self.open_container_copy(group);
            } else {
                self.bake_container_xform(group);
            }
            if frame.is_some() {
                if let Some(g) = self.doc.find_group_mut(group) {
                    g.axes = frame;
                }
            }
        } else if let Some(x) = xform {
            let proto = g.mesh;
            let world = transformed_mesh(self.doc.mesh(proto), &x);
            let state0 = Arc::new(world.clone());
            let id = self.doc.add_mesh(world);
            if let Some(g) = self.doc.find_group_mut(group) {
                g.mesh = id;
                g.xform = None;
            }
            share = Some(EditShare {
                group,
                proto,
                xform: x,
                state0,
                contents: None,
            });
        }
        self.edit_stack.push(EditLevel {
            group,
            prev_mesh: prev,
            share,
            frame,
            undo_mark: 0,
        });
        self.selection.clear();
        self.version += 1;
    }

    /// Push a container's own matrix down into its children and its mesh.
    fn bake_container_xform(&mut self, group: GroupId) {
        let Some(g) = self.doc.find_group(group) else {
            return;
        };
        let Some(x) = g.xform else { return };
        if x == DMat4::IDENTITY {
            return;
        }
        let mesh_id = g.mesh;
        let has_verts = !self.doc.mesh(mesh_id).verts.is_empty();
        let new_mesh = has_verts.then(|| transformed_mesh(self.doc.mesh(mesh_id), &x));
        let new_id = new_mesh.map(|m| self.doc.add_mesh(m));
        let Some(g) = self.doc.find_group_mut(group) else {
            return;
        };
        // A plain group placed by its parent's matrix stays a group: the
        // matrix it gets is a place, not a component's.
        for c in g.children.iter_mut().filter(|c| c.xform.is_none()) {
            c.component = false;
        }
        push_matrix_down(g, &x);
        if let Some(id) = new_id {
            g.mesh = id;
        }
    }

    /// Open a container component copy: it edits a world copy of its mesh,
    /// with its matrix pushed down into its children, and the share
    /// remembers its contents to put them back.
    fn open_container_copy(&mut self, group: GroupId) -> Option<EditShare> {
        let g = self.doc.find_group(group)?;
        let x = g.xform?;
        let proto = g.mesh;
        let contents = EntryContents {
            children: g.children.clone(),
            axes: g.axes,
            component: g.is_component(),
        };
        let world = transformed_mesh(self.doc.mesh(proto), &x);
        let state0 = Arc::new(world.clone());
        let id = self.doc.add_mesh(world);
        let g = self.doc.find_group_mut(group)?;
        // A part without a matrix stays a part while the copy is open: the
        // matrix it gets is the copy's place, not a component's. Leaving
        // gives it back its flag with its matrix (`pull_matrix_up`).
        for c in g.children.iter_mut().filter(|c| c.xform.is_none()) {
            c.component = false;
        }
        push_matrix_down(g, &x);
        g.mesh = id;
        Some(EditShare {
            group,
            proto,
            xform: x,
            state0,
            contents: Some(contents),
        })
    }

    /// Leave the INNERMOST group only (Esc).
    pub fn end_one_group_edit(&mut self) {
        self.leave_level();
    }

    /// Leave every open group-edit context, back to the loose mesh.
    pub fn end_group_edit(&mut self) {
        while !self.edit_stack.is_empty() {
            self.leave_level();
        }
    }

    fn leave_level(&mut self) {
        let Some(level) = self.edit_stack.pop() else {
            return;
        };
        match level.share {
            Some(share) if share.contents.is_some() => self.share_back_contents(&share),
            Some(share) => {
                let edited = self.doc.find_group(share.group).map(|g| g.mesh);
                if !self.edit_changed(&share) {
                    self.restore_sharing(share.group, share.proto, share.xform);
                } else if let Some(m) = edited {
                    self.share_back(share.group, share.proto, share.xform, m);
                }
            }
            None => {}
        }
        self.selection.clear();
        self.version += 1;
    }

    /// The pending instance share-back of the INNERMOST open group — handed
    /// over ONCE, so whoever ends the edit decides how.
    pub fn take_edit_share(&mut self) -> Option<EditShare> {
        self.edit_stack.last_mut().and_then(|l| l.share.take())
    }

    /// The instance left untouched: back on the shared definition.
    pub fn restore_sharing(&mut self, group: GroupId, proto: MeshId, xform: DMat4) {
        if let Some(g) = self.doc.find_group_mut(group) {
            g.mesh = proto;
            g.xform = Some(xform);
        }
    }

    /// Whether the open copy of `share` was edited: its faces differ from
    /// those it opened with, or a container's children from those it held
    /// (compared in its local coordinates), or a copy that held none has
    /// some.
    pub fn edit_changed(&self, share: &EditShare) -> bool {
        let Some(g) = self.doc.find_group(share.group) else {
            return false;
        };
        if !mesh_equal(self.doc.mesh(g.mesh), &share.state0) {
            return true;
        }
        let Some(c) = &share.contents else {
            return !g.children.is_empty();
        };
        let mut local = g.clone();
        crate::definitions::pull_matrix_up(&mut local, &share.xform, &c.children);
        !crate::definitions::same_children(&local.children, &c.children)
    }

    /// Leave an open container copy: the copy gets its matrix, axes and
    /// children back in local coordinates. Edited faces of a component go
    /// back into the shared mesh (groups that held the same mesh keep the
    /// faces they had: they are not copies), and edited contents to every
    /// copy of it, at every depth. A group inside a component keeps its
    /// edited faces in a mesh of its own: its component shares them.
    pub fn share_back_contents(&mut self, share: &EditShare) {
        let Some(c) = &share.contents else {
            return;
        };
        let Some(edited) = self.doc.find_group(share.group).map(|g| g.mesh) else {
            return;
        };
        let changed = self.edit_changed(share);
        let mut mesh = share.proto;
        if !mesh_equal(self.doc.mesh(edited), &share.state0) {
            let inv = share.xform.try_inverse();
            let local =
                transformed_mesh(self.doc.mesh(edited), inv.as_ref().unwrap_or(&share.xform));
            if c.component {
                crate::definitions::detach_groups(
                    &mut self.doc.meshes,
                    &mut self.doc.groups,
                    share.proto,
                );
                *self.doc.mesh_mut(share.proto) = local;
            } else {
                mesh = self.doc.add_mesh(local);
            }
        }
        if let Some(g) = self.doc.find_group_mut(share.group) {
            g.mesh = mesh;
            g.xform = Some(share.xform);
            g.axes = c.axes;
            crate::definitions::pull_matrix_up(g, &share.xform, &c.children);
        }
        if changed && c.component {
            crate::definitions::share_children(
                &mut self.doc.meshes,
                &mut self.doc.groups,
                share.group,
            );
        }
    }

    /// Write the edited world-space mesh into the shared prototype (local
    /// coordinates) and put the instance back on it. Groups made inside it
    /// go back into its local coordinates too, and a component's reach its
    /// copies.
    pub fn share_back(&mut self, group: GroupId, proto: MeshId, xform: DMat4, edited: MeshId) {
        let inv = xform.try_inverse();
        let local = transformed_mesh(self.doc.mesh(edited), inv.as_ref().unwrap_or(&xform));
        *self.doc.mesh_mut(proto) = local;
        self.restore_sharing(group, proto, xform);
        let Some(g) = self.doc.find_group_mut(group) else {
            return;
        };
        if g.children.is_empty() {
            return;
        }
        crate::definitions::pull_matrix_up(g, &xform, &[]);
        if g.component {
            crate::definitions::share_children(&mut self.doc.meshes, &mut self.doc.groups, group);
        }
    }

    // ---- Placements -------------------------------------------------------------------------
    /// Every visible placement: each group and the nested ones below it, as
    /// `(group, world_matrix)`.
    /// The faces drawn: the visible loose faces, then every face of every
    /// visible placement (in its own mesh's coordinates).
    pub fn render_faces(&self) -> Vec<(MeshId, xtrude_core::FaceId)> {
        let mut out: Vec<(MeshId, xtrude_core::FaceId)> = self
            .loose_mesh()
            .face_ids()
            .into_iter()
            .filter(|&f| self.entity_visible(&EntityRef::Face(MeshId::LOOSE, f)))
            .map(|f| (MeshId::LOOSE, f))
            .collect();
        for (g, _m) in self.placements() {
            out.extend(
                self.doc
                    .mesh(g.mesh)
                    .face_ids()
                    .into_iter()
                    .map(|f| (g.mesh, f)),
            );
        }
        out
    }

    /// The edges drawn: the visible loose edges, then every edge of every
    /// visible placement.
    pub fn render_edges(&self) -> Vec<(MeshId, xtrude_core::EdgeId)> {
        let mut out: Vec<(MeshId, xtrude_core::EdgeId)> = self
            .loose_mesh()
            .edge_ids()
            .into_iter()
            .filter(|&e| self.entity_visible(&EntityRef::Edge(MeshId::LOOSE, e)))
            .map(|e| (MeshId::LOOSE, e))
            .collect();
        for (g, _m) in self.placements() {
            out.extend(
                self.doc
                    .mesh(g.mesh)
                    .edge_ids()
                    .into_iter()
                    .map(|e| (g.mesh, e)),
            );
        }
        out
    }

    pub fn placements(&self) -> Vec<(&Group, Option<DMat4>)> {
        let mut out = Vec::new();
        for g in &self.doc.groups {
            if !self.entity_visible(&EntityRef::Group(g.id)) {
                continue;
            }
            for (pg, m) in iter_placements(g, None) {
                if pg.id != g.id && !self.entity_visible(&EntityRef::Group(pg.id)) {
                    continue;
                }
                out.push((pg, m));
            }
        }
        out
    }

    // ---- Selection ------------------------------------------------------------------------------
    pub fn select<I: IntoIterator<Item = EntityRef>>(&mut self, ents: I, mode: SelectMode) {
        if mode == SelectMode::Replace {
            self.selection.clear();
        }
        match mode {
            SelectMode::Toggle => {
                for e in ents {
                    if !self.selection.shift_remove(&e) {
                        self.selection.insert(e);
                    }
                }
            }
            SelectMode::Remove => {
                for e in ents {
                    self.selection.shift_remove(&e);
                }
            }
            _ => self.selection.extend(ents),
        }
        self.bump_view();
    }

    pub fn clear_selection(&mut self) {
        if !self.selection.is_empty() {
            self.selection.clear();
            self.bump_view();
        }
    }

    pub fn discard(&mut self, e: &EntityRef) {
        self.selection.shift_remove(e);
    }

    /// Drop selection entries that no longer resolve (after undo/redo).
    pub fn prune_selection(&mut self) {
        let keep: IndexSet<EntityRef> = self
            .selection
            .iter()
            .copied()
            .filter(|e| self.entity_exists(e))
            .collect();
        self.selection = keep;
    }

    /// A change of what is shown, not of the document.
    pub fn bump_view(&mut self) {
        self.version += 1;
        self.view_version += 1;
    }

    /// `version` minus the view-only bumps.
    pub fn content_version(&self) -> u64 {
        self.version - self.view_version
    }

    /// Edit ▸ Invert Selection: select every entity of the open context
    /// that is NOT selected now, and drop what is.
    pub fn invert_selection(&mut self) -> usize {
        let mid = self.active_mesh_id();
        let show_hidden = self.show_hidden_geometry;
        let mut universe: Vec<EntityRef> = Vec::new();
        let mesh = self.doc.mesh(mid);
        for (eid, e) in mesh.edges.iter() {
            let r = EntityRef::Edge(mid, eid);
            if self.entity_selectable(&r) && (show_hidden || !e.hidden) {
                universe.push(r);
            }
        }
        for (fid, _) in mesh.faces.iter() {
            let r = EntityRef::Face(mid, fid);
            if self.entity_selectable(&r) {
                universe.push(r);
            }
        }
        let groups: Vec<GroupId> = match self.edit_group() {
            None => self.doc.groups.iter().map(|g| g.id).collect(),
            Some(ctx) => self
                .doc
                .find_group(ctx)
                .map(|g| g.children.iter().map(|c| c.id).collect())
                .unwrap_or_default(),
        };
        for g in groups {
            let r = EntityRef::Group(g);
            if self.entity_selectable(&r) {
                universe.push(r);
            }
        }
        for d in &self.doc.dimensions {
            let r = EntityRef::Dimension(d.id);
            if self.entity_selectable(&r) {
                universe.push(r);
            }
        }
        let new: Vec<EntityRef> = universe
            .into_iter()
            .filter(|e| !self.selection.contains(e))
            .collect();
        self.selection.clear();
        self.selection.extend(new.iter().copied());
        self.bump_view();
        new.len()
    }

    /// Reset to an empty document (File ▸ New / before a load).
    pub fn clear(&mut self) {
        self.end_group_edit();
        let has_content = !self.loose_mesh().is_empty()
            || !self.selection.is_empty()
            || !self.doc.groups.is_empty()
            || !self.doc.dimensions.is_empty()
            || !self.doc.guides.is_empty()
            || !self.doc.text_labels.is_empty()
            || !self.saved_views.is_empty()
            || !self.doc.plugin_data.is_empty();
        if has_content {
            self.doc = Doc::default();
            self.saved_views.clear();
            self.selection.clear();
            self.back_face_color = None;
            self.display_style = Style::default();
            self.show_section_planes = true;
            self.show_section_cuts = true;
            self.show_hidden_objects = false;
            self.show_hidden_geometry = false;
            self.version += 1;
        }
        self.camera_home = None;
        self.units = units::default_model_units();
        self.units.length = "mm".into();
        self.units.precision = 2;
        units::bind(&self.units);
        self.doc.plugin_data = Map::new();
    }

    // ---- Queries ---------------------------------------------------------------------------------

    /// Axis-aligned bounding box of the SELECTION.
    pub fn selection_bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        let mut seen = false;
        let mut add = |p: DVec3| {
            lo = lo.min(p);
            hi = hi.max(p);
            seen = true;
        };
        let dv = self.doc_view();
        for ent in &self.selection {
            match *ent {
                EntityRef::Group(gid) => {
                    if let Some(g) = self.doc.find_group(gid) {
                        for p in crate::group::placement_points(&self.doc.meshes, g) {
                            add(p);
                        }
                    }
                }
                EntityRef::Face(m, f) => {
                    if self.doc.mesh(m).has_face(f) {
                        for p in self.doc.mesh(m).face_positions(f) {
                            add(p);
                        }
                    }
                }
                EntityRef::Edge(m, e) => {
                    if self.doc.mesh(m).has_edge(e) {
                        let (a, b) = self.doc.mesh(m).edge_ab(e);
                        add(a);
                        add(b);
                    }
                }
                EntityRef::Dimension(d) => {
                    if let Some(d) = self.doc.dimension(d) {
                        add(d.a(&dv));
                        add(d.b(&dv));
                    }
                }
                _ => {}
            }
        }
        seen.then_some((lo, hi))
    }

    /// Axis-aligned bounding box of all visible geometry (cached per
    /// version).
    pub fn bounds(&mut self) -> Option<(DVec3, DVec3)> {
        if let Some((v, b)) = self.bounds_cache {
            if v == self.version {
                return b;
            }
        }
        let b = self.compute_bounds();
        self.bounds_cache = Some((self.version, b));
        b
    }

    pub fn compute_bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        let mut seen = false;
        let loose = self.loose_mesh();
        for (eid, _) in loose.edges.iter() {
            if self.entity_visible(&EntityRef::Edge(MeshId::LOOSE, eid)) {
                let (a, b) = loose.edge_ab(eid);
                lo = lo.min(a).min(b);
                hi = hi.max(a).max(b);
                seen = true;
            }
        }
        for (fid, _) in loose.faces.iter() {
            if self.entity_visible(&EntityRef::Face(MeshId::LOOSE, fid)) {
                for p in loose.face_positions(fid) {
                    lo = lo.min(p);
                    hi = hi.max(p);
                    seen = true;
                }
            }
        }
        for (g, m) in self.placements() {
            let mesh = self.doc.mesh(g.mesh);
            if mesh.verts.is_empty() {
                continue;
            }
            for (_, v) in mesh.verts.iter() {
                let p = m.map_or(v.position, |mm| mm.project_point(v.position));
                lo = lo.min(p);
                hi = hi.max(p);
            }
            seen = true;
        }
        seen.then_some((lo, hi))
    }

    /// Refresh every dimension's anchors (after each command, undo, redo).
    pub fn rebind_dimensions(&mut self) {
        let mut dims = std::mem::take(&mut self.doc.dimensions);
        {
            let dv = self.doc.doc_view(MeshId::LOOSE);
            for d in dims.iter_mut() {
                d.refresh_anchors(&dv);
            }
        }
        self.doc.dimensions = dims;
    }

    /// Bind a fresh dimension to the vertices under its endpoints.
    pub fn bind_dimension(&self, d: &mut Dimension) {
        let dv = self.doc.doc_view(MeshId::LOOSE);
        d.bind(&dv);
    }
}

/// Whether two meshes hold the same geometry and attributes (the test on
/// the edited instance copy).
/// Push a container's matrix down into its children: the container sits
/// at identity, its children where they were drawn.
fn push_matrix_down(g: &mut Group, x: &DMat4) {
    for c in g.children.iter_mut() {
        c.xform = Some(*x * c.xform.unwrap_or(DMat4::IDENTITY));
    }
    crate::explode::rotate_offsets(g, x);
    g.xform = Some(DMat4::IDENTITY);
}

pub fn mesh_equal(a: &Mesh, b: &Mesh) -> bool {
    if a.verts.len() != b.verts.len()
        || a.edges.len() != b.edges.len()
        || a.faces.len() != b.faces.len()
    {
        return false;
    }
    let va: Vec<_> = a.verts.iter().map(|(id, v)| (id, v.position)).collect();
    let vb: Vec<_> = b.verts.iter().map(|(id, v)| (id, v.position)).collect();
    if va != vb {
        return false;
    }
    let ea: Vec<_> = a
        .edges
        .iter()
        .map(|(id, e)| (id, e.v0, e.v1, e.flags()))
        .collect();
    let eb: Vec<_> = b
        .edges
        .iter()
        .map(|(id, e)| (id, e.v0, e.v1, e.flags()))
        .collect();
    if ea != eb {
        return false;
    }
    a.faces
        .iter()
        .zip(b.faces.iter())
        .all(|((ia, fa), (ib, fb))| {
            ia == ib && fa.outer == fb.outer && fa.holes == fb.holes && fa.attrs == fb.attrs
        })
}
