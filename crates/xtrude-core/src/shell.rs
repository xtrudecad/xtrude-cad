// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Shell: hollow a closed solid to a wall thickness.
//!
//! Every face of the solid gets an inner twin, moved inward along its
//! normal by the thickness and turned to face the cavity. Each corner of
//! the cavity lies where the offset planes of its corner's faces meet. An
//! open face is not offset: its corners stay on its plane, and the face
//! becomes a rim, the ring between its outline and the cavity's. With no
//! open face the cavity is a second, closed shell inside the first.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::attrs::FaceAttrs;
use crate::math::{newell, DVec3, MIN_FACE_AREA};
use crate::mesh::{FaceId, Mesh, VertexId};

/// Two face normals closer than this (1 − cos) are one plane.
const SAME_PLANE: f64 = 1e-9;
/// A normal adds a direction to a corner when this much of it is new.
const NEW_DIRECTION: f64 = 1e-6;
/// An inner corner may miss an offset plane by this share of the
/// thickness.
const CORNER_TOL: f64 = 1e-6;

/// Why a solid cannot be hollowed; the message is for the user.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ShellError {
    #[error("Pick a solid to hollow.")]
    NoSolid,
    #[error("The wall thickness must be positive.")]
    NotPositive,
    #[error("That is not a closed solid.")]
    NotClosed,
    #[error("Every face would be open: nothing would be left.")]
    AllOpen,
    #[error("The faces at {0} have no common inner corner.")]
    NoCorner(String),
    #[error("The wall is too thick: a face would turn inside out.")]
    TooThick,
}

/// A face the shell adds: an outline and its holes.
#[derive(Clone, Debug)]
pub struct ShellFace {
    pub outer: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    pub attrs: FaceAttrs,
}

/// What [`apply_shell`] does.
#[derive(Clone, Debug, Default)]
pub struct ShellPlan {
    pub thickness: f64,
    /// The open faces; each leaves, its rim takes its place.
    pub removed_faces: Vec<FaceId>,
    /// The cavity's faces, facing into it.
    pub cavity: Vec<ShellFace>,
    /// The rims: one ring per loop of an open face.
    pub rims: Vec<ShellFace>,
}

fn fmt(p: DVec3) -> String {
    format!("({:.2}, {:.2}, {:.2})", p.x, p.y, p.z)
}

/// The smallest move `d` with `n · d = b` for every `(n, b)`, or `None`
/// when no move meets them all. The normals are unit length.
fn corner_move(constraints: &[(DVec3, f64)], tol: f64) -> Option<DVec3> {
    // An independent basis of the normals; the move is a combination of it.
    let mut basis: Vec<(DVec3, f64)> = Vec::new();
    let mut span: Vec<DVec3> = Vec::new();
    for &(n, b) in constraints {
        let mut rest = n;
        for &s in &span {
            rest -= s * rest.dot(s);
        }
        if rest.length() > NEW_DIRECTION && basis.len() < 3 {
            span.push(rest.normalize());
            basis.push((n, b));
        }
    }
    // Gram system G c = b, then d = Σ c_i n_i.
    let k = basis.len();
    let mut g = [[0.0f64; 4]; 3];
    for i in 0..k {
        for j in 0..k {
            g[i][j] = basis[i].0.dot(basis[j].0);
        }
        g[i][3] = basis[i].1;
    }
    for col in 0..k {
        let pivot = (col..k).max_by(|&a, &b| g[a][col].abs().total_cmp(&g[b][col].abs()))?;
        if g[pivot][col].abs() < 1e-12 {
            return None;
        }
        g.swap(col, pivot);
        let top = g[col];
        for (row, r) in g.iter_mut().enumerate().take(k) {
            if row != col {
                let f = r[col] / top[col];
                for (x, t) in r.iter_mut().zip(top).skip(col) {
                    *x -= f * t;
                }
            }
        }
    }
    let d: DVec3 = (0..k).map(|i| basis[i].0 * (g[i][3] / g[i][i])).sum();
    constraints
        .iter()
        .all(|&(n, b)| (n.dot(d) - b).abs() <= tol)
        .then_some(d)
}

/// Plan hollowing the closed solid made of `faces` to `thickness`, leaving
/// `open` (some of `faces`) open.
pub fn plan_shell(
    mesh: &Mesh,
    faces: &[FaceId],
    open: &[FaceId],
    thickness: f64,
) -> Result<ShellPlan, ShellError> {
    let mut solid: Vec<FaceId> = Vec::new();
    for &f in faces {
        if mesh.has_face(f) && !solid.contains(&f) {
            solid.push(f);
        }
    }
    if solid.is_empty() {
        return Err(ShellError::NoSolid);
    }
    if thickness <= 0.0 {
        return Err(ShellError::NotPositive);
    }
    let open: HashSet<FaceId> = open.iter().copied().filter(|f| solid.contains(f)).collect();
    if open.len() == solid.len() {
        return Err(ShellError::AllOpen);
    }

    // Closed: every edge of the set bordered by exactly two of its faces.
    let mut uses: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    for &f in &solid {
        for lp in mesh.face(f).loops() {
            let n = lp.len();
            for i in 0..n {
                let (a, b) = (lp[i], lp[(i + 1) % n]);
                *uses.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
            }
        }
    }
    if uses.values().any(|&c| c != 2) {
        return Err(ShellError::NotClosed);
    }
    let mut volume = 0.0;
    for &f in &solid {
        for [a, b, c] in mesh.face_triangulate(f, None) {
            volume += a.dot(b.cross(c)) / 6.0;
        }
    }
    if volume.abs() < 1e-15 {
        return Err(ShellError::NotClosed);
    }
    // Outward normals, whichever way the faces are wound.
    let outward = volume.signum();
    let normals: HashMap<FaceId, DVec3> = solid
        .iter()
        .map(|&f| (f, mesh.face_normal(f) * outward))
        .collect();

    // The cavity's corners.
    let in_solid: HashSet<FaceId> = solid.iter().copied().collect();
    let mut inner: IndexMap<VertexId, DVec3> = IndexMap::new();
    for &f in &solid {
        for &v in mesh.face(f).loops().flatten() {
            if inner.contains_key(&v) {
                continue;
            }
            let mut planes: Vec<(DVec3, f64)> = Vec::new();
            for g in mesh.vertex_faces(v) {
                if !in_solid.contains(&g) {
                    continue;
                }
                let n = normals[&g];
                let b = if open.contains(&g) { 0.0 } else { -thickness };
                match planes.iter().find(|(m, _)| 1.0 - m.dot(n) < SAME_PLANE) {
                    Some(&(_, c)) if (c - b).abs() > 0.0 => {
                        return Err(ShellError::NoCorner(fmt(mesh.pos(v))));
                    }
                    Some(_) => {}
                    None => planes.push((n, b)),
                }
            }
            let d = corner_move(&planes, CORNER_TOL * thickness)
                .ok_or_else(|| ShellError::NoCorner(fmt(mesh.pos(v))))?;
            inner.insert(v, mesh.pos(v) + d);
        }
    }
    let inner_loop = |lp: &[VertexId]| -> Vec<DVec3> { lp.iter().map(|v| inner[v]).collect() };

    // A loop keeps every edge's direction and its area, or the wall is too
    // thick for it.
    let keeps_shape = |lp: &[VertexId]| -> bool {
        let n = lp.len();
        let edges_hold = (0..n).all(|i| {
            let (a, b) = (lp[i], lp[(i + 1) % n]);
            (inner[&b] - inner[&a]).dot(mesh.pos(b) - mesh.pos(a)) > 0.0
        });
        let before = newell(&mesh.loop_positions(lp));
        let after = newell(&inner_loop(lp));
        edges_hold && after.dot(before) > 0.0 && 0.5 * after.length() > MIN_FACE_AREA
    };

    let mut plan = ShellPlan {
        thickness,
        ..ShellPlan::default()
    };
    for &f in &solid {
        let face = mesh.face(f);
        if !face.loops().all(|lp| keeps_shape(lp)) {
            return Err(ShellError::TooThick);
        }
        let attrs = face.attrs.clone();
        if open.contains(&f) {
            // The rim: a ring between each loop and its inner twin.
            let n = mesh.face_newell(f);
            for (k, lp) in face.loops().enumerate() {
                let edge = mesh.loop_positions(lp);
                let twin = inner_loop(lp);
                let (mut outer, hole) = if k == 0 { (edge, twin) } else { (twin, edge) };
                if newell(&outer).dot(n) < 0.0 {
                    outer.reverse();
                }
                plan.rims.push(ShellFace {
                    outer,
                    holes: vec![hole],
                    attrs: attrs.clone(),
                });
            }
            plan.removed_faces.push(f);
        } else {
            let rev = |lp: &[VertexId]| -> Vec<DVec3> {
                let mut l = inner_loop(lp);
                l.reverse();
                l
            };
            plan.cavity.push(ShellFace {
                outer: rev(&face.outer),
                holes: face.holes.iter().map(|h| rev(h)).collect(),
                attrs,
            });
        }
    }
    Ok(plan)
}

/// Apply a plan to the mesh.
pub fn apply_shell(mesh: &mut Mesh, plan: &ShellPlan) {
    for &f in &plan.removed_faces {
        mesh.remove_face(f);
    }
    for sf in plan.cavity.iter().chain(&plan.rims) {
        let f = mesh.add_face(&sf.outer, &sf.holes);
        mesh.faces[f].attrs = sf.attrs.clone();
    }
}
