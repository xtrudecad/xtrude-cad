// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A face has two sides and Paint paints the clicked one. The document test lives
//! in crates/xtrude-io. The drawing tests read the frame cache
//! (`Viewport::frame_data`).

use crate::common;
use crate::common::FakeProbe;

use common::{rgb_key, scene_triangles, stub, v3, Corner};
use glam::DMat4;
use xtrude_app::input::{Key, KeyEvent, Modifiers};
use xtrude_app::tools::paint::{
    clicked_back_side, paint_state, set_paint_state, PaintState, PaintTool, DEFAULT_FACE_COLOR,
};
use xtrude_app::tools::{set_paint_sample_armed, Tool, ToolContext};
use xtrude_app::viewport::{FaceHit, Viewport};
use xtrude_core::attrs::{Back, FaceAttrs, Paint};
use xtrude_core::{DVec3, FaceId, Mesh};
use xtrude_model::commands::{MoveGroupCommand, SetFaceBackCommand, SetFaceColorCommand};
use xtrude_model::group::{add_mesh, Group};
use xtrude_model::history::History;
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::materials::{back_is_default, is_translucent, Material};
use xtrude_model::scene::Scene;
use xtrude_model::style::DEFAULT_BACK_COLOR;

fn quad(m: &mut Mesh) -> FaceId {
    m.add_face(
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
        &[],
    )
}

/// A viewport whose ray looks at the face from ABOVE (the front, +Z
/// normal) or from BELOW (the back).
fn sided_viewport(from_below: bool) -> Viewport {
    set_paint_state(PaintState::default());
    set_paint_sample_armed(false);
    let mut vp = stub();
    FakeProbe::of(&mut vp).pixel_to_ray(move |_, _| {
        Some(if from_below {
            (v3(2., 2., -10.), v3(0., 0., 1.))
        } else {
            (v3(2., 2., 10.), v3(0., 0., -1.))
        })
    });
    vp
}

fn hit(face: FaceId) -> FaceHit {
    FaceHit {
        mesh: MeshId::LOOSE,
        face,
        placement: None,
        xform: None,
        owner: None,
    }
}

fn click(vp: &mut Viewport, face: FaceId, mods: Modifiers) {
    FakeProbe::of(vp).pick(Some(hit(face)));
    PaintTool::new().on_click(vp, &ToolContext::at(DVec3::ZERO).with_modifiers(mods));
}

fn attrs(vp: &Viewport, f: FaceId) -> &FaceAttrs {
    &vp.scene.mesh().face(f).attrs
}

fn red(f: &mut FaceAttrs) {
    f.paint.color = Some([1.0, 0.0, 0.0]);
}

fn painted(c: [f64; 3]) -> Paint {
    Paint {
        color: Some(c),
        ..Paint::default()
    }
}

#[test]
fn test_the_back_is_the_default_unless_it_has_its_own_or_the_front_is_translucent() {
    assert!(back_is_default(&FaceAttrs::default()));
    let mut a = FaceAttrs::default();
    red(&mut a);
    assert!(back_is_default(&a));
    a.back = Some(Back::Mirror);
    assert!(!back_is_default(&a));
    a.back = Some(Back::Own(painted([0., 0., 1.])));
    assert!(!back_is_default(&a));
    // glass / water: opacity < 1
    let g = FaceAttrs {
        paint: Paint {
            color: Some([0., 0., 1.]),
            opacity: Some(0.4),
            ..Paint::default()
        },
        ..FaceAttrs::default()
    };
    assert!(is_translucent(&g));
    assert!(!back_is_default(&g));
}

#[test]
fn test_the_clicked_side_comes_from_the_ray_against_the_normal() {
    let mut vp = sided_viewport(false);
    let f = quad(vp.scene.mesh_mut());
    assert!(!clicked_back_side(&vp, &hit(f), 0., 0.));
    let mut below = sided_viewport(true);
    let f = quad(below.scene.mesh_mut());
    assert!(clicked_back_side(&below, &hit(f), 0., 0.));
}

#[test]
fn test_painting_the_front_leaves_the_back_alone() {
    let mut vp = sided_viewport(false);
    let f = quad(vp.scene.mesh_mut());
    set_paint_state(PaintState {
        color: [1.0, 0.0, 0.0],
        ..PaintState::default()
    });
    click(&mut vp, f, Modifiers::NONE);
    assert_eq!(attrs(&vp, f).paint.color, Some([1.0, 0.0, 0.0]));
    assert!(
        attrs(&vp, f).back.is_none(),
        "the back keeps the default colour"
    );
    assert!(back_is_default(attrs(&vp, f)));
    set_paint_state(PaintState::default());
}

#[test]
fn test_painting_the_back_gives_it_its_own_color_and_keeps_the_front() {
    let mut vp = sided_viewport(true);
    let f = quad(vp.scene.mesh_mut());
    red(&mut vp.scene.mesh_mut().face_mut(f).attrs);
    set_paint_state(PaintState {
        color: [0.0, 0.0, 1.0],
        named_color: Some(Material::new("Blue", Some([0.0, 0.0, 1.0]), None)),
        ..PaintState::default()
    });
    click(&mut vp, f, Modifiers::NONE);
    let a = attrs(&vp, f);
    assert_eq!(
        a.paint.color,
        Some([1.0, 0.0, 0.0]),
        "the front is unchanged"
    );
    assert_eq!(
        a.back,
        Some(Back::Own(Paint {
            color: Some([0.0, 0.0, 1.0]),
            mat: Some("Blue".into()),
            opacity: None,
        }))
    );
    assert!(
        a.paint.mat.is_none(),
        "the identity belongs to the painted side"
    );
    assert!(vp.scene.doc.materials.contains_key("Blue"));
    assert!(vp.undo());
    assert!(attrs(&vp, f).back.is_none());
    assert!(!vp.scene.doc.materials.contains_key("Blue"));
    set_paint_state(PaintState::default());
}

#[test]
fn test_the_eyedropper_takes_the_side_under_the_cursor() {
    let mut vp = sided_viewport(false);
    let f = quad(vp.scene.mesh_mut());
    {
        let a = &mut vp.scene.mesh_mut().face_mut(f).attrs;
        red(a);
        a.back = Some(Back::Own(painted([0., 0., 1.])));
    }
    click(&mut vp, f, Modifiers::ALT);
    assert_eq!(paint_state().color, [1.0, 0.0, 0.0]);
    // the same scene seen from below
    FakeProbe::of(&mut vp).pixel_to_ray(|_, _| Some((v3(2., 2., -10.), v3(0., 0., 1.))));
    click(&mut vp, f, Modifiers::ALT);
    assert_eq!(paint_state().color, [0.0, 0.0, 1.0]);
    // a default back samples the default paint
    vp.scene.mesh_mut().face_mut(f).attrs.back = None;
    click(&mut vp, f, Modifiers::ALT);
    assert_eq!(paint_state().color, DEFAULT_FACE_COLOR);
    // a two-sided face samples its front from behind
    vp.scene.mesh_mut().face_mut(f).attrs.back = Some(Back::Mirror);
    click(&mut vp, f, Modifiers::ALT);
    assert_eq!(paint_state().color, [1.0, 0.0, 0.0]);
    set_paint_state(PaintState::default());
}

#[test]
fn test_set_face_back_command_undoes_to_what_was_there() {
    let mut scene = Scene::new();
    let mut hist = History::new();
    let f = quad(scene.mesh_mut());
    scene.mesh_mut().face_mut(f).attrs.back = Some(Back::Mirror);
    let green = Back::Own(painted([0., 1., 0.]));
    let mut c1 = SetFaceBackCommand::uniform(vec![(MeshId::LOOSE, f)], Some(green.clone()));
    hist.execute(&mut scene, &mut c1);
    assert_eq!(scene.mesh().face(f).attrs.back, Some(green.clone()));
    let mut c2 = SetFaceBackCommand::uniform(vec![(MeshId::LOOSE, f)], None);
    hist.execute(&mut scene, &mut c2);
    assert!(scene.mesh().face(f).attrs.back.is_none());
    assert!(hist.undo(&mut scene));
    assert_eq!(scene.mesh().face(f).attrs.back, Some(green));
    assert!(hist.undo(&mut scene));
    assert_eq!(scene.mesh().face(f).attrs.back, Some(Back::Mirror));
}

// ---- the "default" swatch (no color)

#[test]
fn test_the_eyedropper_takes_no_color_and_painting_it_removes_it() {
    let mut vp = sided_viewport(false);
    let clean = quad(vp.scene.mesh_mut());
    let brick = vp.scene.mesh_mut().add_face(
        &[
            v3(10., 0., 0.),
            v3(14., 0., 0.),
            v3(14., 4., 0.),
            v3(10., 4., 0.),
        ],
        &[],
    );
    vp.scene.mesh_mut().face_mut(brick).attrs.paint = Paint {
        color: Some([1.0, 0.0, 0.0]),
        mat: Some("Brick red".into()),
        opacity: Some(0.5),
    };
    click(&mut vp, clean, Modifiers::ALT); // samples "no color"
    assert!(paint_state().is_default);
    click(&mut vp, brick, Modifiers::NONE); // and paints it: removes it
    assert!(attrs(&vp, brick).paint.is_empty());
    assert!(vp.undo());
    let p = &attrs(&vp, brick).paint;
    assert!(p.color == Some([1.0, 0.0, 0.0]) && p.mat.as_deref() == Some("Brick red"));
    click(&mut vp, brick, Modifiers::ALT); // a real color
    assert!(!paint_state().is_default);
    set_paint_state(PaintState::default());
}

#[test]
fn test_no_color_on_the_back_returns_it_to_the_default() {
    let mut vp = sided_viewport(true);
    let f = quad(vp.scene.mesh_mut());
    vp.scene.mesh_mut().face_mut(f).attrs.back = Some(Back::Own(painted([0., 0., 1.])));
    set_paint_state(PaintState {
        is_default: true,
        ..PaintState::default()
    });
    click(&mut vp, f, Modifiers::NONE);
    assert!(attrs(&vp, f).back.is_none());
    set_paint_state(PaintState::default());
}

#[test]
fn test_with_the_eyedropper_on_the_pointer_the_click_samples() {
    let mut vp = sided_viewport(false);
    let f = quad(vp.scene.mesh_mut());
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.0, 1.0, 0.0]);
    // the pointer is already the eyedropper
    vp.app_key_event(&KeyEvent::new(Key::Alt, "", Modifiers::ALT), true);
    assert!(vp.alt_down());
    click(&mut vp, f, Modifiers::NONE); // an event without modifiers
    assert_eq!(paint_state().color, [0.0, 1.0, 0.0]);
    assert_eq!(attrs(&vp, f).paint.color, Some([0.0, 1.0, 0.0])); // no paint
    set_paint_state(PaintState::default());
}

// ---- the drawing -------------------------------------------------------------
// Xtrude CAD bakes both side colours into every face vertex of the frame
// cache, and the shader picks one by facing; the checks read that cache.

fn sq(m: &mut Mesh, x0: f64) -> FaceId {
    m.add_face(
        &[
            v3(x0, 0., 0.),
            v3(x0 + 1., 0., 0.),
            v3(x0 + 1., 1., 0.),
            v3(x0, 1., 0.),
        ],
        &[],
    )
}

fn instance(vp: &mut Viewport, proto: Mesh, xform: DMat4) -> (GroupId, MeshId) {
    let mid = add_mesh(&mut vp.scene.doc.meshes, proto);
    let mut g = Group::new(mid, Some("Arch"));
    g.xform = Some(xform);
    let gid = g.id;
    vp.scene.doc.groups.push(g);
    vp.scene.version += 1;
    (gid, mid)
}

/// Does this corner carry the style's default back colour?
fn default_back(c: &Corner) -> bool {
    let [r, g, b] = DEFAULT_BACK_COLOR.map(|x| x as f32);
    rgb_key(c.back) == rgb_key([r, g, b, 1.0])
}

#[test]
fn test_the_chunk_carries_the_default_back_triangles() {
    // Only faces with a default back and an opaque front wear the tint.
    let mut vp = stub();
    let mut m = Mesh::new();
    let default_face = sq(&mut m, 0.);
    red(&mut m.face_mut(default_face).attrs);
    let dos = sq(&mut m, 2.);
    red(&mut m.face_mut(dos).attrs);
    m.face_mut(dos).attrs.back = Some(Back::Mirror);
    let glass = sq(&mut m, 4.);
    m.face_mut(glass).attrs.paint.color = Some([0., 0., 1.]);
    m.face_mut(glass).attrs.paint.opacity = Some(0.3);
    sq(&mut m, 6.); // unpainted
    let mid = add_mesh(&mut vp.scene.doc.meshes, m);
    vp.scene.doc.groups.push(Group::new(mid, Some("g")));
    vp.scene.version += 1;
    let tris: Vec<_> = scene_triangles(&mut vp)
        .into_iter()
        .filter(|t| t.iter().all(default_back))
        .collect();
    let mut xs: Vec<i64> = tris
        .iter()
        .map(|t| t.iter().map(|c| c.pos.x).fold(f64::MAX, f64::min).round() as i64)
        .collect();
    xs.sort();
    xs.dedup();
    assert_eq!(xs, vec![0, 6], "only the front-painted and the unpainted");
    assert_eq!(tris.len(), 4); // two triangles per face
}

#[test]
fn test_the_chunk_fingerprint_sees_the_back_and_the_opacity() {
    // Painting the BACK of a face changed nothing on screen: the cache's
    // key saw colour and layer only. The back and the translucency are
    // baked too, so editing them must rebuild what is drawn.
    let mut vp = stub();
    let f = quad(vp.scene.mesh_mut());
    vp.scene.mesh_mut().face_mut(f).attrs.paint.color = Some([0.7, 0.6, 0.5]);
    let backs = |vp: &mut Viewport| -> Vec<[i32; 3]> {
        scene_triangles(vp)
            .iter()
            .flatten()
            .map(|c| rgb_key(c.back))
            .collect()
    };
    let b0 = backs(&mut vp);
    vp.scene.mesh_mut().face_mut(f).attrs.back = Some(Back::Own(painted([0.4, 0.4, 0.4])));
    let b_back = backs(&mut vp);
    assert_ne!(b_back, b0);
    vp.scene.mesh_mut().face_mut(f).attrs.paint.opacity = Some(0.4);
    let alphas: Vec<i32> = scene_triangles(&mut vp)
        .iter()
        .flatten()
        .map(|c| (c.front[3] * 1000.0).round() as i32)
        .collect();
    assert!(alphas.iter().all(|&a| a == 400), "{alphas:?}");
}

#[test]
fn test_chunks_are_told_apart_by_build_number_not_by_id() {
    // A face painted inside a component came back unpainted one level up:
    // the instance kept what it derived from the old prototype bake.
    let mut vp = stub();
    let mut proto = Mesh::new();
    let f = quad(&mut proto);
    let (_g, mid) = instance(&mut vp, proto, DMat4::IDENTITY);
    let fronts = |vp: &mut Viewport| -> Vec<[i32; 3]> {
        scene_triangles(vp)
            .iter()
            .flatten()
            .map(|c| rgb_key(c.front))
            .collect()
    };
    let vcol0 = fronts(&mut vp);
    vp.run(SetFaceColorCommand {
        faces: vec![(mid, f)],
        color: Some([0.8, 0.2, 0.1]),
    });
    assert_ne!(
        fronts(&mut vp),
        vcol0,
        "the instance follows the repainted prototype"
    );
}

#[test]
fn test_the_back_tint_travels_with_the_instance_when_moved() {
    // "I rotated the component, moved it and that ghost stayed": the back
    // tint was drawn where the arch had been.
    let mut vp = stub();
    let mut proto = Mesh::new();
    let f = quad(&mut proto);
    red(&mut proto.face_mut(f).attrs); // default back: the tint
    let (g, _mid) = instance(&mut vp, proto, DMat4::IDENTITY);
    let tinted_min_x = |vp: &mut Viewport| {
        let corners: Vec<Corner> = scene_triangles(vp)
            .into_iter()
            .flatten()
            .filter(default_back)
            .collect();
        assert_eq!(corners.len(), 6);
        corners.iter().map(|c| c.pos.x).fold(f64::MAX, f64::min)
    };
    assert!(tinted_min_x(&mut vp).abs() < 1e-6);
    vp.run(MoveGroupCommand {
        group: g,
        delta: DVec3::new(10., 0., 0.),
    });
    assert!(
        (tinted_min_x(&mut vp) - 10.0).abs() < 1e-4,
        "the tint moved with the arch"
    );
}

#[test]
fn test_a_mirrored_instance_keeps_the_front_of_its_faces() {
    // "I mirrored a component and its textures disappeared": a mirror
    // flips every triangle's winding, so the painted side read as the back.
    let mut vp = stub();
    let mut proto = Mesh::new();
    let f = quad(&mut proto); // +Z normal, counter-clockwise
    proto.face_mut(f).attrs.paint.color = Some([1., 0.8, 0.]);
    instance(&mut vp, proto, DMat4::from_scale(DVec3::new(-1., 1., 1.)));
    let tris = scene_triangles(&mut vp);
    assert_eq!(tris.len(), 2);
    for [a, b, c] in tris {
        let n = (b.pos - a.pos).cross(c.pos - a.pos);
        assert!(
            n.z > 0.,
            "still counter-clockwise seen from +Z: the front is the front"
        );
    }
}
