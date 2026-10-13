// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The title shows an import's name until a document path takes over.

use crate::common;

use common::window;
use std::path::PathBuf;

#[test]
fn test_title_shows_imported_name_until_the_document_takes_over() {
    let mut w = window();
    assert!(w.title.contains("Untitled"));
    w.import_name = Some("good part.skp".into()); // what an import sets
    w.update_title();
    assert!(w.title.contains("good part.skp"));
    w.current_path = Some(PathBuf::from("/tmp/project.xtrude")); // Save As wins
    w.update_title();
    assert!(w.title.contains("project.xtrude") && !w.title.contains("good part"));
    w.current_path = None; // New clears the import name
    w.import_name = None;
    w.update_title();
    assert!(!w.title.contains("project.xtrude"));
}
