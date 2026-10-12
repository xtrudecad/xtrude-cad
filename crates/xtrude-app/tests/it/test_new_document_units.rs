// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window tests: a new document starts in the units remembered
//! for new documents, millimetres when none are. The opened-document test is
//! in xtrude-io.

use crate::common;

use common::window;
use xtrude_model::settings;
use xtrude_model::units::{self, default_model_units, fmt_len, Units};

#[test]
fn test_new_documents_start_in_the_remembered_units() {
    let mm1 = Units::new("mm", 1);
    let _w0 = window(); // the memory settings store, before remembering
    units::remember_new_document_units(&mm1);
    let mut w2 = xtrude_app::window::MainWindow::new(); // a restart
    let sc = &w2.viewport.scene;
    assert_eq!(sc.units, mm1);
    assert_eq!(Some(sc.dimension_style.units.as_str()), Some("mm"));
    assert!(!w2.is_dirty()); // not «modified»
    w2.viewport.scene.units = Units::new("m", 2);
    w2.mark_saved();
    w2.trigger("new"); // File ▸ New
    assert_eq!(w2.viewport.scene.units, mm1);
    units::remember_new_document_units(&default_model_units());
}

#[test]
fn test_with_nothing_remembered_a_new_document_is_in_millimetres() {
    let _w0 = window();
    settings::clear(&settings::keys::NEW_DOCUMENT_UNITS);
    let w = xtrude_app::window::MainWindow::new();
    let sc = &w.viewport.scene;
    assert_eq!(sc.units, Units::new("mm", 2));
    assert_eq!(Some(sc.dimension_style.units.as_str()), Some("mm"));
    assert_eq!(fmt_len(0.0125), "12.50 mm");
}
