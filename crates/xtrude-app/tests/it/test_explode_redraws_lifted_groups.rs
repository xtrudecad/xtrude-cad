// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A group lifted out by Explode is drawn where it IS. Xtrude CAD
//! draws one frame cache, so each box's triangles are the ones that fall in
//! its box, and none may be drawn elsewhere.

use crate::common;

use common::{draw_rect, push, scene_triangles, v3, viewport};
use glam::{DMat4, DVec3};
use xtrude_app::viewport::Viewport;
use xtrude_model::commands::{
    ExplodeGroupCommand, InsertGroupCommand, MakeGroupCommand, MakeNestedGroupCommand,
};
use xtrude_model::group::copy_group;
use xtrude_model::ids::GroupId;

/// A unit cube pushed up from a rectangle, grouped.
fn box_group(vp: &mut Viewport) -> GroupId {
    let v = |x, y| v3(x, y, 0.);
    draw_rect(vp, &[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)]);
    let f = vp.scene.mesh().face_ids()[0];
    let d = common::up(vp, f, 1.0);
    push(vp, f, d, false);
    let m = vp.scene.mesh();
    let mut mk = MakeGroupCommand::new(m, &m.face_ids(), &m.edge_ids(), false, None);
    vp.execute(&mut mk);
    vp.scene.doc.groups.last().unwrap().id
}

/// The world box of a group's mesh (its vertices as they are stored).
fn mesh_bbox(vp: &Viewport, g: GroupId) -> (DVec3, DVec3) {
    let grp = vp.scene.doc.find_group(g).unwrap();
    let m = vp.scene.doc.mesh(grp.mesh);
    let pts: Vec<DVec3> = m
        .vertex_ids()
        .iter()
        .map(|&v| m.verts[v].position)
        .collect();
    let lo = pts.iter().fold(DVec3::INFINITY, |a, p| a.min(*p));
    let hi = pts.iter().fold(DVec3::NEG_INFINITY, |a, p| a.max(*p));
    (lo, hi)
}

#[test]
fn test_exploded_copies_are_drawn_where_they_are() {
    let mut vp = viewport();
    vp.scene.doc.groups.clear(); // no scale figure
    let first = box_group(&mut vp);
    let mut boxes = vec![first];
    for dx in [2.0, 4.0] {
        // Move + Ctrl copies
        let i = vp
            .scene
            .doc
            .groups
            .iter()
            .position(|g| g.id == first)
            .unwrap();
        let mut src = vp.scene.doc.groups[i].clone();
        let dup = copy_group(
            &mut vp.scene.doc.meshes,
            &mut src,
            Some(v3(dx, 0., 0.)),
            false,
        );
        vp.scene.doc.groups[i] = src;
        boxes.push(dup.id);
        vp.execute(&mut InsertGroupCommand::new(dup));
    }
    scene_triangles(&mut vp); // drawn once, top level
    let mut nest =
        MakeNestedGroupCommand::new(vp.scene.mesh(), &[], &[], boxes.clone(), Some("Row"));
    vp.execute(&mut nest);
    let row = vp.scene.doc.groups.last().unwrap().id;
    {
        let r = vp.scene.doc.find_group_mut(row).unwrap();
        let m = DMat4::from_translation(v3(0., 3., 1.));
        r.xform = Some(m * r.xform.unwrap_or(DMat4::IDENTITY)); // the container moved
    }
    vp.scene.version += 1;
    scene_triangles(&mut vp); // drawn nested
    vp.execute(&mut ExplodeGroupCommand { group: row });

    let corners: Vec<DVec3> = scene_triangles(&mut vp)
        .iter()
        .flat_map(|t| t.iter().map(|c| c.pos))
        .collect();
    let eps = 1e-5;
    let inside =
        |p: DVec3, (lo, hi): (DVec3, DVec3)| p.cmpge(lo - eps).all() && p.cmple(hi + eps).all();
    let mut checked = Vec::new();
    for &g in &boxes {
        let grp = vp.scene.doc.find_group(g).unwrap();
        if grp.xform.is_some() {
            continue;
        }
        let (mlo, mhi) = mesh_bbox(&vp, g);
        let drawn: Vec<DVec3> = corners
            .iter()
            .copied()
            .filter(|p| inside(*p, (mlo, mhi)))
            .collect();
        assert!(!drawn.is_empty(), "box {g:?} is not drawn");
        let lo = drawn.iter().fold(DVec3::INFINITY, |a, p| a.min(*p));
        let hi = drawn.iter().fold(DVec3::NEG_INFINITY, |a, p| a.max(*p));
        assert!((lo - mlo).abs().max_element() < eps, "{g:?} {lo:?} {mlo:?}");
        assert!((hi - mhi).abs().max_element() < eps, "{g:?} {hi:?} {mhi:?}");
        checked.push((mlo, mhi));
    }
    assert!(!checked.is_empty());
    // nothing is drawn where a box stood before it was grouped and moved
    for p in &corners {
        assert!(
            checked.iter().any(|b| inside(*p, *b)),
            "a triangle drawn at {p:?}, where no box is"
        );
    }
}
