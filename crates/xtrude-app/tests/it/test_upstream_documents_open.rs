// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A document written by upstream IngeTrazo, data of the removed features and
//! all, opens in the window. (The format tests are in xtrude-io.)

use crate::common;

use std::path::Path;

use common::window;

#[test]
fn test_the_main_window_opens_it() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join("upstream_removed_features.igz");
    let mut win = window();
    assert!(win.open_path(&fixture));
    assert_eq!(win.viewport.scene.mesh().faces.len(), 6);
}
