// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Saved views through the document.

use crate::common;
use common::*;
use xtrude_model::saved_views::{CameraPose, SavedView};
use xtrude_model::scene::Scene;

#[test]
fn test_document_round_trips_saved_views() {
    let mut s = Scene::new();
    let mut top = SavedView::new(
        "Top",
        CameraPose {
            target: [1.0, 2.0, 3.0],
            distance: 51.7,
            yaw: -1.0,
            pitch: 1.55,
            fov_deg: 35.0,
            perspective: false,
        },
    );
    top.hidden_layers = vec!["Curves".into()];
    s.saved_views.push(top);
    s.saved_views
        .push(SavedView::new("View", CameraPose::default()));
    let out = roundtrip(&s);
    assert_eq!(
        out.saved_views
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>(),
        ["Top", "View"]
    );
    let v = &out.saved_views[0];
    assert_eq!(v.pose.target, [1.0, 2.0, 3.0]);
    assert_eq!(v.pose.distance, 51.7);
    assert!(!v.pose.perspective);
    assert_eq!(v.hidden_layers, ["Curves"]);
    assert!(out.saved_views[1].pose.perspective);
}
