// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The selection dots turn blue on an orange or red surface, each side judged
//! against its own paint. The frame carries the colour in each vertex, so the
//! runs are read off the selection draw's vertices.

use crate::common;

use common::{stub, v3, viewport};
use xtrude_core::attrs::Back;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_render::frame::{Source, Stipple};
use xtrude_render::selection::{selection_dot_color, SELECTION_DOT_ALT_COLOR, SELECTION_DOT_COLOR};

#[test]
fn test_dot_colour_switches_on_orange_and_red_surfaces() {
    assert_eq!(
        selection_dot_color(Some([0.96, 0.95, 0.925])),
        SELECTION_DOT_COLOR
    );
    assert_eq!(
        selection_dot_color(Some([0.62, 0.70, 0.78])),
        SELECTION_DOT_COLOR
    );
    assert_eq!(selection_dot_color(None), SELECTION_DOT_COLOR);
    assert_eq!(
        selection_dot_color(Some([0.95, 0.45, 0.16])),
        SELECTION_DOT_ALT_COLOR
    );
    assert_eq!(
        selection_dot_color(Some([0.85, 0.15, 0.10])),
        SELECTION_DOT_ALT_COLOR
    );
    // A pale, washed-out peach is not orange enough to hide the dots.
    assert_eq!(
        selection_dot_color(Some([0.95, 0.90, 0.85])),
        SELECTION_DOT_COLOR
    );
}

#[test]
fn test_each_side_is_judged_against_its_own_paint() {
    let mut vp = stub();
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    // Default cream front, default blue-grey back: orange on both.
    assert_eq!(
        vp.selection_dot_colors(MeshId::LOOSE, f),
        (SELECTION_DOT_COLOR, SELECTION_DOT_COLOR)
    );
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.95, 0.50, 0.15]); // orange front
    assert_eq!(
        vp.selection_dot_colors(MeshId::LOOSE, f),
        (SELECTION_DOT_ALT_COLOR, SELECTION_DOT_COLOR)
    );
    vp.scene.mesh_mut().face_mut(f).attrs.back = Some(Back::Mirror); // both sides orange
    assert_eq!(
        vp.selection_dot_colors(MeshId::LOOSE, f),
        (SELECTION_DOT_ALT_COLOR, SELECTION_DOT_ALT_COLOR)
    );
}

#[test]
fn test_selected_faces_are_split_into_runs_per_dot_colour() {
    let mut vp = viewport();
    let quad = |x: f64| {
        [
            v3(x, 0., 0.),
            v3(x + 1., 0., 0.),
            v3(x + 1., 1., 0.),
            v3(x, 1., 0.),
        ]
    };
    let plain = vp.scene.mesh_mut().add_face(&quad(0.0), &[]);
    let orange = vp.scene.mesh_mut().add_face(&quad(2.0), &[]);
    vp.scene.mesh_mut().face_mut(orange).attrs.paint.color = Some([0.95, 0.45, 0.16]);
    for f in [plain, orange] {
        vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    }
    vp.scene.version += 1;
    let frame = vp.frame_data([800, 600], false);
    let sel: Vec<_> = frame
        .draws
        .iter()
        .filter(|d| d.stipple == Stipple::Selection && d.source == Source::Faces)
        .collect();
    let verts: Vec<[f32; 4]> = sel
        .iter()
        .flat_map(|d| &frame.faces[d.first as usize..(d.first + d.count) as usize])
        .map(|v| v.front)
        .collect();
    // runs of one dot colour each
    let mut spans: Vec<([f32; 4], usize)> = Vec::new();
    for c in &verts {
        match spans.last_mut() {
            Some((k, n)) if k == c => *n += 1,
            _ => spans.push((*c, 1)),
        }
    }
    assert_eq!(spans.len(), 2, "{spans:?}");
    // every selected face's triangles are in them: two quads, two each
    assert_eq!(spans.iter().map(|s| s.1).sum::<usize>(), 2 * 2 * 3);
}
