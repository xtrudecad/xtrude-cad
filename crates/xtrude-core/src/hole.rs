// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Screw holes: what a hole is (bore, depth, head), the ISO 273 clearance
//! table, the side count that keeps a printed hole round, and the cutter —
//! a solid of revolution about the hole's axis that the Hole tool
//! subtracts from a solid.
//!
//! Lengths are metres. The cutter's corners lie on the nominal circles, so
//! a flat between two corners sits inside the circle by the sagitta;
//! [`hole_sides`] keeps that under [`ROUNDNESS`].

use std::f64::consts::PI;

use glam::DVec3;

use crate::axes::plane_axes;
use crate::math::VecExt;
use crate::mesh::Mesh;

/// How far a flat of the hole may fall inside the nominal circle (0.02 mm,
/// well under what a printer resolves).
pub const ROUNDNESS: f64 = 2e-5;
/// The fewest sides a hole has.
pub const MIN_SIDES: usize = 24;
/// The most sides a hole has, however large.
pub const MAX_SIDES: usize = 128;

/// The countersink angle of flat-head screws (ISO 10642), degrees.
pub const COUNTERSINK_DEG: f64 = 90.0;

/// The fit of a clearance hole (ISO 273).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// "Fine": the screw locates the part.
    Close,
    /// "Medium": the everyday clearance.
    Normal,
}

/// ISO 273 clearance holes for M2–M8: `(nominal, close, normal)` in mm.
pub const CLEARANCES: [(f64, f64, f64); 7] = [
    (2.0, 2.2, 2.4),
    (2.5, 2.7, 2.9),
    (3.0, 3.2, 3.4),
    (4.0, 4.3, 4.5),
    (5.0, 5.3, 5.5),
    (6.0, 6.4, 6.6),
    (8.0, 8.4, 9.0),
];

/// The clearance hole diameter (metres) for an M`nominal` screw (mm), or
/// `None` for a size the table does not hold.
pub fn clearance(nominal: f64, fit: Fit) -> Option<f64> {
    CLEARANCES
        .iter()
        .find(|(m, _, _)| (m - nominal).abs() < 1e-9)
        .map(|&(_, close, normal)| match fit {
            Fit::Close => close / 1000.0,
            Fit::Normal => normal / 1000.0,
        })
}

/// How deep the hole goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    /// Through the wall it is drilled into.
    Through,
    /// A blind hole this deep, with a flat bottom.
    Blind(f64),
}

/// The head of the hole, at the face it is drilled from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Head {
    None,
    /// A cone from `diameter` at the face down to the bore.
    Countersink {
        diameter: f64,
        angle_deg: f64,
    },
    /// A wider cylinder `depth` deep.
    Counterbore {
        diameter: f64,
        depth: f64,
    },
}

/// A hole: its bore, its depth and its head.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleSpec {
    pub diameter: f64,
    pub depth: Depth,
    pub head: Head,
}

impl Default for HoleSpec {
    /// An M3 normal-fit hole, through, without a head.
    fn default() -> HoleSpec {
        HoleSpec {
            diameter: 0.0034,
            depth: Depth::Through,
            head: Head::None,
        }
    }
}

impl HoleSpec {
    /// How deep the head goes below the face (0 without one).
    pub fn head_depth(&self) -> f64 {
        match self.head {
            Head::None => 0.0,
            Head::Countersink {
                diameter,
                angle_deg,
            } => 0.5 * (diameter - self.diameter) / (0.5 * angle_deg.to_radians()).tan(),
            Head::Counterbore { depth, .. } => depth,
        }
    }

    /// The widest diameter of the hole: the head's, or the bore's.
    pub fn widest(&self) -> f64 {
        match self.head {
            Head::None => self.diameter,
            Head::Countersink { diameter, .. } | Head::Counterbore { diameter, .. } => {
                diameter.max(self.diameter)
            }
        }
    }

    /// The cutter's profile, `(depth below the face, radius)` from top to
    /// bottom: it starts `above` the face (so the cut does not graze it)
    /// and ends `length` below it.
    pub fn profile(&self, above: f64, length: f64) -> Vec<(f64, f64)> {
        let r = 0.5 * self.diameter;
        let mut p = match self.head {
            Head::None => vec![(-above, r)],
            Head::Countersink { diameter, .. } => {
                let big = 0.5 * diameter;
                vec![(-above, big), (0.0, big), (self.head_depth(), r)]
            }
            Head::Counterbore { diameter, depth } => {
                let big = 0.5 * diameter;
                vec![(-above, big), (depth, big), (depth, r)]
            }
        };
        p.push((length, r));
        p
    }
}

/// The side count of a hole of `diameter`: enough that no flat falls more
/// than [`ROUNDNESS`] inside the circle, a multiple of 8, between
/// [`MIN_SIDES`] and [`MAX_SIDES`].
pub fn hole_sides(diameter: f64) -> usize {
    let r = 0.5 * diameter;
    if r <= ROUNDNESS {
        return MIN_SIDES;
    }
    let n = PI / (1.0 - ROUNDNESS / r).acos();
    let n = (n.ceil() as usize).div_ceil(8) * 8;
    n.clamp(MIN_SIDES, MAX_SIDES)
}

/// The rings of a solid of revolution: one per profile point
/// `(depth along axis, radius)`, `sides` corners each, about `axis` through
/// `centre`, corner 0 on the same side for every ring.
pub fn revolve_rings(
    centre: DVec3,
    axis: DVec3,
    profile: &[(f64, f64)],
    sides: usize,
) -> Vec<Vec<DVec3>> {
    let w = axis.unit_or_zero();
    let (u, v) = plane_axes(w);
    // u × v = w, so the corners turn counter-clockwise about the axis
    let v = if u.cross(v).dot(w) < 0.0 { -v } else { v };
    profile
        .iter()
        .map(|&(z, r)| {
            (0..sides)
                .map(|k| {
                    let t = 2.0 * PI * k as f64 / sides as f64;
                    centre + w * z + (u * t.cos() + v * t.sin()) * r
                })
                .collect()
        })
        .collect()
}

/// The closed solid of revolution of `profile` (see [`revolve_rings`]),
/// its faces pointing out, its seams soft.
pub fn revolve(centre: DVec3, axis: DVec3, profile: &[(f64, f64)], sides: usize) -> Mesh {
    let rings = revolve_rings(centre, axis, profile, sides);
    let mut mesh = Mesh::new();
    let (Some(top), Some(bottom)) = (rings.first(), rings.last()) else {
        return mesh;
    };
    let mut cap: Vec<DVec3> = top.clone();
    cap.reverse();
    mesh.add_face(&cap, &[]);
    for pair in rings.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for j in 0..sides {
            let k = (j + 1) % sides;
            mesh.add_face(&[a[j], a[k], b[k], b[j]], &[]);
        }
    }
    mesh.add_face(bottom, &[]);
    for pair in rings.windows(2) {
        for (&pa, &pb) in pair[0].iter().zip(pair[1].iter()) {
            let (Some(va), Some(vb)) = (mesh.vertex_at(pa), mesh.vertex_at(pb)) else {
                continue;
            };
            if let Some(e) = mesh.find_edge(va, vb) {
                mesh.edge_mut(e).soft = true;
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orient::{is_closed, signed_volume};

    #[test]
    fn the_side_count_keeps_the_flats_within_the_roundness() {
        for d in [0.001, 0.0034, 0.009, 0.02, 0.1] {
            let n = hole_sides(d);
            assert!(
                (MIN_SIDES..=MAX_SIDES).contains(&n) && n % 8 == 0,
                "{d}: {n}"
            );
            let sag = 0.5 * d * (1.0 - (PI / n as f64).cos());
            assert!(n == MAX_SIDES || sag <= ROUNDNESS, "{d}: {n} sides, {sag}");
        }
        assert_eq!(hole_sides(0.0034), 24);
        assert_eq!(hole_sides(1.0), MAX_SIDES);
    }

    #[test]
    fn the_clearance_table_has_both_fits() {
        assert_eq!(clearance(3.0, Fit::Normal), Some(0.0034));
        assert_eq!(clearance(3.0, Fit::Close), Some(0.0032));
        assert_eq!(clearance(8.0, Fit::Normal), Some(0.009));
        assert_eq!(clearance(7.0, Fit::Normal), None);
    }

    #[test]
    fn a_countersink_is_as_deep_as_its_cone() {
        let s = HoleSpec {
            diameter: 0.0034,
            depth: Depth::Through,
            head: Head::Countersink {
                diameter: 0.0064,
                angle_deg: 90.0,
            },
        };
        assert!((s.head_depth() - 0.0015).abs() < 1e-12);
        assert_eq!(s.profile(0.001, 0.01).len(), 4);
    }

    #[test]
    fn the_revolved_cutter_is_a_closed_solid_pointing_out() {
        let s = HoleSpec {
            diameter: 0.004,
            depth: Depth::Through,
            head: Head::Counterbore {
                diameter: 0.008,
                depth: 0.002,
            },
        };
        let m = revolve(DVec3::ZERO, -DVec3::Z, &s.profile(0.001, 0.01), 32);
        assert!(is_closed(&m));
        let v = signed_volume(&m);
        let exact = PI * (0.004f64.powi(2) * 0.003 + 0.002f64.powi(2) * 0.008);
        assert!(
            v > 0.0 && (v - exact).abs() < 0.02 * exact,
            "{v} vs {exact}"
        );
        let soft = m.edges.iter().filter(|(_, e)| e.soft).count();
        assert_eq!(soft, 3 * 32);
    }
}
