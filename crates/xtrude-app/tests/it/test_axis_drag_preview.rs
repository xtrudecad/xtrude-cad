// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A drag from the protractor's vertex tilts the disc WHILE it happens
//! (Protractor and Rotate); a move under the threshold shows nothing; the
//! release commits exactly the axis the preview showed; a plain click keeps
//! the inferred plane.

use crate::common;
use crate::common::FakeProbe;

use common::{stub, v3};
use glam::DVec3;
use xtrude_app::tools::protractor::{ProtractorBase, ProtractorTool};
use xtrude_app::tools::rotate::RotateTool;
use xtrude_app::tools::{Tool, ToolContext};
use xtrude_app::viewport::Viewport;
use xtrude_core::math::VecExt;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;

/// A front elevation at 50 px per metre: X to the right, Z up.
fn vp() -> Viewport {
    let mut vp = stub();
    FakeProbe::of(&mut vp).world_to_pixel(|p: DVec3| Some((p.x * 50.0, -p.z * 50.0)));
    let f = vp.scene.mesh_mut().add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    vp.scene.selection.insert(EntityRef::Face(MeshId::LOOSE, f));
    vp
}

fn ctx(x: f64, y: f64, z: f64) -> ToolContext {
    ToolContext::at(v3(x, y, z))
}

/// Each tool with its protractor state reachable.
/// A tool, how to reach its protractor state, and its viewport.
type Case = (Box<dyn Tool>, fn(&dyn Tool) -> &ProtractorBase, Viewport);

fn tools() -> Vec<Case> {
    fn prot(t: &dyn Tool) -> &ProtractorBase {
        &t.as_any().downcast_ref::<ProtractorTool>().unwrap().p
    }
    fn rot(t: &dyn Tool) -> &ProtractorBase {
        &t.as_any().downcast_ref::<RotateTool>().unwrap().p
    }
    vec![
        (Box::new(ProtractorTool::new()), prot, vp()),
        (Box::new(RotateTool::new()), rot, vp()),
    ]
}

/// The plane the drawn disc lies in, read back from its own segments.
fn disc_normal(t: &dyn Tool, base: &ProtractorBase, vp: &Viewport) -> Option<DVec3> {
    let c = base.start_point?;
    let pts: Vec<DVec3> = t
        .rubber_band_lines(vp)
        .into_iter()
        .take(24)
        .flat_map(|(a, b)| [a, b])
        .collect();
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            let n = (pts[i] - c).cross(pts[j] - c);
            if n.length() > 1e-6 {
                return Some(n.unit_or_zero());
            }
        }
    }
    None
}

#[test]
fn test_the_disc_tilts_while_the_drag_is_happening() {
    for (mut t, p, mut vp) in tools() {
        let name = t.info().key;
        t.on_click(&mut vp, &ctx(0., 0., 0.)); // press at the vertex
        let flat = disc_normal(t.as_ref(), p(t.as_ref()), &vp).expect("a disc");
        // Diagonal on purpose: straight up is the plane it already is in.
        t.on_hover(&mut vp, &ctx(2., 0., 2.)); // 141 px: a real drag
        assert!(p(t.as_ref()).axis_drag_live.is_some(), "{name}");
        let tilted = disc_normal(t.as_ref(), p(t.as_ref()), &vp).expect("a disc");
        assert!(flat.dot(tilted).abs() < 0.99, "{name}");
    }
}

#[test]
fn test_a_move_too_small_to_count_shows_nothing() {
    for (mut t, p, mut vp) in tools() {
        t.on_click(&mut vp, &ctx(0., 0., 0.));
        t.on_hover(&mut vp, &ctx(0.05, 0., 0.)); // 2.5 px at 50 px/m
        assert!(p(t.as_ref()).axis_drag_live.is_none(), "{}", t.info().key);
    }
}

#[test]
fn test_the_release_commits_exactly_the_axis_the_preview_showed() {
    for (mut t, p, mut vp) in tools() {
        t.on_click(&mut vp, &ctx(0., 0., 0.));
        t.on_hover(&mut vp, &ctx(2., 0., 2.));
        let previewed = p(t.as_ref()).axis_drag_live.unwrap();
        t.on_release(&mut vp);
        let base = p(t.as_ref());
        let axis = base.custom_axis.expect("an axis");
        assert!((axis - previewed).length() < 1e-9, "{}", t.info().key);
        assert!(base.axis_drag_live.is_none()); // the window closed
    }
}

#[test]
fn test_a_plain_click_still_keeps_the_inferred_plane() {
    for (mut t, p, mut vp) in tools() {
        t.on_click(&mut vp, &ctx(0., 0., 0.));
        t.on_release(&mut vp); // no move in between
        assert!(p(t.as_ref()).custom_axis.is_none(), "{}", t.info().key);
    }
}
