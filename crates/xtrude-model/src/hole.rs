// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Drill a hole into a solid of a mesh — the Hole tool (the hole and its
//! cutter are `xtrude_core::hole`).
//!
//! The solid is the connected piece of the mesh that holds the clicked
//! face. The hole goes in square to that face, into the material; how
//! thick the material is there is read along the axis, so a blind hole or
//! a head that would come out the far side is refused, and a through hole
//! stops at the far side of that wall. The cut is a boolean subtract, so
//! the solid stays closed; the piece is replaced by the result, its
//! circles tagged as curves.

use xtrude_core::hole::{hole_sides, revolve, revolve_rings, Depth, Head, HoleSpec};
use xtrude_core::math::{DVec3, VecExt};
use xtrude_core::orient::ray_triangle;
use xtrude_core::solids::{run as run_solid, solid_report, SolidInput, SolidOp};
use xtrude_core::{EdgeId, FaceId, Mesh};

use crate::i18n::tr;
use crate::pieces::{connected_parts, copy_part, copy_part_into};
use crate::units::fmt_len_fine;

/// How far the cutter reaches past the faces it cuts through (0.5 mm), so
/// that no face of the cut lies on a face of the solid; less where other
/// material is nearer.
pub const OVERSHOOT: f64 = 5e-4;

/// The thinnest wall a blind hole leaves at its bottom (0.1 mm, the length
/// tolerance).
pub const MIN_FLOOR: f64 = 1e-4;

/// Why a hole cannot be drilled.
#[derive(Clone, Debug, PartialEq)]
pub enum HoleError {
    /// The face is not part of a closed solid.
    NotSolid,
    /// No material behind the face, along its normal.
    NoMaterial,
    /// A size that makes no hole (a nil diameter, a head no wider than the
    /// bore…); the text says which.
    BadSize(String),
    /// The hole, or its head, would come out the far side of a material
    /// `thickness` thick.
    TooDeep { thickness: f64 },
    /// The boolean failed.
    Cut(String),
}

impl HoleError {
    /// What the user is told.
    pub fn message(&self) -> String {
        match self {
            HoleError::NotSolid => tr(
                "A hole is drilled into a solid: that face is not part of a closed solid.",
                &[],
            ),
            HoleError::NoMaterial => tr("There is no material behind that face.", &[]),
            HoleError::BadSize(s) => s.clone(),
            HoleError::TooDeep { thickness } => tr(
                "The hole would come out the far side ({t} thick there): make it through, or shallower.",
                &[("t", &fmt_len_fine(*thickness))],
            ),
            HoleError::Cut(s) => tr("The hole could not be cut: {why}", &[("why", s)]),
        }
    }
}

/// A finite length above zero.
fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// Where a hole goes and how far: what the tool previews and the cut
/// needs.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleSite {
    /// The faces and edges of the solid drilled.
    pub faces: Vec<FaceId>,
    pub edges: Vec<EdgeId>,
    /// The hole's centre, on the clicked face.
    pub centre: DVec3,
    /// The unit direction into the material.
    pub axis: DVec3,
    /// The material's thickness along the axis.
    pub thickness: f64,
    /// Free room behind the far side before the next material, if any.
    pub room_below: Option<f64>,
    /// Free room in front of the face before any material, if any.
    pub room_above: Option<f64>,
}

/// The solid a hole is drilled into: the connected piece of the mesh
/// that holds the clicked face, checked closed.
#[derive(Clone, Debug)]
pub struct HoleSolid {
    pub faces: Vec<FaceId>,
    pub edges: Vec<EdgeId>,
    /// Its boundary as triangles, by face, to read thicknesses along an
    /// axis.
    tris: Vec<(FaceId, [DVec3; 3])>,
}

impl HoleSolid {
    /// The solid of `face`.
    pub fn of(mesh: &Mesh, face: FaceId) -> Result<HoleSolid, HoleError> {
        if !mesh.has_face(face) {
            return Err(HoleError::NotSolid);
        }
        let (faces, edges) = connected_parts(mesh)
            .into_iter()
            .find(|(faces, _)| faces.contains(&face))
            .ok_or(HoleError::NotSolid)?;
        let outer: Vec<FaceId> = faces
            .iter()
            .copied()
            .filter(|&f| !mesh.face(f).interior)
            .collect();
        if !solid_report(&copy_part(mesh, &outer, &[])).0 {
            return Err(HoleError::NotSolid);
        }
        let tris = outer
            .iter()
            .flat_map(|&f| {
                mesh.face_triangulate(f, None)
                    .into_iter()
                    .map(move |t| (f, t))
            })
            .collect();
        Ok(HoleSolid { faces, edges, tris })
    }

    /// The axis and the thickness at `at` (projected onto the plane of
    /// `face`, a face of this solid).
    pub fn site(&self, mesh: &Mesh, face: FaceId, at: DVec3) -> Result<HoleSite, HoleError> {
        if !mesh.has_face(face) || !self.faces.contains(&face) {
            return Err(HoleError::NotSolid);
        }
        let n = mesh.face_normal(face).unit_or_zero();
        if n == DVec3::ZERO {
            return Err(HoleError::NoMaterial);
        }
        let on = mesh.face_positions(face)[0];
        let centre = at - n * (at - on).dot(n);
        let hits = |dir: DVec3| -> Vec<f64> {
            let mut ts: Vec<f64> = self
                .tris
                .iter()
                .filter(|(f, _)| *f != face)
                .filter_map(|(_, t)| ray_triangle(centre, dir, t))
                .collect();
            ts.sort_by(f64::total_cmp);
            // a ray through an edge or a corner meets each face there once
            ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            ts
        };
        // Into the material: the side where the ray leaves the solid an odd
        // number of times.
        let (axis, inward, outward) = {
            let (back, front) = (hits(-n), hits(n));
            if back.len() % 2 == 1 {
                (-n, back, front)
            } else if front.len() % 2 == 1 {
                (n, front, back)
            } else {
                return Err(HoleError::NoMaterial);
            }
        };
        Ok(HoleSite {
            faces: self.faces.clone(),
            edges: self.edges.clone(),
            centre,
            axis,
            thickness: inward[0],
            room_below: inward.get(1).map(|t| t - inward[0]),
            room_above: outward.first().copied(),
        })
    }
}

impl HoleSite {
    /// Find the solid of `face`, and the axis and thickness at `at`
    /// (projected onto the face's plane).
    pub fn find(mesh: &Mesh, face: FaceId, at: DVec3) -> Result<HoleSite, HoleError> {
        HoleSolid::of(mesh, face)?.site(mesh, face, at)
    }

    /// Where the cutter ends below the face, for `spec`.
    pub fn length(&self, spec: &HoleSpec) -> f64 {
        match spec.depth {
            Depth::Blind(d) => d,
            Depth::Through => {
                self.thickness
                    + self
                        .room_below
                        .map_or(OVERSHOOT, |r| OVERSHOOT.min(0.5 * r))
            }
        }
    }

    /// How far the cutter starts in front of the face.
    pub fn above(&self) -> f64 {
        self.room_above
            .map_or(OVERSHOOT, |r| OVERSHOOT.min(0.5 * r))
    }

    /// Whether `spec` fits this site; the error says why not.
    pub fn check(&self, spec: &HoleSpec) -> Result<(), HoleError> {
        let bad = |s: &str| Err(HoleError::BadSize(tr(s, &[])));
        if !positive(spec.diameter) {
            return bad("The hole needs a diameter.");
        }
        let head_depth = spec.head_depth();
        match spec.head {
            Head::None => {}
            Head::Countersink {
                diameter,
                angle_deg,
            } => {
                if !positive(diameter - spec.diameter) {
                    return bad("A countersink is wider than the hole.");
                }
                if !positive(angle_deg) || angle_deg >= 180.0 {
                    return bad("A countersink's angle is between 0° and 180°.");
                }
            }
            Head::Counterbore { diameter, depth } => {
                if !positive(diameter - spec.diameter) {
                    return bad("A counterbore is wider than the hole.");
                }
                if !positive(depth) {
                    return bad("A counterbore needs a depth.");
                }
            }
        }
        let room = self.thickness - MIN_FLOOR;
        match spec.depth {
            Depth::Blind(d) => {
                if !positive(d) {
                    return bad("A blind hole needs a depth.");
                }
                if d > room {
                    return Err(HoleError::TooDeep {
                        thickness: self.thickness,
                    });
                }
                if head_depth >= d {
                    return bad("The head is deeper than the hole.");
                }
            }
            Depth::Through => {
                if head_depth > room {
                    return Err(HoleError::TooDeep {
                        thickness: self.thickness,
                    });
                }
            }
        }
        Ok(())
    }

    /// The cutter's rings (see [`revolve_rings`]), for a preview.
    pub fn rings(&self, spec: &HoleSpec) -> Vec<Vec<DVec3>> {
        let profile = spec.profile(0.0, self.length(spec));
        revolve_rings(self.centre, self.axis, &profile, hole_sides(spec.widest()))
    }
}

/// A hole ready to cut: the solid's replacement.
#[derive(Clone, Debug)]
pub struct HolePlan {
    pub site: HoleSite,
    pub spec: HoleSpec,
    /// The drilled solid.
    pub result: Mesh,
}

/// Drill `spec` into the solid of `face` at `at`.
pub fn plan_hole(
    mesh: &Mesh,
    face: FaceId,
    at: DVec3,
    spec: &HoleSpec,
) -> Result<HolePlan, HoleError> {
    let site = HoleSite::find(mesh, face, at)?;
    site.check(spec)?;
    let profile = spec.profile(site.above(), site.length(spec));
    let cutter = revolve(site.centre, site.axis, &profile, hole_sides(spec.widest()));
    let solid = copy_part(mesh, &site.faces, &site.edges);
    let res = run_solid(
        SolidOp::Subtract,
        &[
            SolidInput {
                mesh: &cutter,
                paint: None,
            },
            SolidInput {
                mesh: &solid,
                paint: None,
            },
        ],
    )
    .map_err(|e| HoleError::Cut(e.0))?;
    let result = res
        .meshes
        .into_iter()
        .next()
        .map(|(m, _)| m)
        .ok_or_else(|| HoleError::Cut("empty".into()))?;
    Ok(HolePlan {
        site,
        spec: *spec,
        result,
    })
}

/// Replace the solid by the drilled one, and tag the hole's circles (where
/// they are edges) as curves.
pub fn apply_hole(mesh: &mut Mesh, plan: &HolePlan) {
    for &f in &plan.site.faces {
        mesh.remove_face(f);
    }
    for &e in &plan.site.edges {
        mesh.remove_edge(e);
    }
    mesh.prune_orphan_vertices();
    let r = &plan.result;
    copy_part_into(mesh, r, &r.face_ids(), &r.edge_ids());
    let site = &plan.site;
    let spec = &plan.spec;
    let mut profile = spec.profile(0.0, site.length(spec));
    if spec.depth == Depth::Through {
        // where the bore leaves the far side
        profile.push((site.thickness, 0.5 * spec.diameter));
    }
    let sides = hole_sides(spec.widest());
    for ring in revolve_rings(site.centre, site.axis, &profile, sides) {
        if !ring.is_empty() {
            mesh.tag_curve(&ring, true);
        }
    }
}
