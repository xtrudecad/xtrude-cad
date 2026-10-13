// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! New-document units through the document.

use crate::common;
use common::*;
use xtrude_model::scene::Scene;
use xtrude_model::units::{self, Units};

#[test]
fn test_an_opened_document_keeps_its_own_units() {
    xtrude_model::settings::use_memory_store(); // never the user's settings
    let mut drawn = Scene::new();
    drawn.units = Units::new("m", 2);
    units::remember_new_document_units(&Units::new("mm", 2));
    let loaded = roundtrip(&drawn);
    units::remember_new_document_units(&units::default_model_units());
    assert_eq!(loaded.units.length, "m");
}
