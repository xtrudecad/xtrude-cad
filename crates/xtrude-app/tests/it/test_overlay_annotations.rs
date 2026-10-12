// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The overlay's annotation layer (dimensions, text labels, section planes),
//! checked through the paint commands.

use crate::common;

use common::{sized_viewport, v3};
use xtrude_app::viewport::overlay::Paint;
use xtrude_model::dimension::Dimension;
use xtrude_model::entity::EntityRef;
use xtrude_model::section::SectionPlane;
use xtrude_model::textlabel::TextLabel;

fn texts(p: &[Paint]) -> Vec<String> {
    p.iter()
        .filter_map(|p| match p {
            Paint::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn test_dimension_draws_lines_arrows_and_value() {
    let mut vp = sized_viewport(800., 600.);
    vp.camera.set_view("top");
    vp.camera.fit_to(v3(0., 0., 0.), v3(2., 1., 0.), 1.5);
    vp.scene.doc.dimensions.push(Dimension::new(
        v3(0., 0., 0.),
        v3(2., 0., 0.),
        v3(0., -0.3, 0.),
    ));
    let out = vp.overlay();
    let t = texts(&out);
    assert_eq!(
        t.iter().filter(|s| s.starts_with("2000")).count(),
        2,
        "{t:?}"
    ); // halo + ink
    let arrows = out
        .iter()
        .filter(|p| matches!(p, Paint::Polygon { .. }))
        .count();
    assert_eq!(arrows, 2);
    assert!(
        out.iter()
            .filter(|p| matches!(p, Paint::Line { .. }))
            .count()
            >= 48
    );
}

#[test]
fn test_selected_text_label_turns_orange() {
    let mut vp = sized_viewport(800., 600.);
    vp.camera.set_view("top");
    vp.camera.fit_to(v3(-1., -1., 0.), v3(1., 1., 0.), 1.5);
    let lab = TextLabel::new(v3(0., 0., 0.), v3(0.5, 0.5, 0.), "A\nB");
    let id = lab.id;
    vp.scene.doc.text_labels.push(lab);
    vp.scene.selection.insert(EntityRef::TextLabel(id));
    let out = vp.overlay();
    let inks: Vec<_> = out
        .iter()
        .filter_map(|p| match p {
            Paint::Text { text, color, .. } if text == "B" => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(inks.len(), 2);
    assert!((inks[1][0] - 243.0 / 255.0).abs() < 1e-6);
}

#[test]
fn test_section_plane_frame_and_symbols() {
    let mut vp = sized_viewport(800., 600.);
    vp.camera.set_view("iso");
    vp.camera.fit_to(v3(-1., -1., -1.), v3(1., 1., 1.), 1.5);
    vp.scene.doc.section_planes.push(SectionPlane::new(
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        "S1",
        "A",
        true,
        None,
    ));
    let out = vp.overlay();
    assert_eq!(texts(&out).iter().filter(|s| *s == "A").count(), 4);
    vp.scene.show_section_planes = false;
    assert!(texts(&vp.overlay()).iter().all(|s| s != "A"));
}
