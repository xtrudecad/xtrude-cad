// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Geometry commands over the active mesh.
//!
//! Commands resolve the geometry they act on **by position at do-time**,
//! so the position-based plans `edits` builds execute correctly onto the
//! real mesh.

use std::collections::{HashMap, HashSet};

use xtrude_core::arrangement::{coplanar_plane, planar_rebuild};
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::geometry::PolyFace;
use xtrude_core::math::{rotation_about, DMat4, DVec3, DVec4, MatExt, VecExt};
use xtrude_core::mesh::{EdgeFlags, FaceId, Key, Mesh};
use xtrude_core::topology::{
    carve_loop_by_chords, find_containing_face, fold_nonplanar_faces, heal_overlapping_faces,
    loop_edges, loop_inside_face, orient_coplanar_faces, orphaned_edges_at, point_on_seg_incl,
    prune_collinear_orphan_edges, subtract_loop_from_face, tkey, winding_footprint,
    PLANAR_TOLERANCE, SPLIT_TOLERANCE,
};
use xtrude_core::triangulate::triangulate;

use crate::entity::EntityRef;
use crate::history::{CmdError, CmdResult, Command};
use crate::ids::MeshId;
use crate::scene::Scene;

fn key_set(pts: &[DVec3]) -> Vec<Key> {
    let mut k: Vec<Key> = pts.iter().map(|&p| tkey(p)).collect();
    k.sort();
    k.dedup();
    k
}

/// The mesh face whose outer loop matches `positions` (by key), or `None`.
pub fn find_face_by_loop(mesh: &Mesh, positions: &[DVec3]) -> Option<FaceId> {
    let target = key_set(positions);
    mesh.face_ids()
        .into_iter()
        .find(|&f| key_set(&mesh.face_positions(f)) == target)
}

/// The edge between the vertices at two positions, if any.
pub fn find_edge_at(mesh: &Mesh, a: DVec3, b: DVec3) -> Option<xtrude_core::EdgeId> {
    let v0 = mesh.vertex_at(a)?;
    let v1 = mesh.vertex_at(b)?;
    mesh.find_edge(v0, v1)
}

fn discard_face(scene: &mut Scene, mid: MeshId, f: FaceId) {
    scene.discard(&EntityRef::Face(mid, f));
}

fn discard_edge(scene: &mut Scene, mid: MeshId, e: xtrude_core::EdgeId) {
    scene.discard(&EntityRef::Edge(mid, e));
}

// ---- Edges -----------------------------------------------------------------------

/// Add a single edge. The mesh always welds and dedups; flags are stamped
/// on the resulting edge (a split's pieces inherit the curve/soft).
pub struct AddEdgeCommand {
    pub a: DVec3,
    pub b: DVec3,
    pub soft: Option<bool>,
    pub curve: Option<i64>,
    /// The edge this command created (None when it merged into one).
    pub edge: Option<xtrude_core::EdgeId>,
}

impl AddEdgeCommand {
    pub fn new(a: DVec3, b: DVec3) -> Self {
        AddEdgeCommand {
            a,
            b,
            soft: None,
            curve: None,
            edge: None,
        }
    }

    pub fn with_flags(a: DVec3, b: DVec3, soft: Option<bool>, curve: Option<i64>) -> Self {
        AddEdgeCommand {
            a,
            b,
            soft,
            curve,
            edge: None,
        }
    }
}

impl Command for AddEdgeCommand {
    fn name(&self) -> &str {
        "AddEdgeCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let (soft, curve) = (self.soft, self.curve);
        let m = scene.mesh_mut();
        let stamp = |m: &mut Mesh, e| {
            if let Some(s) = soft {
                m.edge_mut(e).soft = s;
            }
            if let Some(c) = curve {
                m.edge_mut(e).curve = Some(c);
            }
        };
        if let Some(pre) = find_edge_at(m, self.a, self.b) {
            stamp(m, pre);
            scene.version += 1;
            return Ok(());
        }
        let e = m.add_edge(self.a, self.b)?;
        stamp(m, e);
        self.edge = Some(e);
        scene.version += 1;
        Ok(())
    }
}

/// Erase edges (resolved by endpoint position). With `cascade_faces`,
/// every face that used a deleted edge on its OUTER boundary goes too.
pub struct DeleteEdgesCommand {
    pub endpoints: Vec<(DVec3, DVec3)>,
    pub cascade_faces: bool,
}

impl Command for DeleteEdgesCommand {
    fn name(&self) -> &str {
        "DeleteEdgesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mid = scene.active_mesh_id();
        let m = scene.mesh_mut();
        let removed_edges: Vec<_> = self
            .endpoints
            .iter()
            .filter_map(|&(a, b)| find_edge_at(m, a, b))
            .collect();
        let mut removed_faces = Vec::new();
        if self.cascade_faces {
            let gone: HashSet<(Key, Key)> = self
                .endpoints
                .iter()
                .map(|&(a, b)| ukey(tkey(a), tkey(b)))
                .collect();
            for f in m.face_ids() {
                if loop_edges(&m.face_positions(f))
                    .iter()
                    .any(|e| gone.contains(e))
                {
                    removed_faces.push(f);
                }
            }
            for &f in &removed_faces {
                m.remove_face(f);
            }
        }
        for &e in &removed_edges {
            m.remove_edge(e);
        }
        for f in removed_faces {
            discard_face(scene, mid, f);
        }
        for e in removed_edges {
            discard_edge(scene, mid, e);
        }
        scene.version += 1;
        Ok(())
    }
}

fn ukey(a: Key, b: Key) -> (Key, Key) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Erase selected edges and faces, the usual way: an edge dividing two
/// *coplanar* faces is dissolved and the faces merge back into one; any
/// other erased edge takes the faces it bounds with it.
pub struct EraseSelectionCommand {
    pub edge_endpoints: Vec<(DVec3, DVec3)>,
    pub face_loops: Vec<Vec<DVec3>>,
}

impl EraseSelectionCommand {
    /// Capture the positions of the selected edges and faces of `mesh`.
    pub fn from_entities(mesh: &Mesh, edges: &[xtrude_core::EdgeId], faces: &[FaceId]) -> Self {
        EraseSelectionCommand {
            edge_endpoints: edges
                .iter()
                .filter(|&&e| mesh.has_edge(e))
                .map(|&e| mesh.edge_ab(e))
                .collect(),
            face_loops: faces
                .iter()
                .filter(|&&f| mesh.has_face(f))
                .map(|&f| mesh.face_positions(f))
                .collect(),
        }
    }
}

impl Command for EraseSelectionCommand {
    fn name(&self) -> &str {
        "EraseSelectionCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mid = scene.active_mesh_id();
        let mut dropped_sel: Vec<EntityRef> = Vec::new();
        let mut slit_planes: Vec<(DVec3, DVec3)> = Vec::new();
        let footprint = winding_footprint(scene.mesh());
        {
            let m = scene.mesh_mut();
            let mut by_loop: HashMap<Vec<Key>, Vec<FaceId>> = HashMap::new();
            for f in m.face_ids() {
                by_loop
                    .entry(key_set(&m.face_positions(f)))
                    .or_default()
                    .push(f);
            }
            for lp in &self.face_loops {
                if let Some(bucket) = by_loop.get_mut(&key_set(lp)) {
                    if !bucket.is_empty() {
                        let f = bucket.remove(0);
                        m.remove_face(f);
                        dropped_sel.push(EntityRef::Face(mid, f));
                    }
                }
            }
            let mut seg_faces: HashMap<(Key, Key), Vec<FaceId>> = HashMap::new();
            let index_face = |m: &Mesh, f: FaceId, seg: &mut HashMap<(Key, Key), Vec<FaceId>>| {
                for ek in loop_edges(&m.face_positions(f)) {
                    seg.entry(ek).or_default().push(f);
                }
            };
            for f in m.face_ids() {
                index_face(m, f, &mut seg_faces);
            }
            let mut dead: HashSet<FaceId> = HashSet::new();
            for &(a, b) in &self.edge_endpoints {
                let Some(edge) = find_edge_at(m, a, b) else {
                    continue;
                };
                let faces = m.edge(edge).faces.clone();
                if faces.len() == 2 && faces[0] == faces[1] {
                    // Slit edge: one face walks it twice. Erasing the line
                    // keeps the face; the plane is re-derived below.
                    let f = faces[0];
                    slit_planes.push((m.face_positions(f)[0], m.face_normal(f)));
                    m.remove_edge(edge);
                    dropped_sel.push(EntityRef::Edge(mid, edge));
                    continue;
                }
                let mut merged = None;
                if faces.len() == 2 && m.face_normal(faces[0]).dot(m.face_normal(faces[1])) > 0.999
                {
                    merged = m.dissolve_coplanar_region(&faces);
                    if let Some(nf) = merged {
                        dead.extend(faces.iter().copied());
                        index_face(m, nf, &mut seg_faces);
                    }
                }
                if merged.is_none() {
                    let ed = m.edge(edge);
                    let ekey = ukey(tkey(m.pos(ed.v0)), tkey(m.pos(ed.v1)));
                    for f in seg_faces.get(&ekey).cloned().unwrap_or_default() {
                        if !dead.insert(f) {
                            continue;
                        }
                        m.remove_face(f);
                        dropped_sel.push(EntityRef::Face(mid, f));
                    }
                    m.remove_edge(edge);
                }
                dropped_sel.push(EntityRef::Edge(mid, edge));
            }
        }
        let mut done: Vec<(DVec3, DVec3)> = Vec::new();
        for (origin, normal) in slit_planes {
            let nn = normal.unit_or_zero();
            if done
                .iter()
                .any(|(o, n)| nn.dot(*n).abs() > 0.999 && (origin - *o).dot(*n).abs() < 1e-4)
            {
                continue;
            }
            done.push((origin, nn));
            RebuildPlaneFacesCommand::new(origin, nn).apply(scene)?;
        }
        let m = scene.mesh_mut();
        for e in m.edge_ids() {
            if m.edge(e).soft && m.edge(e).faces.is_empty() {
                m.remove_edge(e);
            }
        }
        let removed = heal_overlapping_faces(m, 0.5, None, footprint.as_ref());
        m.prune_orphan_vertices();
        for f in removed {
            dropped_sel.push(EntityRef::Face(mid, f));
        }
        for e in dropped_sel {
            scene.discard(&e);
        }
        scene.version += 1;
        Ok(())
    }
}

/// Mark the edges of a drawn circle/arc loop as one curve.
pub struct TagCurveCommand {
    pub pts: Vec<DVec3>,
    pub closed: bool,
}

impl Command for TagCurveCommand {
    fn name(&self) -> &str {
        "TagCurveCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        scene.mesh_mut().tag_curve(&self.pts, self.closed);
        Ok(())
    }
}

// ---- Faces ------------------------------------------------------------------------

/// Give `face` the paint of the `mother` it was drawn on, for the keys it
/// does not already carry itself.
pub fn inherit_paint(face: &mut FaceAttrs, mother: &FaceAttrs) {
    if face.paint.color.is_none() && mother.paint.color.is_some() {
        face.paint.color = mother.paint.color;
    }
    if face.paint.mat.is_none() && mother.paint.mat.is_some() {
        face.paint.mat = mother.paint.mat.clone();
    }
    if face.paint.opacity.is_none() && mother.paint.opacity.is_some() {
        face.paint.opacity = mother.paint.opacity;
    }
}

/// Add a face, dividing any coplanar face it lands strictly inside.
pub struct AddFaceCommand {
    pub vertices: Vec<DVec3>,
    pub auto: bool,
    pub holes: Vec<Vec<DVec3>>,
    pub attrs: Option<FaceAttrs>,
    /// The face this command created.
    pub face: Option<FaceId>,
}

impl AddFaceCommand {
    pub fn new(vertices: Vec<DVec3>) -> Self {
        AddFaceCommand {
            vertices,
            auto: true,
            holes: Vec::new(),
            attrs: None,
            face: None,
        }
    }

    pub fn plain(vertices: Vec<DVec3>, holes: Vec<Vec<DVec3>>, attrs: Option<FaceAttrs>) -> Self {
        AddFaceCommand {
            vertices,
            auto: false,
            holes,
            attrs,
            face: None,
        }
    }

    pub fn with_attrs(vertices: Vec<DVec3>, attrs: Option<FaceAttrs>) -> Self {
        AddFaceCommand {
            vertices,
            auto: true,
            holes: Vec::new(),
            attrs,
            face: None,
        }
    }
}

impl Command for AddFaceCommand {
    fn name(&self) -> &str {
        "AddFaceCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let face = add_face_auto(
            scene.mesh_mut(),
            &self.vertices,
            &self.holes,
            self.attrs.as_ref(),
            self.auto,
        );
        self.face = Some(face);
        scene.version += 1;
        Ok(())
    }
}

/// The body of [`AddFaceCommand`] on any mesh: add the face and, with
/// `auto`, divide the coplanar face it lands in (a hole in a mother, holes
/// for faces it encloses, or a carve of a face it is drawn against).
pub fn add_face_auto(
    m: &mut Mesh,
    vertices: &[DVec3],
    holes: &[Vec<DVec3>],
    attrs: Option<&FaceAttrs>,
    auto: bool,
) -> FaceId {
    let face = m.add_face(vertices, holes);
    if let Some(a) = attrs {
        m.faces[face].attrs.update(a);
    }
    if !auto {
        return face;
    }
    let face_pos = m.face_positions(face);
    let mut punched = false;
    // Direction A: the new face falls inside an existing mother.
    let all = m.face_ids();
    if let Some(mother) = find_containing_face(m, &all, &face_pos, Some(face)) {
        m.add_hole(mother, &face_pos);
        punched = true;
        let mattrs = m.face(mother).attrs.clone();
        inherit_paint(&mut m.faces[face].attrs, &mattrs);
    }
    // Direction B: the new face encloses existing smaller faces.
    let me = m.poly_face(face);
    for other in m.face_ids() {
        if other == face {
            continue;
        }
        let op = m.face_positions(other);
        if loop_inside_face(&me, &op, false) {
            m.add_hole(face, &op);
            punched = true;
        }
    }
    // Direction C: drawn against an existing face's boundary.
    if !punched {
        let face_pos = m.face_positions(face);
        for other in m.face_ids() {
            if other == face {
                continue;
            }
            let op = m.poly_face(other);
            let pieces = match subtract_loop_from_face(&op, &face_pos) {
                Some(r) => Some(vec![r]),
                None => carve_loop_by_chords(&op, &face_pos),
            };
            let Some(pieces) = pieces.filter(|p| !p.is_empty()) else {
                continue;
            };
            let mut piece_holes: Vec<Vec<Vec<DVec3>>> = vec![Vec::new(); pieces.len()];
            let mut straddle = false;
            for hole in &op.holes {
                let mut placed = false;
                for (k, piece) in pieces.iter().enumerate() {
                    if loop_inside_face(&PolyFace::new(piece.clone()), hole, false) {
                        piece_holes[k].push(hole.clone());
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    straddle = true;
                    break;
                }
            }
            if straddle {
                continue;
            }
            let oattrs = m.face(other).attrs.clone();
            m.remove_face(other);
            for (piece, holes) in pieces.iter().zip(piece_holes) {
                let rf = m.add_face(piece, &holes);
                m.faces[rf].attrs = oattrs.clone();
            }
            inherit_paint(&mut m.faces[face].attrs, &oattrs);
            break;
        }
    }
    face
}

/// Remove the face whose outer loop matches `lp` (by key); returns it.
pub fn delete_face_by_loop(m: &mut Mesh, lp: &[DVec3]) -> Option<FaceId> {
    let f = find_face_by_loop(m, lp)?;
    m.remove_face(f);
    Some(f)
}

/// [`AddEdgeCommand`] on any mesh: weld/dedup, stamp the flags.
pub fn add_edge_stamped(
    m: &mut Mesh,
    a: DVec3,
    b: DVec3,
    soft: Option<bool>,
    curve: Option<i64>,
) -> Result<Option<xtrude_core::EdgeId>, CmdError> {
    let stamp = |m: &mut Mesh, e| {
        if let Some(s) = soft {
            m.edge_mut(e).soft = s;
        }
        if let Some(c) = curve {
            m.edge_mut(e).curve = Some(c);
        }
    };
    if let Some(pre) = find_edge_at(m, a, b) {
        stamp(m, pre);
        return Ok(None);
    }
    let e = m.add_edge(a, b)?;
    stamp(m, e);
    Ok(Some(e))
}

/// [`PruneOrphanEdgesCommand`] on any mesh.
pub fn prune_orphans_at(m: &mut Mesh, vertices: &[DVec3]) -> Vec<xtrude_core::EdgeId> {
    let removed = orphaned_edges_at(m, vertices);
    for &e in &removed {
        m.remove_edge(e);
    }
    removed
}

/// Remove a face (resolved by its outer loop).
pub struct DeleteFaceCommand {
    pub lp: Vec<DVec3>,
}

impl Command for DeleteFaceCommand {
    fn name(&self) -> &str {
        "DeleteFaceCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mid = scene.active_mesh_id();
        let m = scene.mesh_mut();
        if let Some(f) = find_face_by_loop(m, &self.lp) {
            m.remove_face(f);
            discard_face(scene, mid, f);
        }
        scene.version += 1;
        Ok(())
    }
}

/// Reverse the winding of a set of faces — "Reverse Faces" (the material
/// follows the front, by design).
pub struct FlipFacesCommand {
    pub faces: Vec<(MeshId, FaceId)>,
}

impl Command for FlipFacesCommand {
    fn name(&self) -> &str {
        "FlipFacesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        for &(mid, f) in &self.faces {
            let m = scene.doc.mesh_mut(mid);
            if !m.has_face(f) {
                continue;
            }
            let fc = m.face_mut(f);
            fc.outer.reverse();
            for h in fc.holes.iter_mut() {
                h.reverse();
            }
        }
        scene.version += 1;
        Ok(())
    }
}

// ---- Vertex transforms -----------------------------------------------------------

/// Move every shared vertex whose position key is in `keys` by `delta`.
pub fn translate_points(mesh: &mut Mesh, keys: &HashSet<Key>, delta: DVec3) {
    let moving: Vec<_> = mesh
        .vertex_ids()
        .into_iter()
        .filter(|&v| keys.contains(&tkey(mesh.pos(v))))
        .collect();
    for v in moving {
        mesh.move_vertex(v, delta);
    }
}

/// Map every shared vertex whose key is in `keys` through `matrix`.
pub fn rotate_points(mesh: &mut Mesh, keys: &HashSet<Key>, matrix: &DMat4) {
    let moving: Vec<_> = mesh
        .vertex_ids()
        .into_iter()
        .filter(|&v| keys.contains(&tkey(mesh.pos(v))))
        .collect();
    for v in moving {
        let p = mesh.pos(v);
        mesh.move_vertex(v, matrix.project_point(p) - p);
    }
}

/// Rigid rotation of `degrees` around `axis` through `center`.
pub fn rotation_matrix(center: DVec3, axis: DVec3, degrees: f64) -> DMat4 {
    rotation_about(center, axis.unit_or_zero(), degrees)
}

/// Reflection about the plane through `centre` with normal `axis`.
pub fn mirror_matrix(centre: DVec3, axis: DVec3) -> DMat4 {
    let n = axis.unit_or_zero();
    let r = DMat4::from_cols(
        DVec4::new(
            1.0 - 2.0 * n.x * n.x,
            -2.0 * n.y * n.x,
            -2.0 * n.z * n.x,
            0.0,
        ),
        DVec4::new(
            -2.0 * n.x * n.y,
            1.0 - 2.0 * n.y * n.y,
            -2.0 * n.z * n.y,
            0.0,
        ),
        DVec4::new(
            -2.0 * n.x * n.z,
            -2.0 * n.y * n.z,
            1.0 - 2.0 * n.z * n.z,
            0.0,
        ),
        DVec4::new(0.0, 0.0, 0.0, 1.0),
    );
    DMat4::from_translation(centre) * r * DMat4::from_translation(-centre)
}

/// A scale factor: uniform or per axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleFactor {
    Uniform(f64),
    PerAxis([f64; 3]),
}

/// Scale about `center` (per axis along `axes` when given).
pub fn scale_matrix(
    center: DVec3,
    factor: ScaleFactor,
    axes: Option<(DVec3, DVec3, DVec3)>,
) -> DMat4 {
    if let (Some((x, y, z)), ScaleFactor::PerAxis(f)) = (axes, factor) {
        let rot = xtrude_core::axes::frame_matrix(DVec3::ZERO, x, y, z);
        let inv = rot.transpose();
        let s = DMat4::from_scale(DVec3::new(f[0], f[1], f[2]));
        return DMat4::from_translation(center) * rot * s * inv * DMat4::from_translation(-center);
    }
    let s = match factor {
        ScaleFactor::Uniform(k) => DVec3::splat(k),
        ScaleFactor::PerAxis(f) => DVec3::new(f[0], f[1], f[2]),
    };
    DMat4::from_translation(center) * DMat4::from_scale(s) * DMat4::from_translation(-center)
}

/// Translate every shared vertex at a set of positions by `delta`, then
/// **autofold** any face the move warped out of its plane.
pub struct MoveVerticesCommand {
    pub src: Vec<DVec3>,
    pub delta: DVec3,
}

impl Command for MoveVerticesCommand {
    fn name(&self) -> &str {
        "MoveVerticesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let keys: HashSet<Key> = self.src.iter().map(|&p| tkey(p)).collect();
        let m = scene.mesh_mut();
        translate_points(m, &keys, self.delta);
        fold_nonplanar_faces(m, PLANAR_TOLERANCE);
        scene.version += 1;
        Ok(())
    }
}

/// Rotate every shared vertex at a set of positions, then autofold.
pub struct RotateVerticesCommand {
    pub src: Vec<DVec3>,
    pub center: DVec3,
    pub axis: DVec3,
    pub degrees: f64,
}

impl Command for RotateVerticesCommand {
    fn name(&self) -> &str {
        "RotateVerticesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let keys: HashSet<Key> = self.src.iter().map(|&p| tkey(p)).collect();
        let mat = rotation_matrix(self.center, self.axis, self.degrees);
        let m = scene.mesh_mut();
        rotate_points(m, &keys, &mat);
        fold_nonplanar_faces(m, PLANAR_TOLERANCE);
        scene.version += 1;
        Ok(())
    }
}

/// Mirror loose positions about an axis plane and re-reverse the fully
/// selected faces' windings.
pub struct FlipVerticesCommand {
    pub src: Vec<DVec3>,
    pub centre: DVec3,
    pub axis: DVec3,
    pub faces: Vec<FaceId>,
}

impl Command for FlipVerticesCommand {
    fn name(&self) -> &str {
        "FlipVerticesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let keys: HashSet<Key> = self.src.iter().map(|&p| tkey(p)).collect();
        let mat = mirror_matrix(self.centre, self.axis);
        let m = scene.mesh_mut();
        rotate_points(m, &keys, &mat);
        for &f in &self.faces {
            if m.has_face(f) {
                let fc = m.face_mut(f);
                fc.outer.reverse();
                for h in fc.holes.iter_mut() {
                    h.reverse();
                }
            }
        }
        fold_nonplanar_faces(m, PLANAR_TOLERANCE);
        scene.version += 1;
        Ok(())
    }
}

/// Scale every shared vertex at a set of positions, then autofold.
pub struct ScaleVerticesCommand {
    pub src: Vec<DVec3>,
    pub center: DVec3,
    pub factor: ScaleFactor,
    pub axes: Option<(DVec3, DVec3, DVec3)>,
}

impl Command for ScaleVerticesCommand {
    fn name(&self) -> &str {
        "ScaleVerticesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let keys: HashSet<Key> = self.src.iter().map(|&p| tkey(p)).collect();
        let mat = scale_matrix(self.center, self.factor, self.axes);
        let m = scene.mesh_mut();
        rotate_points(m, &keys, &mat);
        fold_nonplanar_faces(m, PLANAR_TOLERANCE);
        scene.version += 1;
        Ok(())
    }
}

// ---- Cleanup passes -------------------------------------------------------------

/// Remove edges incident to `vertices` that border no face.
pub struct PruneOrphanEdgesCommand {
    pub vertices: Vec<DVec3>,
}

impl Command for PruneOrphanEdgesCommand {
    fn name(&self) -> &str {
        "PruneOrphanEdgesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mid = scene.active_mesh_id();
        let m = scene.mesh_mut();
        let removed = orphaned_edges_at(m, &self.vertices);
        for &e in &removed {
            m.remove_edge(e);
        }
        if !removed.is_empty() {
            for e in removed {
                discard_edge(scene, mid, e);
            }
            scene.version += 1;
        }
        Ok(())
    }
}

/// Dissolve coplanar seams around the operation's vertices.
pub struct CoplanarMergeCommand {
    pub seed: Vec<DVec3>,
}

impl Command for CoplanarMergeCommand {
    fn name(&self) -> &str {
        "CoplanarMergeCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let seedkeys: HashSet<Key> = self.seed.iter().map(|&p| tkey(p)).collect();
        let m = scene.mesh_mut();
        let mut merged_any = false;
        loop {
            let mut progress = false;
            for e in m.edge_ids() {
                if !m.has_edge(e) || m.edge(e).faces.len() != 2 {
                    continue;
                }
                let (a, b) = m.edge_ab(e);
                if !seedkeys.contains(&tkey(a)) && !seedkeys.contains(&tkey(b)) {
                    continue;
                }
                if m.dissolve_edge(e).is_some() {
                    progress = true;
                    merged_any = true;
                    break;
                }
            }
            if !progress {
                break;
            }
        }
        if merged_any {
            scene.version += 1;
        }
        Ok(())
    }
}

pub use xtrude_core::stitch::{coplanar_component, run_stitch, seed_bounds};

/// Make a solid watertight again after push/pull.
pub struct StitchSolidCommand {
    pub seed: Vec<DVec3>,
}

impl Command for StitchSolidCommand {
    fn name(&self) -> &str {
        "StitchSolidCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        run_stitch(scene.mesh_mut(), &self.seed, None, true, true);
        scene.version += 1;
        Ok(())
    }
}

/// Run a list of sub-commands under one step, then clean up any coplanar
/// overlap they created and re-split curve contours (the line-draw plan).
pub struct SnapshotCompound {
    pub inner: Vec<Box<dyn Command>>,
}

impl SnapshotCompound {
    pub fn new(inner: Vec<Box<dyn Command>>) -> Self {
        SnapshotCompound { inner }
    }
}

impl Command for SnapshotCompound {
    fn name(&self) -> &str {
        "SnapshotCompound"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let footprint = winding_footprint(scene.mesh());
        for c in self.inner.iter_mut() {
            c.apply(scene)?;
        }
        let mid = scene.active_mesh_id();
        let removed = heal_overlapping_faces(scene.mesh_mut(), 0.5, None, footprint.as_ref());
        for f in removed {
            discard_face(scene, mid, f);
        }
        scene.mesh_mut().resplit_curves();
        scene.version += 1;
        Ok(())
    }
}

/// Run sub-commands plus the stitch pass as one step.
pub struct MeshSnapshotCommand {
    pub inner: Vec<Box<dyn Command>>,
    pub seed: Vec<DVec3>,
}

impl Command for MeshSnapshotCommand {
    fn name(&self) -> &str {
        "MeshSnapshotCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let before: HashSet<FaceId> = scene.mesh().face_ids().into_iter().collect();
        for c in self.inner.iter_mut() {
            c.apply(scene)?;
        }
        let mut new_faces: HashSet<FaceId> = scene
            .mesh()
            .face_ids()
            .into_iter()
            .filter(|f| !before.contains(f))
            .collect();
        run_stitch(
            scene.mesh_mut(),
            &self.seed,
            Some(&mut new_faces),
            true,
            true,
        );
        scene.version += 1;
        Ok(())
    }
}

/// Remove redundant 'mother' faces left overlapping their subdivisions.
#[derive(Default)]
pub struct HealOverlapsCommand {
    pub healed: usize,
}

impl Command for HealOverlapsCommand {
    fn name(&self) -> &str {
        "HealOverlapsCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let mid = scene.active_mesh_id();
        let fp = winding_footprint(scene.mesh());
        let removed = heal_overlapping_faces(scene.mesh_mut(), 0.5, None, fp.as_ref());
        self.healed = removed.len();
        for f in removed {
            discard_face(scene, mid, f);
        }
        scene.version += 1;
        Ok(())
    }
}

/// Coplanar point-in-triangle via consistent cross-product orientation.
pub fn point_in_tri(p: DVec3, t0: DVec3, t1: DVec3, t2: DVec3) -> bool {
    let n = (t1 - t0).cross(t2 - t0);
    if n.length() < 1e-12 {
        return false;
    }
    for (a, b) in [(t0, t1), (t1, t2), (t2, t0)] {
        let c = (b - a).cross(p - a);
        if c.dot(n) < -1e-9 {
            return false;
        }
    }
    true
}

/// Rebuild the minimal faces of a flat drawing from its edge graph.
#[derive(Default)]
pub struct RebuildPlanarFacesCommand {
    pub rebuilt: usize,
    pub flat: bool,
}

impl RebuildPlanarFacesCommand {
    pub fn new() -> Self {
        RebuildPlanarFacesCommand {
            rebuilt: 0,
            flat: true,
        }
    }
}

impl Command for RebuildPlanarFacesCommand {
    fn name(&self) -> &str {
        "RebuildPlanarFacesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        self.flat = true;
        let mesh = scene.mesh();
        let footprint = winding_footprint(mesh);
        if mesh.edges.is_empty() {
            self.flat = false;
            return Ok(());
        }
        let verts: Vec<DVec3> = mesh.verts.iter().map(|(_, v)| v.position).collect();
        let Some((mut origin, mut normal)) = coplanar_plane(&verts, 1e-4) else {
            self.flat = false;
            return Ok(());
        };
        if let Some(f) = mesh.face_ids().first().copied() {
            normal = mesh.face_normal(f);
            origin = mesh.face_positions(f)[0];
        }
        let segments: Vec<(DVec3, DVec3)> = mesh
            .edge_ids()
            .into_iter()
            .map(|e| mesh.edge_ab(e))
            .collect();
        let flagged: Vec<(DVec3, DVec3, EdgeFlags)> = mesh
            .edges
            .iter()
            .filter(|(_, e)| !e.is_plain())
            .map(|(id, e)| {
                let (a, b) = mesh.edge_ab(id);
                (a, b, e.flags())
            })
            .collect();
        let old_attrs: Vec<(Vec<[DVec3; 3]>, FaceAttrs)> = mesh
            .faces
            .iter()
            .filter(|(_, f)| !f.attrs.is_empty())
            .map(|(id, f)| (mesh.face_triangulate(id, None), f.attrs.clone()))
            .collect();
        let (edges, faces) = planar_rebuild(&segments, origin, normal);
        scene.selection.clear();
        let m = scene.mesh_mut();
        m.clear();
        for (a, b) in edges {
            let Ok(e) = m.add_edge(a, b) else { continue };
            let (ea, eb) = m.edge_ab(e);
            for (fa, fb, flags) in &flagged {
                if point_on_seg_incl(ea, *fa, *fb, SPLIT_TOLERANCE)
                    && point_on_seg_incl(eb, *fa, *fb, SPLIT_TOLERANCE)
                {
                    m.edge_mut(e).stamp_flags(flags);
                    break;
                }
            }
        }
        for (outer, holes) in &faces {
            let f = m.add_face(outer, holes);
            if old_attrs.is_empty() {
                continue;
            }
            let tris = m.face_triangulate(f, None);
            let Some(t) = tris.first() else { continue };
            let probe = (t[0] + t[1] + t[2]) / 3.0;
            for (old_tris, attrs) in &old_attrs {
                if old_tris
                    .iter()
                    .any(|t| point_in_tri(probe, t[0], t[1], t[2]))
                {
                    m.faces[f].attrs.update(attrs);
                    break;
                }
            }
        }
        if let Some(fp) = &footprint {
            orient_coplanar_faces(m, Some(fp));
        }
        m.resplit_curves();
        self.rebuilt = faces.len();
        scene.version += 1;
        Ok(())
    }
}

/// Rebuild the faces of ONE plane of a (possibly 3D) mesh from that
/// plane's edge subgraph, with coverage semantics: a region keeps a face
/// only if an existing on-plane face covered it.
pub struct RebuildPlaneFacesCommand {
    pub origin: DVec3,
    pub normal: DVec3,
    pub rebuilt: usize,
}

impl RebuildPlaneFacesCommand {
    const TOL: f64 = 1e-4;

    pub fn new(origin: DVec3, normal: DVec3) -> Self {
        let len = normal.length();
        RebuildPlaneFacesCommand {
            origin,
            normal: if len > 1e-9 {
                normal / len
            } else {
                DVec3::ZERO
            },
            rebuilt: 0,
        }
    }

    fn on_plane(&self, p: DVec3) -> bool {
        (p - self.origin).dot(self.normal).abs() < Self::TOL
    }
}

impl Command for RebuildPlaneFacesCommand {
    fn name(&self) -> &str {
        "RebuildPlaneFacesCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        if self.normal.length_squared() < 0.5 {
            return Ok(()); // no plane, nothing to rebuild
        }
        let mesh = scene.mesh();
        let plane_edges: Vec<_> = mesh
            .edge_ids()
            .into_iter()
            .filter(|&e| {
                let (a, b) = mesh.edge_ab(e);
                self.on_plane(a) && self.on_plane(b)
            })
            .collect();
        if plane_edges.is_empty() {
            return Ok(());
        }
        let plane_faces: Vec<FaceId> = mesh
            .face_ids()
            .into_iter()
            .filter(|&f| {
                mesh.face_positions(f).iter().all(|&p| self.on_plane(p))
                    && mesh
                        .face_hole_positions(f)
                        .iter()
                        .flatten()
                        .all(|&p| self.on_plane(p))
            })
            .collect();
        let old: Vec<(Vec<[DVec3; 3]>, DVec3, FaceAttrs)> = plane_faces
            .iter()
            .map(|&f| {
                (
                    mesh.face_triangulate(f, None),
                    mesh.face_normal(f),
                    mesh.face(f).attrs.clone(),
                )
            })
            .collect();
        let segments: Vec<(DVec3, DVec3)> = plane_edges.iter().map(|&e| mesh.edge_ab(e)).collect();
        let (_, regions) = planar_rebuild(&segments, self.origin, self.normal);
        let mut keep = Vec::new();
        for (outer, holes) in regions {
            let tris = triangulate(&outer, &holes, Some(self.normal));
            let Some(t) = tris.first() else { continue };
            let probe = (t[0] + t[1] + t[2]) / 3.0;
            let cov = old
                .iter()
                .find(|(ot, _, _)| ot.iter().any(|t| point_in_tri(probe, t[0], t[1], t[2])));
            let Some((_, nrm, attrs)) = cov else { continue };
            let (mut outer, mut holes) = (outer, holes);
            if xtrude_core::math::newell(&outer).dot(*nrm) < 0.0 {
                outer.reverse();
                for h in holes.iter_mut() {
                    h.reverse();
                }
            }
            keep.push((outer, holes, attrs.clone()));
        }
        let m = scene.mesh_mut();
        for f in plane_faces {
            m.remove_face(f);
        }
        for (outer, holes, attrs) in &keep {
            let f = m.add_face(outer, holes);
            if !attrs.is_empty() {
                m.faces[f].attrs.update(attrs);
            }
        }
        prune_collinear_orphan_edges(m);
        m.resplit_curves();
        self.rebuilt = keep.len();
        scene.selection.clear();
        scene.version += 1;
        Ok(())
    }
}

/// Merge near-coplanar faces of the active mesh (Edit ▸ Simplify Mesh).
pub struct SimplifyMeshCommand {
    pub max_angle_degrees: f64,
    pub faces_removed: usize,
}

impl Command for SimplifyMeshCommand {
    fn name(&self) -> &str {
        "SimplifyMeshCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        self.faces_removed =
            xtrude_core::fuse::simplify_mesh(scene.mesh_mut(), self.max_angle_degrees);
        scene.selection.clear();
        scene.version += 1;
        Ok(())
    }
}

/// Error helper for commands.
pub fn fail(msg: &str) -> CmdError {
    CmdError::msg(msg)
}
