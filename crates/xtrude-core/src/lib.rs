// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Xtrude CAD geometry kernel: f64 math, triangulation, the shared-vertex mesh,
//! topology, planar arrangement, orientation and the modeling operations.
//! No GUI or GPU dependencies.

pub mod arena;
pub mod arrangement;
pub mod attrs;
pub mod axes;
pub mod camera;
pub mod cap_rebuild;
pub mod fillet;
pub mod fuse;
pub mod geometry;
pub mod hole;
pub mod intersect;
pub mod math;
pub mod measure;
pub mod mesh;
pub mod mt19937;
pub mod offset;
pub mod orient;
pub mod orient_faces;
pub mod shell;
pub mod snap;
pub mod solids;
pub mod stable_json;
pub mod stitch;
pub mod sweep;
pub mod text3d;
pub mod topology;
pub mod triangulate;

pub use axes::Axis;
pub use math::{v3, DMat4, DVec3, DVec4};
pub use mesh::{EdgeId, FaceId, Mesh, VertexId};
