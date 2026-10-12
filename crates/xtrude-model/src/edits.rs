// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Edit operations that turn drawn segments into history commands.
//!
//! The bridge between the pure geometry planner in `topology` and the
//! commands: tools hand it the raw segments the user drew; it returns a
//! single command that splits existing edges the new segments cross, breaks
//! each new segment at those crossings, welds coincident edges, and
//! optionally auto-faces any planar cycle the new sub-edges close.
//!
//! Segments are processed in order against a running simulation of the
//! scene's edge list, so a batch splits correctly even when later segments
//! cross edges created by earlier ones.

use xtrude_core::arrangement::coplanar_plane;
use xtrude_core::geometry::{PolyFace, Segment};
use xtrude_core::math::{newell, DVec3};
use xtrude_core::mesh::{EdgeId, Mesh};
use xtrude_core::topology::{
    face_exists, find_cycles_through, find_duplicate_edge, is_planar, plan_edge_split,
    split_face_by_chord, EdgeSplitPlan, PLANAR_TOLERANCE,
};

use crate::commands::mesh::{
    AddEdgeCommand, AddFaceCommand, DeleteEdgesCommand, DeleteFaceCommand,
    RebuildPlanarFacesCommand, RebuildPlaneFacesCommand, SnapshotCompound,
};
use crate::history::Command;
use crate::scene::Scene;

/// A planned command (kept as an enum so the planner can tell its own
/// rebuild commands apart).
pub enum Planned {
    AddEdge(AddEdgeCommand),
    AddFace(AddFaceCommand),
    DeleteEdges(DeleteEdgesCommand),
    DeleteFace(DeleteFaceCommand),
    RebuildPlanar(RebuildPlanarFacesCommand),
    RebuildPlane(RebuildPlaneFacesCommand),
    Other(Box<dyn Command>),
}

impl Planned {
    fn is_rebuild(&self) -> bool {
        matches!(self, Planned::RebuildPlanar(_) | Planned::RebuildPlane(_))
    }

    pub fn into_command(self) -> Box<dyn Command> {
        match self {
            Planned::AddEdge(c) => Box::new(c),
            Planned::AddFace(c) => Box::new(c),
            Planned::DeleteEdges(c) => Box::new(c),
            Planned::DeleteFace(c) => Box::new(c),
            Planned::RebuildPlanar(c) => Box::new(c),
            Planned::RebuildPlane(c) => Box::new(c),
            Planned::Other(c) => c,
        }
    }
}

/// A segment of the simulated edge list, with the flags its mesh edge had.
#[derive(Clone)]
struct SimEdge {
    seg: Segment,
    soft: bool,
    curve: Option<i64>,
}

fn sim_from_mesh(mesh: &Mesh) -> Vec<SimEdge> {
    mesh.edges
        .iter()
        .map(|(id, e)| {
            let (a, b) = mesh.edge_ab(id);
            SimEdge {
                seg: Segment::new(a, b),
                soft: e.soft,
                curve: e.curve,
            }
        })
        .collect()
}

fn segs(sim: &[SimEdge]) -> Vec<Segment> {
    sim.iter().map(|s| s.seg).collect()
}

/// Build the ordered command list to add `segments` to the active mesh.
pub fn plan_edge_commands(
    scene: &Scene,
    segments: &[(DVec3, DVec3)],
    detect_faces: bool,
    eye: Option<DVec3>,
) -> Vec<Planned> {
    let mesh = scene.mesh();
    let mut commands: Vec<Planned> = Vec::new();
    let mut simulated = sim_from_mesh(mesh);
    let mut faces_snapshot: Vec<PolyFace> = mesh
        .face_ids()
        .into_iter()
        .map(|f| {
            let mut pf = mesh.poly_face(f);
            let a = mesh.face(f).attrs.clone();
            pf.attrs = (!a.is_empty()).then_some(a);
            pf
        })
        .collect();

    for &(a, b) in segments {
        let EdgeSplitPlan {
            new_segments,
            edge_cuts,
        } = plan_edge_split(&segs(&simulated), a, b);
        // Process cuts in a stable order; remove by identity from the sim.
        let cut_edges: Vec<(SimEdge, DVec3)> = edge_cuts
            .iter()
            .map(|&(i, p)| (simulated[i].clone(), p))
            .collect();
        let mut remove_idx: Vec<usize> = edge_cuts.iter().map(|&(i, _)| i).collect();
        remove_idx.sort_unstable();
        for (edge, point) in cut_edges {
            commands.push(Planned::DeleteEdges(DeleteEdgesCommand {
                endpoints: vec![(edge.seg.a, edge.seg.b)],
                cascade_faces: false,
            }));
            if let Some(pos) = simulated.iter().position(|s| s.seg == edge.seg) {
                simulated.remove(pos);
            }
            let e_soft = edge.soft.then_some(true);
            for (sa, sb) in [(edge.seg.a, point), (point, edge.seg.b)] {
                if find_duplicate_edge(&segs(&simulated), sa, sb).is_none() {
                    commands.push(Planned::AddEdge(AddEdgeCommand::with_flags(
                        sa, sb, e_soft, edge.curve,
                    )));
                    simulated.push(SimEdge {
                        seg: Segment::new(sa, sb),
                        soft: false,
                        curve: None,
                    });
                }
            }
            // Carry the split into faces sharing this edge — but not the
            // face the drawn segment chord-splits.
            let changed =
                split_edge_in_snapshot(&faces_snapshot, edge.seg.a, edge.seg.b, point, &[a, b]);
            for (fi, new_verts) in changed {
                let f = faces_snapshot[fi].clone();
                commands.push(Planned::DeleteFace(DeleteFaceCommand {
                    lp: f.vertices.clone(),
                }));
                commands.push(Planned::AddFace(AddFaceCommand::plain(
                    new_verts.clone(),
                    f.holes.clone(),
                    f.attrs.clone(),
                )));
                faces_snapshot[fi] = PolyFace {
                    vertices: new_verts,
                    holes: f.holes.clone(),
                    attrs: f.attrs.clone(),
                };
            }
        }
        for (sa, sb) in new_segments {
            let already = find_duplicate_edge(&segs(&simulated), sa, sb);
            commands.push(Planned::AddEdge(AddEdgeCommand::new(sa, sb)));
            if already.is_none() {
                simulated.push(SimEdge {
                    seg: Segment::new(sa, sb),
                    soft: false,
                    curve: None,
                });
            }
            if detect_faces {
                plan_faces(&mut commands, &mut faces_snapshot, &simulated, sa, sb, eye);
            }
        }
    }
    commands
}

fn split_edge_in_snapshot(
    faces: &[PolyFace],
    edge_a: DVec3,
    edge_b: DVec3,
    point: DVec3,
    skip: &[DVec3],
) -> Vec<(usize, Vec<DVec3>)> {
    use xtrude_core::topology::{locate_on_loop, tkey};
    let (ka, kb) = (tkey(edge_a), tkey(edge_b));
    let kp = tkey(point);
    let mut out = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        if !skip.is_empty()
            && skip
                .iter()
                .all(|&p| locate_on_loop(&f.vertices, p).is_some())
        {
            continue;
        }
        let n = f.vertices.len();
        for i in 0..n {
            let j = (i + 1) % n;
            let (ki, kj) = (tkey(f.vertices[i]), tkey(f.vertices[j]));
            if (ki == ka && kj == kb) || (ki == kb && kj == ka) {
                if kp != ki && kp != kj {
                    let mut nv = f.vertices.clone();
                    nv.insert(i + 1, point);
                    out.push((fi, nv));
                }
                break;
            }
        }
    }
    out
}

/// Emit the face commands for a freshly added edge `sa`–`sb`: a chord split
/// of an existing face, or else auto-face the new minimal cycles.
fn plan_faces(
    commands: &mut Vec<Planned>,
    faces_snapshot: &mut Vec<PolyFace>,
    simulated: &[SimEdge],
    sa: DVec3,
    sb: DVec3,
    eye: Option<DVec3>,
) {
    let chord = faces_snapshot
        .iter()
        .enumerate()
        .find_map(|(i, f)| split_face_by_chord(f, sa, sb).map(|(x, y)| (i, x, y)));
    if let Some((mi, loop_a, loop_b)) = chord {
        let mother = faces_snapshot.remove(mi);
        let keep = mother.attrs.clone();
        commands.push(Planned::DeleteFace(DeleteFaceCommand {
            lp: mother.vertices.clone(),
        }));
        for lp in [loop_a, loop_b] {
            let existing: Vec<&[DVec3]> = faces_snapshot
                .iter()
                .map(|f| f.vertices.as_slice())
                .collect();
            if is_planar(&lp, PLANAR_TOLERANCE) && !face_exists(existing, &lp) {
                commands.push(Planned::AddFace(AddFaceCommand::with_attrs(
                    lp.clone(),
                    keep.clone(),
                )));
                faces_snapshot.push(PolyFace {
                    vertices: lp,
                    holes: Vec::new(),
                    attrs: keep.clone(),
                });
            }
        }
        return;
    }
    for cycle in find_cycles_through(&segs(simulated), sa, sb, 2) {
        let existing: Vec<&[DVec3]> = faces_snapshot
            .iter()
            .map(|f| f.vertices.as_slice())
            .collect();
        if is_planar(&cycle, PLANAR_TOLERANCE) && !face_exists(existing, &cycle) {
            let cycle = face_up_or_toward(&cycle, eye);
            commands.push(Planned::AddFace(AddFaceCommand::new(cycle.clone())));
            faces_snapshot.push(PolyFace::new(cycle));
        }
    }
}

fn scene_has_curves(mesh: &Mesh) -> bool {
    mesh.edges.iter().any(|(_, e)| e.curve.is_some())
}

/// Append the deterministic planar-arrangement rebuild after a draw when
/// the cycle planner alone cannot produce the expected regions.
fn append_flat_curve_rebuild(scene: &Scene, commands: &mut Vec<Planned>, points: &[DVec3]) {
    if commands.iter().any(Planned::is_rebuild) {
        return;
    }
    let mesh = scene.mesh();
    let has_curves = scene_has_curves(mesh);
    let mut verts: Vec<DVec3> = mesh.verts.iter().map(|(_, v)| v.position).collect();
    verts.extend_from_slice(points);
    if let Some((mut origin, mut normal)) = coplanar_plane(&verts, 1e-4) {
        if has_curves {
            commands.push(Planned::RebuildPlanar(RebuildPlanarFacesCommand::new()));
        } else if !mesh.faces.is_empty() {
            if let Some(f) = mesh.face_ids().first().copied() {
                normal = mesh.face_normal(f);
                origin = mesh.face_positions(f)[0];
            }
            commands.push(Planned::RebuildPlane(RebuildPlaneFacesCommand::new(
                origin, normal,
            )));
        }
        return;
    }
    if !has_curves {
        return;
    }
    let Some((origin, normal)) = coplanar_plane(points, 1e-4) else {
        return;
    };
    let tol = 1e-4;
    let on_plane = |p: DVec3| (p - origin).dot(normal).abs() < tol;
    let any = mesh.edges.iter().any(|(id, e)| {
        let (a, b) = mesh.edge_ab(id);
        e.curve.is_some() && on_plane(a) && on_plane(b)
    });
    if any {
        commands.push(Planned::RebuildPlane(RebuildPlaneFacesCommand::new(
            origin, normal,
        )));
    }
}

/// Append the scoped plane rebuild when straight edges land on a plane that
/// already carries faces (and it is not a reference-scale plane).
fn append_face_plane_rebuild(scene: &Scene, commands: &mut Vec<Planned>, points: &[DVec3]) {
    if commands.iter().any(Planned::is_rebuild) {
        return;
    }
    let tol = 1e-4;
    let mesh = scene.mesh();
    for f in mesh.face_ids() {
        let origin = mesh.face_positions(f)[0];
        let normal = mesh.face_normal(f);
        if normal.length_squared() < 0.5 {
            continue;
        }
        if points.iter().all(|&p| (p - origin).dot(normal).abs() < tol) {
            let on_plane = mesh
                .face_ids()
                .into_iter()
                .filter(|&g| {
                    (mesh.face_positions(g)[0] - origin).dot(normal).abs() < tol
                        && mesh.face_normal(g).dot(normal).abs() > 0.999
                })
                .count();
            if on_plane <= 400 {
                commands.push(Planned::RebuildPlane(RebuildPlaneFacesCommand::new(
                    origin, normal,
                )));
            }
            return;
        }
    }
}

/// The winding a brand-new face should take: a horizontal one shows its
/// FRONT upwards, any other one faces the `eye` that drew it.
pub fn face_up_or_toward(lp: &[DVec3], eye: Option<DVec3>) -> Vec<DVec3> {
    let pts = lp.to_vec();
    if pts.len() < 3 {
        return pts;
    }
    let n = newell(&pts);
    if n.length() < 1e-12 {
        return pts;
    }
    let want = if n.z.abs() > 0.5 * n.length() {
        n.z > 0.0
    } else if let Some(eye) = eye {
        let c = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
        n.dot(eye - c) > 0.0
    } else {
        return pts;
    };
    if want {
        pts
    } else {
        let mut out = vec![pts[0]];
        out.extend(pts[1..].iter().rev().copied());
        out
    }
}

fn wrap(commands: Vec<Planned>, curves: bool) -> Box<dyn Command> {
    if commands.len() == 1 && !curves {
        return commands.into_iter().next().expect("one").into_command();
    }
    Box::new(SnapshotCompound::new(
        commands.into_iter().map(Planned::into_command).collect(),
    ))
}

/// Single-segment convenience: one drawn edge → one atomic command.
pub fn build_add_edge(
    scene: &Scene,
    a: DVec3,
    b: DVec3,
    detect_faces: bool,
    eye: Option<DVec3>,
) -> Box<dyn Command> {
    let mut commands = plan_edge_commands(scene, &[(a, b)], detect_faces, eye);
    append_flat_curve_rebuild(scene, &mut commands, &[a, b]);
    if detect_faces {
        append_face_plane_rebuild(scene, &mut commands, &[a, b]);
    }
    wrap(commands, scene_has_curves(scene.mesh()))
}

/// Batch convenience: many drawn edges (+ optional `extra` commands such as
/// a tool-managed face) → one atomic command.
pub fn build_add_edges(
    scene: &Scene,
    segments: &[(DVec3, DVec3)],
    detect_faces: bool,
    extra: Vec<Box<dyn Command>>,
    eye: Option<DVec3>,
) -> Box<dyn Command> {
    let mut commands = plan_edge_commands(scene, segments, detect_faces, eye);
    commands.extend(extra.into_iter().map(Planned::Other));
    let pts: Vec<DVec3> = segments.iter().flat_map(|&(a, b)| [a, b]).collect();
    append_flat_curve_rebuild(scene, &mut commands, &pts);
    wrap(commands, scene_has_curves(scene.mesh()))
}

/// Divide: split each selected edge — or, for an edge of a
/// curve, the whole curve — into `n` pieces of equal length; a curve comes
/// out as `n` independent arcs. Returns the number of cuts.
pub fn divide_edges(mesh: &mut Mesh, edges: &[EdgeId], n: usize) -> usize {
    if n < 2 {
        return 0;
    }
    let mut done: std::collections::HashSet<EdgeId> = std::collections::HashSet::new();
    let mut cuts = 0;
    for &edge in edges {
        if done.contains(&edge) || !mesh.has_edge(edge) {
            continue;
        }
        let is_curve = mesh.edge(edge).curve.is_some();
        let chain = if is_curve {
            mesh.curve_edges(edge)
        } else {
            vec![edge]
        };
        let walk = order_chain(mesh, &chain, edge);
        for (e, _, _) in &walk {
            done.insert(*e);
        }
        let total: f64 = walk
            .iter()
            .map(|&(_, vf, vt)| (mesh.pos(vt) - mesh.pos(vf)).length())
            .sum();
        if total < 1e-9 {
            continue;
        }
        let step = total / n as f64;
        let mut pieces: Vec<Vec<EdgeId>> = vec![Vec::new(); n];
        let piece_of = |s0: f64, s1: f64| -> usize {
            (((s0 + s1) * 0.5 / step).floor().max(0.0) as usize).min(n - 1)
        };
        let mut walked = 0.0;
        for (e, v_from, v_to) in walk {
            let (a, b) = (mesh.pos(v_from), mesh.pos(v_to));
            let ln = (b - a).length();
            if ln < 1e-12 {
                continue;
            }
            let flags = mesh.edge(e).flags();
            let inside: Vec<usize> = (1..n)
                .filter(|&k| {
                    walked + 1e-9 < step * (k as f64) && step * (k as f64) < walked + ln - 1e-9
                })
                .collect();
            let (mut rest, mut rest_from, mut rest_start) = (e, v_from, walked);
            for k in inside {
                let at = step * k as f64;
                let point = a + (b - a) * ((at - walked) / ln);
                let (e0, e1) = mesh.split_edge(rest, point);
                if e0 == e1 {
                    break;
                }
                let first = if mesh.edge(e0).has(rest_from) { e0 } else { e1 };
                let second = if first == e0 { e1 } else { e0 };
                for half in [first, second] {
                    mesh.edge_mut(half).stamp_flags(&flags);
                }
                pieces[piece_of(rest_start, at)].push(first);
                let nf = mesh.edge(first).other(rest_from);
                rest = second;
                rest_from = nf;
                rest_start = at;
                cuts += 1;
            }
            pieces[piece_of(rest_start, walked + ln)].push(rest);
            walked += ln;
        }
        if is_curve {
            for facets in pieces {
                if facets.is_empty() {
                    continue;
                }
                let cid = Mesh::next_curve_id();
                for f in facets {
                    if mesh.has_edge(f) {
                        mesh.edge_mut(f).curve = Some(cid);
                    }
                }
            }
        }
    }
    cuts
}

/// The edges of a curve in walking order as `(edge, v_from, v_to)`.
fn order_chain(
    mesh: &Mesh,
    chain: &[EdgeId],
    seed: EdgeId,
) -> Vec<(EdgeId, xtrude_core::VertexId, xtrude_core::VertexId)> {
    if chain.len() <= 1 {
        let e = chain[0];
        return vec![(e, mesh.edge(e).v0, mesh.edge(e).v1)];
    }
    let mut by_vertex: indexmap::IndexMap<xtrude_core::VertexId, Vec<EdgeId>> =
        indexmap::IndexMap::new();
    for &e in chain {
        for v in [mesh.edge(e).v0, mesh.edge(e).v1] {
            by_vertex.entry(v).or_default().push(e);
        }
    }
    let ends: Vec<_> = by_vertex
        .iter()
        .filter(|(_, es)| es.len() == 1)
        .map(|(v, _)| *v)
        .collect();
    let mut v = ends.first().copied().unwrap_or(mesh.edge(seed).v0);
    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        let nxt = by_vertex[&v].iter().copied().find(|e| !seen.contains(e));
        let Some(nxt) = nxt else { break };
        seen.insert(nxt);
        let w = mesh.edge(nxt).other(v);
        ordered.push((nxt, v, w));
        v = w;
        if ordered.len() == chain.len() {
            break;
        }
    }
    ordered
}
