// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Which tools ask for the protractor-arm inference (the snap tests live in
//! crates/xtrude-core).

use xtrude_app::tools::protractor::ProtractorTool;
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::Tool;

#[test]
fn test_rotate_and_protractor_ask_for_arm_inference() {
    assert!(ProtractorTool::new().snap_prefs().radial_arm);
    assert!(RotateTool::new().snap_prefs().radial_arm);
}
