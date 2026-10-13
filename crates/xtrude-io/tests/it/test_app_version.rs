// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Xtrude CAD numbers its own versions: the workspace `version` is the one
//! source, and a saved document carries it as `app_version` (the IngeTrazo
//! documents in `tests/data/*.igz` keep the IngeTrazo release that wrote
//! them).

use xtrude_io::document;
use xtrude_model::scene::Scene;

#[test]
fn test_version_is_the_workspace_version() {
    assert_eq!(xtrude_model::VERSION, env!("CARGO_PKG_VERSION"));
    assert_ne!(xtrude_model::VERSION, "0.5.7");
}

#[test]
fn test_saved_document_carries_the_app_version() {
    let json = document::scene_json(&Scene::new());
    assert_eq!(json["app_version"], xtrude_model::VERSION);
}
