// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The bridge to the C++ reader (`reader.cpp`): OpenCascade reads the file
//! into an XCAF document (names, colours, assembly structure, units), then
//! meshes every part. Lengths come out in millimetres, the document unit
//! the reader converts the file to.

#[cxx::bridge(namespace = "xtrude_step")]
pub mod ffi {
    /// One B-rep face, meshed: its nodes (xyz in millimetres, in its
    /// part's frame) and triangles (three node indices each, outward).
    struct Face {
        positions: Vec<f64>,
        triangles: Vec<u32>,
        planar: bool,
        has_color: bool,
        color: [f64; 3],
    }

    /// One placement of a part inside an assembly, or at the top level.
    struct Instance {
        part: u32,
        name: String,
        /// The 3×4 placement matrix, row by row, translation in mm.
        matrix: [f64; 12],
    }

    /// One shape of the file: a part with faces, or an assembly with
    /// children. A part placed several times is listed once.
    struct Part {
        name: String,
        has_color: bool,
        color: [f64; 3],
        faces: Vec<Face>,
        children: Vec<Instance>,
        closed: bool,
        failed_faces: u32,
    }

    struct Doc {
        parts: Vec<Part>,
        roots: Vec<Instance>,
    }

    unsafe extern "C++" {
        include!("xtrude-step/src/reader.hpp");

        type Reader;

        /// Read and transfer a STEP file; an error when it is not one or
        /// holds no shape.
        fn open_step(path: &str) -> Result<UniquePtr<Reader>>;
        /// Mesh every part: `chord_mm` is the chord tolerance, `angle_rad`
        /// the angular one.
        fn tessellate(reader: Pin<&mut Reader>, chord_mm: f64, angle_rad: f64) -> Result<Doc>;
    }
}

use glam::{DMat4, DVec3};
use xtrude_io::step::protocol::{RawFace, RawInstance, RawPart, RawStep, PROGRESS_PREFIX};
use xtrude_io::step::Tessellation;

/// Metres per millimetre, the unit the reader hands lengths in.
const MM: f64 = 1e-3;

fn progress(fraction: f64, text: &str) {
    eprintln!("{PROGRESS_PREFIX}{fraction} {text}");
}

/// Read and mesh `file`; lengths in metres.
pub fn read(file: &str, tessellation: &Tessellation) -> Result<RawStep, String> {
    progress(0.05, "Reading the file…");
    let mut reader = ffi::open_step(file).map_err(|e| e.what().to_string())?;
    progress(0.5, "Tessellating…");
    let doc = ffi::tessellate(
        reader.pin_mut(),
        tessellation.chord / MM,
        tessellation.angle_deg.to_radians(),
    )
    .map_err(|e| e.what().to_string())?;
    progress(0.95, "Sending the result…");
    Ok(RawStep {
        parts: doc.parts.iter().map(part).collect(),
        roots: doc.roots.iter().map(instance).collect(),
    })
}

fn color(has: bool, c: [f64; 3]) -> Option<[f64; 3]> {
    has.then_some(c)
}

fn part(p: &ffi::Part) -> RawPart {
    RawPart {
        name: p.name.clone(),
        color: color(p.has_color, p.color),
        faces: p
            .faces
            .iter()
            .map(|f| RawFace {
                positions: f
                    .positions
                    .chunks_exact(3)
                    .map(|c| DVec3::new(c[0], c[1], c[2]) * MM)
                    .collect(),
                triangles: f
                    .triangles
                    .chunks_exact(3)
                    .map(|t| [t[0], t[1], t[2]])
                    .collect(),
                planar: f.planar,
                color: color(f.has_color, f.color),
            })
            .collect(),
        children: p.children.iter().map(instance).collect(),
        closed: p.closed,
        failed_faces: p.failed_faces,
    }
}

fn instance(i: &ffi::Instance) -> RawInstance {
    let m = &i.matrix;
    let mut rows = [0.0; 16];
    rows[..12].copy_from_slice(m);
    rows[15] = 1.0;
    let mut xform = DMat4::from_cols_array(&rows).transpose();
    xform.w_axis.x *= MM;
    xform.w_axis.y *= MM;
    xform.w_axis.z *= MM;
    RawInstance {
        part: i.part as usize,
        name: i.name.clone(),
        xform,
    }
}
