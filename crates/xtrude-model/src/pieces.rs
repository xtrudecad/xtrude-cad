// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Split a group into its physical pieces: the solids that do not touch.
//!
//! A piece is found in two passes ([`solid_parts`]). First the solids:
//! around every edge, each face is paired with the face it meets turning
//! into the material behind it ([`edge_partners`]) — on an ordinary edge
//! the other face, where two boards meet the face of the same board. A
//! result counts as a solid when it closes, has thickness and at least four
//! faces. Then the fragments cluster through shared vertices and join the
//! one solid most of their corners lie on.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use xtrude_core::math::{newell, DMat4, DVec3, VecExt};
use xtrude_core::mesh::{EdgeId, FaceId, Mesh, VertexId};

use crate::group::{add_mesh, world_mesh, Group, MeshTable};

/// A result thinner than this is a sheet, not a board (metres).
pub const MIN_THICKNESS: f64 = 0.001;
/// A fragment joins a solid when at least this share of its corners lie on
/// it.
pub const ATTACH_SHARE: f64 = 0.5;
const COINCIDENT: f64 = 1e-3;

/// A piece: its faces and edges.
pub type Part = (Vec<FaceId>, Vec<EdgeId>);

struct Uf {
    parent: HashMap<u64, u64>,
}

impl Uf {
    fn new() -> Self {
        Uf {
            parent: HashMap::new(),
        }
    }
    fn find(&mut self, x: u64) -> u64 {
        let mut root = x;
        while let Some(&p) = self.parent.get(&root) {
            if p == root {
                break;
            }
            root = p;
        }
        let mut cur = x;
        while let Some(&p) = self.parent.get(&cur) {
            if p == root || p == cur {
                break;
            }
            self.parent.insert(cur, root);
            cur = p;
        }
        root
    }
    fn join(&mut self, a: u64, b: u64) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

fn fkey(f: FaceId) -> u64 {
    f.raw() as u64
}

fn vkey(v: VertexId) -> u64 {
    (1u64 << 40) | v.raw() as u64
}

/// `mesh` cut into its connected pieces, largest first.
pub fn connected_parts(mesh: &Mesh) -> Vec<Part> {
    let mut uf = Uf::new();
    for (_, f) in mesh.faces.iter() {
        let vs: Vec<VertexId> = f.loops().flatten().copied().collect();
        for w in vs.windows(2) {
            uf.join(vkey(w[0]), vkey(w[1]));
        }
    }
    for (_, e) in mesh.edges.iter() {
        uf.join(vkey(e.v0), vkey(e.v1));
    }
    let mut parts: IndexMap<u64, Part> = IndexMap::new();
    for (fid, f) in mesh.faces.iter() {
        let r = uf.find(vkey(f.outer[0]));
        parts.entry(r).or_default().0.push(fid);
    }
    for (eid, e) in mesh.edges.iter() {
        let r = uf.find(vkey(e.v0));
        parts.entry(r).or_default().1.push(eid);
    }
    let mut out: Vec<Part> = parts.into_values().collect();
    out.sort_by_key(|p| (std::cmp::Reverse(p.0.len()), std::cmp::Reverse(p.1.len())));
    out
}

pub fn is_solid(mesh: &Mesh, faces: &[FaceId]) -> bool {
    if faces.len() < 4 {
        return false;
    }
    let mut uses: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    for &f in faces {
        for ring in mesh.face(f).loops() {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let k = if a < b { (a, b) } else { (b, a) };
                *uses.entry(k).or_insert(0) += 1;
            }
        }
    }
    if uses.values().any(|c| c % 2 != 0) {
        return false;
    }
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for &f in faces {
        for &v in &mesh.face(f).outer {
            lo = lo.min(mesh.pos(v));
            hi = hi.max(mesh.pos(v));
        }
    }
    let ext = hi - lo;
    ext.x.min(ext.y).min(ext.z) >= MIN_THICKNESS
}

fn inward(mesh: &Mesh, face: FaceId, a: VertexId, b: VertexId, n: DVec3) -> Option<DVec3> {
    for (k, ring) in mesh.face(face).loops().enumerate() {
        let m = ring.len();
        for i in 0..m {
            let (p, q) = (ring[i], ring[(i + 1) % m]);
            if (p == a && q == b) || (p == b && q == a) {
                let mut w = n.cross(mesh.pos(q) - mesh.pos(p));
                if k > 0 {
                    let hn = newell(&mesh.loop_positions(ring));
                    if hn.dot(n) > 0.0 {
                        w = -w;
                    }
                }
                if w.length_squared() == 0.0 {
                    return None;
                }
                return Some(w.unit_or_zero());
            }
        }
    }
    None
}

/// `(face, partner)` pairs around `edge`: each face paired with the face
/// reached by turning about the edge INTO the material behind it.
pub fn edge_partners(mesh: &Mesh, edge: EdgeId) -> Vec<(FaceId, FaceId)> {
    let faces = mesh.edge(edge).faces.clone();
    if faces.len() < 3 {
        return if faces.len() == 2 {
            vec![(faces[0], faces[1])]
        } else {
            Vec::new()
        };
    }
    let (a, b) = (mesh.edge(edge).v0, mesh.edge(edge).v1);
    let axis = mesh.pos(b) - mesh.pos(a);
    if axis.length_squared() == 0.0 {
        return Vec::new();
    }
    let axis = axis.normalize();
    let mut refs: Option<(DVec3, DVec3)> = None;
    let mut info = Vec::new();
    for &f in &faces {
        let n = mesh.face_normal(f);
        let Some(w) = inward(mesh, f, a, b, n) else {
            continue;
        };
        let (ru, rv) = *refs.get_or_insert((w, axis.cross(w)));
        let theta = w.dot(rv).atan2(w.dot(ru));
        let spin = axis.cross(w).dot(n);
        info.push((f, theta, if spin > 0.0 { -1.0 } else { 1.0 }, w, n));
    }
    let two_pi = 2.0 * std::f64::consts::PI;
    let mut pairs = Vec::new();
    for &(f, theta, s, w_f, _) in &info {
        let mut best: Option<(FaceId, (i64, i32))> = None;
        for &(g, phi, _, _, n_g) in &info {
            if g == f {
                continue;
            }
            let mut d = ((phi - theta) * s).rem_euclid(two_pi);
            if d < COINCIDENT || two_pi - d < COINCIDENT {
                d = two_pi;
            }
            let facing = if n_g.dot(w_f) < 0.0 { 0 } else { 1 };
            let key = (xtrude_core::math::round_even_int(d / COINCIDENT), facing);
            if best.as_ref().is_none_or(|(_, bk)| key < *bk) {
                best = Some((g, key));
            }
        }
        if let Some((g, _)) = best {
            pairs.push((f, g));
        }
    }
    pairs
}

/// `mesh` cut into its physical pieces, largest first.
pub fn solid_parts(mesh: &Mesh) -> Vec<Part> {
    let mut uf = Uf::new();
    for (eid, e) in mesh.edges.iter() {
        let mut pairs = edge_partners(mesh, eid);
        if e.faces.len() > 2 {
            let chosen: HashMap<FaceId, FaceId> = pairs.iter().copied().collect();
            pairs.retain(|(f, g)| chosen.get(g) == Some(f));
        }
        for (f, g) in pairs {
            uf.join(fkey(f), fkey(g));
        }
    }
    let mut groups: IndexMap<u64, Vec<FaceId>> = IndexMap::new();
    for f in mesh.face_ids() {
        groups.entry(uf.find(fkey(f))).or_default().push(f);
    }
    let comps: Vec<Vec<FaceId>> = groups.into_values().collect();
    let solid: Vec<bool> = comps.iter().map(|c| is_solid(mesh, c)).collect();
    let mut owner: HashMap<VertexId, usize> = HashMap::new();
    for (i, c) in comps.iter().enumerate() {
        if solid[i] {
            for &f in c {
                for ring in mesh.face(f).loops() {
                    for &v in ring {
                        owner.entry(v).or_insert(i);
                    }
                }
            }
        }
    }
    let mut uf2 = Uf::new();
    let ckey = |i: usize| (2u64 << 40) | i as u64;
    for (i, c) in comps.iter().enumerate() {
        if solid[i] {
            continue;
        }
        for &f in c {
            for ring in mesh.face(f).loops() {
                for &v in ring {
                    uf2.join(vkey(v), ckey(i));
                }
            }
        }
    }
    let mut clusters: IndexMap<u64, Vec<usize>> = IndexMap::new();
    for (i, _) in comps.iter().enumerate() {
        if !solid[i] {
            let r = uf2.find(ckey(i));
            clusters.entry(r).or_default().push(i);
        }
    }
    #[derive(Hash, PartialEq, Eq, Clone, Copy)]
    enum PKey {
        Solid(usize),
        Fragment(usize),
    }
    let mut pieces: IndexMap<PKey, Vec<FaceId>> = IndexMap::new();
    for (i, c) in comps.iter().enumerate() {
        if solid[i] {
            pieces.insert(PKey::Solid(i), c.clone());
        }
    }
    for members in clusters.values() {
        let fs: Vec<FaceId> = members
            .iter()
            .flat_map(|&i| comps[i].iter().copied())
            .collect();
        let corners: HashSet<VertexId> = fs
            .iter()
            .flat_map(|&f| mesh.face(f).loops().flatten().copied().collect::<Vec<_>>())
            .collect();
        let mut votes: IndexMap<usize, usize> = IndexMap::new();
        let mut cl: Vec<VertexId> = corners.iter().copied().collect();
        cl.sort();
        for v in cl {
            if let Some(&k) = owner.get(&v) {
                *votes.entry(k).or_insert(0) += 1;
            }
        }
        let best = votes.iter().max_by_key(|(_, &c)| c).map(|(&k, &c)| (k, c));
        match best {
            Some((k, c)) if c as f64 >= ATTACH_SHARE * corners.len() as f64 => {
                pieces.get_mut(&PKey::Solid(k)).expect("solid").extend(fs);
            }
            _ => {
                pieces.insert(PKey::Fragment(members[0]), fs);
            }
        }
    }
    let keys: Vec<PKey> = pieces.keys().copied().collect();
    let mut face_piece: HashMap<FaceId, usize> = HashMap::new();
    let mut vertex_piece: HashMap<VertexId, usize> = HashMap::new();
    for (n, k) in keys.iter().enumerate() {
        for &f in &pieces[k] {
            face_piece.insert(f, n);
            for ring in mesh.face(f).loops() {
                for &v in ring {
                    vertex_piece.entry(v).or_insert(n);
                }
            }
        }
    }
    let mut out: Vec<Part> = keys
        .iter()
        .map(|k| (pieces[k].clone(), Vec::new()))
        .collect();
    let mut wires: Vec<EdgeId> = Vec::new();
    for (eid, e) in mesh.edges.iter() {
        if !e.faces.is_empty() {
            let mut ns: Vec<usize> = e
                .faces
                .iter()
                .filter_map(|f| face_piece.get(f).copied())
                .collect();
            ns.sort();
            ns.dedup();
            for n in ns {
                out[n].1.push(eid);
            }
            continue;
        }
        match vertex_piece.get(&e.v0).or_else(|| vertex_piece.get(&e.v1)) {
            Some(&n) => out[n].1.push(eid),
            None => wires.push(eid),
        }
    }
    if !wires.is_empty() {
        let mut wm = Mesh::new();
        let mut map = Vec::new();
        for &e in &wires {
            let (a, b) = mesh.edge_ab(e);
            if let Ok(ne) = wm.add_edge(a, b) {
                map.push((ne, e));
            }
        }
        for (_, es) in connected_parts(&wm) {
            let orig: Vec<EdgeId> = es
                .iter()
                .filter_map(|ne| map.iter().find(|(n, _)| n == ne).map(|(_, o)| *o))
                .collect();
            out.push((Vec::new(), orig));
        }
    }
    out.sort_by_key(|p| (std::cmp::Reverse(p.0.len()), std::cmp::Reverse(p.1.len())));
    out
}

/// A new mesh holding a deep copy of `faces` and `edges` (attrs, holes and
/// every edge flag included).
pub fn copy_part(mesh: &Mesh, faces: &[FaceId], edges: &[EdgeId]) -> Mesh {
    let mut new = Mesh::new();
    let mut flat: Vec<DVec3> = Vec::new();
    for &e in edges {
        let (a, b) = mesh.edge_ab(e);
        flat.push(a);
        flat.push(b);
    }
    let n_edge = flat.len();
    let mut rings = Vec::new();
    for &f in faces {
        let mut holes = mesh.face_hole_positions(f);
        if holes.len() > 1 {
            holes = xtrude_core::topology::maximal_holes(&holes);
        }
        let outer = mesh.face_positions(f);
        flat.extend(outer.iter().copied());
        for h in &holes {
            flat.extend(h.iter().copied());
        }
        rings.push((
            outer.len(),
            holes.iter().map(|h| h.len()).collect::<Vec<_>>(),
            mesh.face(f).attrs.clone(),
        ));
    }
    if flat.is_empty() {
        return new;
    }
    let (vobjs, ids) = new.bulk_weld(&flat);
    if !edges.is_empty() {
        let ia: Vec<usize> = ids[..n_edge].iter().step_by(2).copied().collect();
        let ib: Vec<usize> = ids[..n_edge].iter().skip(1).step_by(2).copied().collect();
        let flags: Vec<_> = edges.iter().map(|&e| mesh.edge(e).flags()).collect();
        let made = new.add_edges_welded(&vobjs, &ia, &ib, Some(&flags));
        for (ne, &e) in made.iter().zip(edges.iter()) {
            if let Some(ne) = ne {
                new.edge_mut(*ne).stamp_flags(&mesh.edge(e).flags());
            }
        }
    }
    let mut k = n_edge;
    for (on, hs, attrs) in rings {
        let o: Vec<_> = (0..on).map(|i| vobjs[ids[k + i]]).collect();
        k += on;
        let mut hl = Vec::new();
        for hn in hs {
            hl.push((0..hn).map(|i| vobjs[ids[k + i]]).collect());
            k += hn;
        }
        let f = new.add_face_welded(o, hl);
        if !attrs.is_empty() {
            new.faces[f].attrs.update(&attrs);
        }
    }
    new.resplit_curves();
    new
}

/// The pieces `group` comes apart into, as new groups in the group's own
/// coordinates (identity placements), or `[]` when there is nothing to
/// split.
pub fn split_into_pieces(meshes: &mut MeshTable, group: &Group) -> Vec<Group> {
    let mut frame = Group::new(group.mesh, Some("frame"));
    frame.children = group.children.clone();
    frame.xform = None;
    let local = world_mesh(meshes, &frame);
    let parts = solid_parts(&local);
    if parts.len() <= 1 && group.children.is_empty() {
        return Vec::new();
    }
    let kids = &group.children;
    let own_faces = meshes.get(&group.mesh).is_some_and(|m| !m.faces.is_empty());
    if !own_faces
        && parts.len() == kids.len()
        && kids.iter().all(|k| {
            k.children.is_empty()
                && meshes
                    .get(&k.mesh)
                    .is_some_and(|m| solid_parts(m).len() == 1)
        })
    {
        return Vec::new();
    }
    let names = inherited_names(meshes, group, &local, &parts);
    let mut pieces = Vec::new();
    for (n, (faces, edges)) in parts.iter().enumerate() {
        let id = add_mesh(meshes, copy_part(&local, faces, edges));
        let name = names[n]
            .clone()
            .unwrap_or_else(|| format!("Piece {}", n + 1));
        let mut p = Group::new(id, Some(&name));
        p.xform = Some(DMat4::IDENTITY);
        p.component = false;
        pieces.push(p);
    }
    pieces
}

fn inherited_names(
    meshes: &MeshTable,
    group: &Group,
    local: &Mesh,
    parts: &[Part],
) -> Vec<Option<String>> {
    let kids = &group.children;
    if kids.is_empty() {
        return vec![None; parts.len()];
    }
    let key = |p: DVec3| {
        (
            xtrude_core::math::round_even_int(p.x * 1e5),
            xtrude_core::math::round_even_int(p.y * 1e5),
            xtrude_core::math::round_even_int(p.z * 1e5),
        )
    };
    let generic = |name: &str| {
        let lower = name.to_lowercase();
        ["part ", "piece "].iter().any(|pre| {
            lower
                .strip_prefix(pre)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
    };
    let mut where_: HashMap<(i64, i64, i64), HashSet<usize>> = HashMap::new();
    for (i, kid) in kids.iter().enumerate() {
        for p in crate::parts::part_points(meshes, kid) {
            where_.entry(key(p)).or_default().insert(i);
        }
    }
    let mut out = Vec::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    for (faces, _) in parts {
        let mut owners: Option<HashSet<usize>> = None;
        'f: for &f in faces {
            for &v in &local.face(f).outer {
                let here = where_.get(&key(local.pos(v))).cloned().unwrap_or_default();
                owners = Some(match owners {
                    None => here,
                    Some(o) => o.intersection(&here).copied().collect(),
                });
                if owners.as_ref().is_some_and(|o| o.is_empty()) {
                    break 'f;
                }
            }
        }
        let mut name = None;
        if let Some(o) = owners.filter(|o| o.len() == 1) {
            let n = kids[*o.iter().next().expect("one")].name.trim().to_string();
            if !n.is_empty() && !generic(&n) {
                name = Some(n);
            }
        }
        if let Some(n) = name.clone() {
            let c = used.entry(n.clone()).or_insert(0);
            *c += 1;
            if *c > 1 {
                name = Some(format!("{n} {c}"));
            }
        }
        out.push(name);
    }
    out
}
