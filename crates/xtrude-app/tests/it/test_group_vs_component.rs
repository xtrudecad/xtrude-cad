// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Viewport test: copies of a GROUP share their geometry only
//! until one is edited.

use crate::common;

use common::{v3, window};
use xtrude_app::tray::properties::Properties;
use xtrude_core::Mesh;
use xtrude_model::commands::{GroupToComponentCommand, MakeNestedGroupCommand};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{add_mesh, copy_group, Group};

#[test]
fn test_a_copied_group_becomes_its_own_when_opened() {
    let mut w = window();
    let vp = &mut w.viewport;
    let mut m = Mesh::new();
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(1., 0., 0.),
            v3(1., 1., 0.),
            v3(0., 1., 0.),
        ],
        &[],
    );
    let g = Group::new(add_mesh(&mut vp.scene.doc.meshes, m), Some("Shell"));
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.clear_selection();
    vp.scene.selection.insert(EntityRef::Group(gid));
    assert!(vp.copy_selection());
    let clip = vp.clipboard.clone().unwrap();
    let mut tpl = clip.groups[0].clone();
    assert!(!tpl.is_component());
    // The template's mesh lives in the clipboard's table; a paste brings it
    // into the document's.
    vp.scene
        .doc
        .meshes
        .insert(tpl.mesh, clip.meshes[&tpl.mesh].clone());
    let s1 = copy_group(
        &mut vp.scene.doc.meshes,
        &mut tpl,
        Some(v3(5., 0., 0.)),
        false,
    );
    let s2 = copy_group(
        &mut vp.scene.doc.meshes,
        &mut tpl,
        Some(v3(9., 0., 0.)),
        false,
    );
    let (id1, id2) = (s1.id, s2.id);
    vp.scene.doc.groups.extend([s1, s2]);
    vp.scene.version += 1;
    let mesh_of =
        |vp: &xtrude_app::viewport::Viewport, id| vp.scene.doc.find_group(id).unwrap().mesh;
    assert_eq!(mesh_of(vp, id1), mesh_of(vp, id2));
    vp.begin_group_edit(id1);
    vp.end_group_edit();
    assert_ne!(mesh_of(vp, id1), mesh_of(vp, id2));
}

#[test]
fn test_the_panel_says_group_for_a_group_of_groups() {
    let mut w = window();
    let mut ids = Vec::new();
    for (x, n) in [(0., "A"), (3., "B")] {
        let mut m = Mesh::new();
        m.add_face(
            &[
                v3(x, 0., 0.),
                v3(x + 1., 0., 0.),
                v3(x + 1., 1., 0.),
                v3(x, 1., 0.),
            ],
            &[],
        );
        let g = Group::new(add_mesh(&mut w.viewport.scene.doc.meshes, m), Some(n));
        ids.push(g.id);
        w.viewport.scene.doc.groups.push(g);
    }
    let mut c = MakeNestedGroupCommand::new(w.viewport.scene.mesh(), &[], &[], ids, Some("Spacer"));
    w.viewport.execute(&mut c);
    let bx = w
        .viewport
        .scene
        .doc
        .groups
        .iter()
        .find(|g| !g.children.is_empty())
        .unwrap()
        .id;
    let mut panel = Properties::default();
    let text = panel.describe(&w, &[EntityRef::Group(bx)]);
    assert!(
        !text.contains("Component") && text.contains("Group"),
        "{text}"
    );
    w.viewport.execute(&mut GroupToComponentCommand {
        group: bx,
        name: Some("Spacer".into()),
    });
    let text = panel.describe(&w, &[EntityRef::Group(bx)]);
    assert!(text.contains("Component"), "{text}");
}
