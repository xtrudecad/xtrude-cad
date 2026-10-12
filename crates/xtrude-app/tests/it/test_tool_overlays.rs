// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The tools' own overlay paint: Scale's box beneath the section planes,
//! Flip's axis planes and Move's rotation grips over them.

use crate::common;

use common::{add_box, sized_viewport, v3};
use xtrude_app::tools::flip::FlipTool;
use xtrude_app::tools::move_tool::MoveTool;
use xtrude_app::tools::scale::ScaleTool;
use xtrude_app::viewport::overlay::Paint;
use xtrude_app::viewport::Viewport;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::section::SectionPlane;

/// A viewport looking at a selected unit box, with a section plane through
/// it.
fn selected_box() -> Viewport {
    let mut vp = sized_viewport(800., 600.);
    vp.camera.set_view("iso");
    vp.camera.fit_to(v3(-1., -1., -1.), v3(1., 1., 1.), 1.5);
    add_box(&mut vp, v3(-0.5, -0.5, -0.5), v3(0.5, 0.5, 0.5));
    let faces: Vec<_> = vp.scene.mesh().faces.iter().map(|(f, _)| f).collect();
    for f in faces {
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    vp.scene.doc.section_planes.push(SectionPlane::new(
        v3(0., 0., 0.),
        v3(1., 0., 0.),
        "S1",
        "A",
        true,
        None,
    ));
    vp
}

/// Index of the first section-plane symbol in `out`.
fn first_symbol(out: &[Paint]) -> usize {
    out.iter()
        .position(|p| matches!(p, Paint::Text { text, .. } if text == "A"))
        .unwrap()
}

#[test]
fn test_scale_box_paints_beneath_the_section_planes() {
    let mut vp = selected_box();
    vp.set_active_tool(Some(Box::new(ScaleTool::new())));
    let out = vp.overlay();
    let grips: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p, Paint::Rect { fill: Some(_), .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(grips.len(), 26);
    assert!(grips.iter().all(|&i| i < first_symbol(&out)));
}

#[test]
fn test_flip_planes_paint_over_the_section_planes() {
    let mut vp = selected_box();
    vp.set_active_tool(Some(Box::new(FlipTool::new())));
    let out = vp.overlay();
    let planes: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p, Paint::Polygon { pen: Some(_), .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(planes.len(), 3);
    assert!(planes.iter().all(|&i| i > first_symbol(&out)));
}

#[test]
fn test_move_rotation_grips_paint_over_the_section_planes() {
    let mut vp = selected_box();
    vp.set_active_tool(Some(Box::new(MoveTool::new())));
    let mv = vp.tool_mut::<MoveTool>().unwrap();
    mv.grips = vec![
        (v3(0., 0., 0.), v3(1., 0., 0.), v3(0., 0., 1.)),
        (v3(0.5, 0., 0.), v3(1., 0., 0.), v3(0., 0., 1.)),
    ];
    mv.hot_grip = Some(1);
    let out = vp.overlay();
    let red = [220. / 255., 40. / 255., 40. / 255., 1.];
    let widths = |paint: &[Paint]| -> Vec<f32> {
        paint
            .iter()
            .filter_map(|p| match p {
                Paint::Line { pen, .. } if pen.color == red => Some(pen.width),
                _ => None,
            })
            .collect()
    };
    // A cross per grip, the hot one thicker, all after the section plane.
    assert_eq!(widths(&out[first_symbol(&out)..]), vec![2., 2., 3., 3.]);
    assert!(widths(&out[..first_symbol(&out)]).is_empty());
}
