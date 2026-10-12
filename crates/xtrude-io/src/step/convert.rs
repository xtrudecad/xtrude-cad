// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! From the helper's meshes ([`RawStep`]) to the import payload the
//! `.skp` import applies too ([`SkpPayload`]).
//!
//! Every part is welded the way the document's mesh welds (through a
//! scratch [`Mesh`]), so what this module builds is what the document will
//! hold. The welded positions are the mesh's own vertices, which weld to
//! themselves again on apply. On the welded part, triangles that collapse
//! are dropped, and so are slivers flattened onto another triangle; a
//! B-rep face that collapses whole (smaller than the weld tolerance) is
//! counted for the warnings, and a closed part the weld opens (a feature
//! thinner than the tolerance) is warned about.
//!
//! A planar B-rep face becomes one flat polygon with its holes, the
//! boundary of its triangles; a curved face stays triangles, joined by soft
//! edges so it reads and selects as one surface.

use std::collections::{HashMap, HashSet};

use xtrude_core::attrs::{Paint, Rgb};
use xtrude_core::math::newell;
use xtrude_core::mesh::VertexId;
use xtrude_core::{DVec3, Mesh};

use super::protocol::{RawInstance, RawPart, RawStep};
use super::StepWarning;
use crate::skp::{AttrEntry, PayloadFace, Placement, Proto, SkpPayload};
use xtrude_model::i18n::tr;

/// The payload and the warnings for `raw`; `None` when no part has
/// geometry.
pub(super) fn convert(raw: &RawStep) -> Option<(SkpPayload, Vec<StepWarning>)> {
    let keep = parts_with_geometry(raw);
    let roots: Vec<RawInstance> = raw
        .roots
        .iter()
        .map(|r| collapse(raw, &keep, r))
        .filter(|r| keep[r.part])
        .collect();
    if roots.is_empty() {
        return None;
    }
    let children: Vec<Vec<RawInstance>> = raw
        .parts
        .iter()
        .map(|p| {
            p.children
                .iter()
                .map(|c| collapse(raw, &keep, c))
                .filter(|c| keep[c.part])
                .collect()
        })
        .collect();
    let mut uses = vec![0usize; raw.parts.len()];
    for inst in children
        .iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .flat_map(|(_, c)| c)
        .chain(&roots)
    {
        uses[inst.part] += 1;
    }
    let labels = labels(raw, &roots, &children);

    let mut payload = SkpPayload::new("opencascade");
    let mut colors = Colors::default();
    let mut warnings = Vec::new();
    let mut small_faces = 0usize;
    for (index, part) in raw.parts.iter().enumerate() {
        let mut proto = Proto {
            name: Some(part.name.clone()).filter(|n| !n.is_empty()),
            ..Default::default()
        };
        if keep[index] {
            let attrs: Vec<Option<usize>> = part
                .faces
                .iter()
                .map(|f| {
                    f.color
                        .or(part.color)
                        .map(|c| colors.entry(&mut payload, c))
                })
                .collect();
            let built = build_part(part, &attrs);
            small_faces += built.dropped_faces;
            proto.faces = built.faces;
            proto.soft_edges = Some(built.soft_edges);
            proto.children = children[index]
                .iter()
                .map(|c| placement(c, &uses))
                .collect();
            let label = labels[index].clone();
            if !part.faces.is_empty() && !part.closed {
                warnings.push(StepWarning::OpenShell {
                    part: label.clone(),
                });
            } else if !part.faces.is_empty() && !built.closed {
                warnings.push(StepWarning::ThinFeatures {
                    part: label.clone(),
                });
            }
            if part.failed_faces > 0 {
                warnings.push(StepWarning::FacesNotMeshed {
                    part: label,
                    count: part.failed_faces as usize,
                });
            }
        }
        payload.protos.push(proto);
    }
    payload.placements = roots.iter().map(|r| placement(r, &uses)).collect();
    if small_faces > 0 {
        warnings.push(StepWarning::SmallFeatures { count: small_faces });
    }
    Some((payload, warnings))
}

/// The name the warnings give each placed part: its own, else the name
/// it is placed under, else "‹assembly› › unnamed part" after the first
/// assembly above it that has one.
fn labels(raw: &RawStep, roots: &[RawInstance], children: &[Vec<RawInstance>]) -> Vec<String> {
    let mut labels: Vec<Option<String>> = vec![None; raw.parts.len()];
    let own = |inst: &RawInstance| {
        [inst.name.as_str(), raw.parts[inst.part].name.as_str()]
            .into_iter()
            .find(|n| !n.is_empty())
            .map(str::to_string)
    };
    let mut stack: Vec<usize> = Vec::new();
    for r in roots {
        if labels[r.part].is_none() {
            labels[r.part] = Some(own(r).unwrap_or_else(|| tr("unnamed part", &[])));
            stack.push(r.part);
        }
    }
    while let Some(p) = stack.pop() {
        let parent = labels[p].clone().unwrap_or_default();
        for c in &children[p] {
            if labels[c.part].is_none() {
                labels[c.part] = Some(
                    own(c).unwrap_or_else(|| tr("{parent} › unnamed part", &[("parent", &parent)])),
                );
                stack.push(c.part);
            }
        }
    }
    labels.into_iter().map(Option::unwrap_or_default).collect()
}

/// Which parts hold faces, themselves or through their children. A
/// child loop (a broken file) counts as empty.
fn parts_with_geometry(raw: &RawStep) -> Vec<bool> {
    fn visit(raw: &RawStep, i: usize, state: &mut [u8]) -> bool {
        match state[i] {
            1 => return false, // on the current path: a loop
            2 => return true,
            3 => return false,
            _ => {}
        }
        state[i] = 1;
        let part = &raw.parts[i];
        let mut any = !part.faces.is_empty();
        for c in &part.children {
            any |= visit(raw, c.part, state);
        }
        state[i] = if any { 2 } else { 3 };
        any
    }
    let mut state = vec![0u8; raw.parts.len()];
    (0..raw.parts.len())
        .map(|i| visit(raw, i, &mut state))
        .collect()
}

/// `inst`, with any chain of face-less assemblies holding a single part
/// with geometry folded into one placement of the part at its end:
/// OpenCascade reads a compound as such an assembly, and a one-part file
/// would otherwise import as three nested groups. The outer name wins.
fn collapse(raw: &RawStep, keep: &[bool], inst: &RawInstance) -> RawInstance {
    let mut out = inst.clone();
    // A chain that loops (a broken file) stops after as many steps as
    // there are parts.
    for _ in 0..raw.parts.len() {
        let part = &raw.parts[out.part];
        let mut kept = part.children.iter().filter(|c| keep[c.part]);
        let (Some(child), None) = (kept.next(), kept.next()) else {
            break;
        };
        if !part.faces.is_empty() {
            break;
        }
        out.part = child.part;
        out.xform *= child.xform;
        if out.name.is_empty() {
            out.name = child.name.clone();
        }
    }
    out
}

/// A placement of a part: a component when the part is placed more than
/// once (its instances share one mesh), a group otherwise.
fn placement(inst: &RawInstance, uses: &[usize]) -> Placement {
    let mut p = Placement::new(inst.part, inst.xform);
    if !inst.name.is_empty() {
        p.name = Some(Some(inst.name.clone()));
    }
    p.component = Some(uses[inst.part] > 1);
    p
}

/// One attrs entry per colour.
#[derive(Default)]
struct Colors {
    entries: HashMap<[u64; 3], usize>,
}

impl Colors {
    fn entry(&mut self, payload: &mut SkpPayload, color: Rgb) -> usize {
        let bits = color.map(f64::to_bits);
        *self.entries.entry(bits).or_insert_with(|| {
            payload.push_attrs(AttrEntry {
                paint: Paint {
                    color: Some(color),
                    ..Default::default()
                },
                ..Default::default()
            })
        })
    }
}

/// One part's faces, welded.
struct BuiltPart {
    faces: Vec<PayloadFace>,
    soft_edges: Vec<(DVec3, DVec3)>,
    /// B-rep faces nothing of which survived the weld.
    dropped_faces: usize,
    /// Every edge lies between exactly two faces.
    closed: bool,
}

/// Weld, clean and outline the faces of `part`; `attrs` is each face's
/// attrs entry.
fn build_part(part: &RawPart, attrs: &[Option<usize>]) -> BuiltPart {
    let mut welder = Mesh::new();
    let mut faces: Vec<Vec<Option<[VertexId; 3]>>> = Vec::with_capacity(part.faces.len());
    for face in &part.faces {
        let ids: Vec<VertexId> = face.positions.iter().map(|&p| welder.vertex(p)).collect();
        let tris = face
            .triangles
            .iter()
            .map(|t| t.map(|i| ids[i as usize]))
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .map(Some)
            .collect();
        faces.push(tris);
    }

    // Slivers the weld flattens onto another triangle, of the same face or
    // a neighbour: a repeat is dropped, and a triangle folded back over
    // another (the same corners, the other way round) cancels it.
    let mut seen: HashMap<[VertexId; 3], (usize, usize)> = HashMap::new();
    for fi in 0..faces.len() {
        for ti in 0..faces[fi].len() {
            let Some(t) = faces[fi][ti] else { continue };
            let (same, folded) = (canonical(t), canonical([t[0], t[2], t[1]]));
            if seen.contains_key(&same) {
                faces[fi][ti] = None;
            } else if let Some((ff, ft)) = seen.remove(&folded) {
                faces[ff][ft] = None;
                faces[fi][ti] = None;
            } else {
                seen.insert(same, (fi, ti));
            }
        }
    }

    let pos = |v: VertexId| welder.verts[v].position;
    let mut out = BuiltPart {
        faces: Vec::new(),
        soft_edges: Vec::new(),
        dropped_faces: 0,
        closed: true,
    };
    let mut edge_uses: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    let mut count =
        |a: VertexId, b: VertexId| *edge_uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
    for (fi, face) in part.faces.iter().enumerate() {
        let tris: Vec<[VertexId; 3]> = faces[fi].iter().flatten().copied().collect();
        if tris.is_empty() {
            if !face.triangles.is_empty() {
                out.dropped_faces += 1;
            }
            continue;
        }
        let rings = if face.planar {
            polygon(&tris, pos)
        } else {
            None
        };
        if let Some(rings) = rings {
            for ring in &rings {
                for k in 0..ring.len() {
                    count(ring[k], ring[(k + 1) % ring.len()]);
                }
            }
            out.faces.push(PayloadFace {
                outer: rings[0].iter().map(|&v| pos(v)).collect(),
                holes: rings[1..]
                    .iter()
                    .map(|r| r.iter().map(|&v| pos(v)).collect())
                    .collect(),
                attrs: attrs[fi],
            });
            continue;
        }
        let mut inner: HashMap<(VertexId, VertexId), usize> = HashMap::new();
        for t in &tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                count(a, b);
                *inner.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
            out.faces.push(PayloadFace {
                outer: t.iter().map(|&v| pos(v)).collect(),
                holes: Vec::new(),
                attrs: attrs[fi],
            });
        }
        // Edges between two triangles of one curved face are soft; the
        // order follows the triangles, so the result is reproducible.
        for t in &tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a < b && inner[&(a, b)] == 2 {
                    out.soft_edges.push((pos(a), pos(b)));
                }
            }
        }
    }
    out.closed = !edge_uses.is_empty() && edge_uses.values().all(|&n| n == 2);
    out
}

/// A triangle turned to start at its smallest corner: two triangles with
/// the same corners in the same turning order compare equal.
fn canonical(t: [VertexId; 3]) -> [VertexId; 3] {
    let k = (0..3).min_by_key(|&k| t[k]).unwrap_or(0);
    [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
}

/// The outline of a planar face's triangles as rings, outer first then
/// the holes: the edges used once, chained into loops, the one turning
/// with the triangles outside and the others holes. `None` when the
/// boundary is not that simple (a pinch point, an open chain, several
/// outer loops); the caller then keeps the triangles.
fn polygon(tris: &[[VertexId; 3]], pos: impl Fn(VertexId) -> DVec3) -> Option<Vec<Vec<VertexId>>> {
    let mut uses: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut next: HashMap<VertexId, VertexId> = HashMap::new();
    let mut starts = Vec::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if uses[&(a.min(b), a.max(b))] == 1 {
                if next.insert(a, b).is_some() {
                    return None;
                }
                starts.push(a);
            }
        }
    }
    let normal: DVec3 = tris.iter().map(|t| newell(&t.map(&pos))).sum();
    let mut done: HashSet<VertexId> = HashSet::new();
    let mut outer = None;
    let mut holes = Vec::new();
    for start in starts {
        if done.contains(&start) {
            continue;
        }
        let mut ring = Vec::new();
        let mut v = start;
        loop {
            if !done.insert(v) {
                return None;
            }
            ring.push(v);
            v = *next.get(&v)?;
            if v == start {
                break;
            }
        }
        if ring.len() < 3 {
            return None;
        }
        let points: Vec<DVec3> = ring.iter().map(|&v| pos(v)).collect();
        if newell(&points).dot(normal) > 0.0 {
            if outer.replace(ring).is_some() {
                return None;
            }
        } else {
            holes.push(ring);
        }
    }
    let mut rings = vec![outer?];
    rings.extend(holes);
    Some(rings)
}
