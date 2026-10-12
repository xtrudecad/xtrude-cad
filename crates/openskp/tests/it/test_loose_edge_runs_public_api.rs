// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP
//! `packages/python/tests/test_loose_edge_runs_public_api.py`: the two
//! tests on the parsed model. `loose_edge_runs` itself is left out
//! (UPSTREAM.md).

use crate::common;

use common::fixture;

/// The upstream test also checks `Edge::layer` on a fixture with edges on
/// a second layer; no committed fixture has one (UPSTREAM.md, Fixtures).
#[test]
fn test_edge_curve_id_is_populated_when_the_file_has_it() {
    let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
        .unwrap()
        .parse()
        .unwrap();
    let edges = || model.definitions.values().flat_map(|d| d.edges.values());
    assert!(edges().any(|e| e.curve_id.is_some()));
}

#[test]
fn test_face_layer_is_populated_when_the_file_has_it() {
    let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(model
        .definitions
        .values()
        .flat_map(|d| d.faces.values())
        .any(|f| f.layer.is_some()));
}
