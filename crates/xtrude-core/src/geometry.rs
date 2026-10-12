// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Geometry primitives over positions: a segment and a planar polygon with
//! holes.
//!
//! The topology helpers work on positions, so mesh faces are viewed through
//! a [`PolyFace`] (`Mesh::poly_face`) and tool previews build them directly.

use crate::attrs::FaceAttrs;
use crate::math::{newell, DVec3};
use crate::mesh::{FaceId, Mesh};

/// A line segment between two 3D points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: DVec3,
    pub b: DVec3,
}

impl Segment {
    pub const fn new(a: DVec3, b: DVec3) -> Self {
        Segment { a, b }
    }

    pub fn length(&self) -> f64 {
        (self.b - self.a).length()
    }
}

/// A planar polygon defined by its outer vertex loop, with optional holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyFace {
    pub vertices: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    /// Optional display attrs — tool PREVIEWS carry them so a paste drags
    /// the real look.
    pub attrs: Option<FaceAttrs>,
}

impl PolyFace {
    pub fn new(vertices: Vec<DVec3>) -> Self {
        PolyFace {
            vertices,
            holes: Vec::new(),
            attrs: None,
        }
    }

    pub fn with_holes(vertices: Vec<DVec3>, holes: Vec<Vec<DVec3>>) -> Self {
        PolyFace {
            vertices,
            holes,
            attrs: None,
        }
    }

    /// Raw (un-normalised) Newell vector of the outer loop.
    pub fn newell(&self) -> DVec3 {
        newell(&self.vertices)
    }

    /// Unit normal (Newell), `(0, 0, 1)` when degenerate.
    pub fn normal(&self) -> DVec3 {
        if self.vertices.len() < 3 {
            return DVec3::Z;
        }
        let n = self.newell();
        let len = n.length();
        if len < 1e-9 {
            return DVec3::Z;
        }
        n / len
    }

    /// Area of the outer loop (holes not subtracted).
    pub fn area(&self) -> f64 {
        if self.vertices.len() < 3 {
            return 0.0;
        }
        0.5 * self.newell().length()
    }

    pub fn centroid(&self) -> DVec3 {
        if self.vertices.is_empty() {
            return DVec3::ZERO;
        }
        self.vertices.iter().copied().sum::<DVec3>() / self.vertices.len() as f64
    }

    pub fn triangulate(&self) -> Vec<[DVec3; 3]> {
        if self.vertices.len() < 3 {
            return Vec::new();
        }
        crate::triangulate::triangulate(&self.vertices, &self.holes, Some(self.normal()))
    }
}

impl Mesh {
    /// A position view of a mesh face (`face.vertices` / `face.holes`).
    pub fn poly_face(&self, f: FaceId) -> PolyFace {
        PolyFace {
            vertices: self.face_positions(f),
            holes: self.face_hole_positions(f),
            attrs: None,
        }
    }
}
