// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File formats: the Xtrude CAD document (`.xtr`), IngeTrazo's `.igz`
//! (read only), the mesh interchange formats, 3MF and the SketchUp `.skp`
//! and STEP imports.

pub mod clip;
pub mod dae;
pub mod document;
pub mod dxf_in;
pub mod glb;
pub mod gltf;
pub mod igz;
pub mod meshexport;
pub mod obj;
mod schema;
pub mod skp;
pub mod step;
pub mod stl;
pub mod threemf;
