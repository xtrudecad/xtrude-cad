// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The `doc` API: a script run is one command and one undo step, builds the
//! same geometry as the commands it wraps, and works in model units.

use xtrude_core::DVec3;
use xtrude_model::commands::AddFaceCommand;
use xtrude_model::pushpull::push_face;
use xtrude_model::scene::mesh_equal;
use xtrude_model::units::Units;
use xtrude_model::EntityRef;

use crate::common::{self, Fixture};

const BOX: &str = r#"
local doc = ...
local base = doc:add_face({ {0, 0}, {20, 0}, {20, 20}, {0, 20} })
local cap = doc:push_pull(base, 10)
assert(cap, "the push returns its cap")
"#;

fn box_corners(fx: &Fixture) -> (DVec3, DVec3) {
    fx.scene.compute_bounds().expect("geometry")
}

#[test]
fn script_box_matches_add_face_and_push_face() {
    let mut fx = common::fixture();
    assert!(fx.run("Box", BOX), "{:?}", fx.history.last_error);

    let (mut expected, mut h) = (xtrude_model::Scene::new(), xtrude_model::History::new());
    let mm = 0.001;
    let square = vec![
        DVec3::new(0., 0., 0.),
        DVec3::new(20. * mm, 0., 0.),
        DVec3::new(20. * mm, 20. * mm, 0.),
        DVec3::new(0., 20. * mm, 0.),
    ];
    let mut add = AddFaceCommand::new(square);
    assert!(h.execute(&mut expected, &mut add));
    let face = add.face.expect("face");
    let mid = expected.active_mesh_id();
    assert!(push_face(&mut expected, &mut h, mid, face, 10. * mm, false));

    assert!(mesh_equal(fx.scene.mesh(), expected.mesh()));
}

#[test]
fn script_run_is_one_undo_step_named_after_the_command() {
    let mut fx = common::fixture();
    let empty = fx.scene.mesh().clone();
    assert!(fx.run("Box…", BOX));
    let built = fx.scene.mesh().clone();
    assert_eq!(fx.history.undo_len(), 1);
    assert_eq!(fx.history.undo_name(), Some("Box"));

    assert!(fx.history.undo(&mut fx.scene));
    assert!(mesh_equal(fx.scene.mesh(), &empty));
    assert_eq!(fx.scene.mesh().faces.len(), 0);
    assert!(fx.history.redo(&mut fx.scene));
    assert!(mesh_equal(fx.scene.mesh(), &built));
}

#[test]
fn failing_script_leaves_the_document_untouched() {
    let mut fx = common::fixture();
    assert!(fx.run("Box", BOX));
    let before = fx.scene.mesh().clone();
    // Geometry of its own, built and pushed, then the error.
    let src = "local doc = ...
local f = doc:add_face({ {50, 50}, {60, 50}, {60, 60} })
assert(doc:push_pull(f, 5))
error('x')
";
    assert!(!fx.run("Broken", src));
    assert!(mesh_equal(fx.scene.mesh(), &before));
    assert_eq!(fx.history.undo_len(), 1, "no step for the failed run");
    let err = fx.history.last_error.clone().expect("last_error");
    assert!(err.starts_with("Broken: "), "{err}");
    assert!(err.contains("test.lua:4: x"), "{err}");
    assert!(err.contains("stack traceback:"), "{err}");
}

#[test]
fn stale_face_handle_is_a_lua_error() {
    let mut fx = common::fixture();
    // A pocket pushed through the whole box: the pushed face is gone.
    let src = format!(
        "{BOX}
local hole = doc:add_face({{ {{5, 5, 10}}, {{15, 5, 10}}, {{15, 15, 10}}, {{5, 15, 10}} }})
local n = hole:normal()
doc:push_pull(hole, n.z > 0 and -10 or 10)
assert(not hole:exists())
hole:vertices()
"
    );
    assert!(!fx.run("Stale", &src));
    let err = fx.history.last_error.clone().expect("last_error");
    assert!(err.contains("face no longer exists"), "{err}");
}

#[test]
fn stale_group_handle_is_a_lua_error() {
    let mut fx = common::fixture();
    let src = format!(
        "{BOX}
local g = doc:make_group(doc:faces(), 'Box')
g:move(xtrude.vec(1, 0, 0))
_G.kept = g
"
    );
    assert!(fx.run("Group", &src));
    // The group a later run kept is gone once the first run is undone.
    assert!(fx.history.undo(&mut fx.scene));
    assert!(!fx.run("Move", "local doc = ...\nkept:move(xtrude.vec(1, 0, 0))"));
    let err = fx.history.last_error.clone().expect("last_error");
    assert!(err.contains("group no longer exists"), "{err}");
}

#[test]
fn lengths_are_in_model_units() {
    let mut mm = common::fixture();
    assert!(mm.run("Box", BOX));
    let mut cm = common::fixture();
    cm.scene.apply_units(&Units::new("cm", 2));
    assert!(cm.run("Box", BOX));

    let (lo_mm, hi_mm) = box_corners(&mm);
    let (lo_cm, hi_cm) = box_corners(&cm);
    assert!((lo_cm - lo_mm * 10.).length() < 1e-12);
    assert!((hi_cm - hi_mm * 10.).length() < 1e-12);
    assert!((hi_mm - DVec3::new(0.02, 0.02, 0.01)).length() < 1e-12);
    assert!((hi_cm - DVec3::new(0.2, 0.2, 0.1)).length() < 1e-12);
}

#[test]
fn reading_back_is_in_model_units() {
    let mut fx = common::fixture();
    let src = format!(
        "{BOX}
assert(xtrude.unit() == 'mm')
assert(xtrude.api == 1)
assert(xtrude.version() == '{}')
local lo, hi = doc:bounds()
assert(lo == xtrude.vec(0, 0, 0) and hi == xtrude.vec(20, 20, 10), tostring(hi))
assert(math.abs(cap:area() - 400) < 1e-9)
assert(cap:normal() == xtrude.vec(0, 0, 1))
local vs = cap:vertices()
assert(#vs == 4)
for _, v in ipairs(vs) do assert(math.abs(v.z - 10) < 1e-9) end
assert(#doc:faces() == 6)
",
        xtrude_model::VERSION
    );
    assert!(fx.run("Read", &src), "{:?}", fx.history.last_error);
}

#[test]
fn group_move_rotate_and_scale() {
    let mut fx = common::fixture();
    let src = format!(
        "{BOX}
local g = doc:make_group(doc:faces(), 'Box')
assert(g:name() == 'Box')
assert(#g:faces() == 6)
assert(#doc:faces() == 0)
g:move(xtrude.vec(5, 0, 0))
"
    );
    assert!(fx.run("Move", &src), "{:?}", fx.history.last_error);
    let (lo, hi) = box_corners(&fx);
    assert!((lo - DVec3::new(0.005, 0., 0.)).length() < 1e-12, "{lo}");
    assert!(
        (hi - DVec3::new(0.025, 0.02, 0.01)).length() < 1e-12,
        "{hi}"
    );

    let rotate = "local doc = ...
local g = doc:selection()[1]
g:rotate(xtrude.vec(0, 0, 0), xtrude.vec(0, 0, 1), 90)";
    assert!(fx.run("Rotate", rotate), "{:?}", fx.history.last_error);
    let (lo, hi) = box_corners(&fx);
    assert!((lo - DVec3::new(-0.02, 0.005, 0.)).length() < 1e-9, "{lo}");
    assert!((hi - DVec3::new(0., 0.025, 0.01)).length() < 1e-9, "{hi}");

    let scale = "local doc = ...
local g = doc:selection()[1]
g:scale(xtrude.vec(0, 0, 0), { 1, 1, 3 })
g:scale(xtrude.vec(0, 0, 0), 2)";
    assert!(fx.run("Scale", scale), "{:?}", fx.history.last_error);
    let (lo, hi) = box_corners(&fx);
    assert!((lo - DVec3::new(-0.04, 0.01, 0.)).length() < 1e-9, "{lo}");
    assert!((hi - DVec3::new(0., 0.05, 0.06)).length() < 1e-9, "{hi}");
    assert_eq!(fx.history.undo_len(), 3);
}

#[test]
fn make_component_and_paint() {
    let mut fx = common::fixture();
    let src = format!(
        "{BOX}
doc:set_paint(cap, '#ff0000')
local c = doc:make_component(doc:faces(), 'Part')
doc:set_paint(c, '#00ff00')
doc:select({{ c }})
"
    );
    assert!(fx.run("Part", &src), "{:?}", fx.history.last_error);
    let g = &fx.scene.doc.groups[0];
    assert!(g.is_component());
    assert_eq!(g.name, "Part");
    let paint = g.material.as_ref().and_then(|p| p.color);
    assert_eq!(paint, Some([0., 1., 0.]));
    let red = fx
        .scene
        .doc
        .mesh(g.mesh)
        .faces
        .iter()
        .filter(|(_, f)| f.attrs.paint.color == Some([1., 0., 0.]))
        .count();
    assert_eq!(red, 1);
    let sel: Vec<EntityRef> = fx.scene.selection.iter().copied().collect();
    assert_eq!(sel, vec![EntityRef::Group(g.id)]);
}

#[test]
fn selection_round_trip() {
    let mut fx = common::fixture();
    let src = format!(
        "{BOX}
doc:select({{ base, cap }})
local s = doc:selection()
assert(#s == 2 and s[1] == base and s[2] == cap)
doc:select({{}})
assert(#doc:selection() == 0)
doc:select({{ cap }})
"
    );
    assert!(fx.run("Select", &src), "{:?}", fx.history.last_error);
    assert_eq!(fx.scene.selection.len(), 1);
    assert!(matches!(
        fx.scene.selection.first(),
        Some(EntityRef::Face(..))
    ));
}

#[test]
fn vectors() {
    let fx = common::fixture();
    let ok: bool = fx
        .rt
        .run_chunk(
            "local a, b = xtrude.vec(1, 2, 3), xtrude.vec(4, 5, 6)
assert(a + b == xtrude.vec(5, 7, 9))
assert(b - a == xtrude.vec(3, 3, 3))
assert(a * 2 == xtrude.vec(2, 4, 6) and 2 * a == a * 2)
assert(-a == xtrude.vec(-1, -2, -3))
assert(a:dot(b) == 32)
assert(xtrude.vec(1, 0, 0):cross(xtrude.vec(0, 1, 0)) == xtrude.vec(0, 0, 1))
assert(xtrude.vec(3, 4, 0):length() == 5)
assert(xtrude.vec(0, 0, 2):unit() == xtrude.vec(0, 0, 1))
assert(xtrude.vec(0, 0, 1e-6):unit() == xtrude.vec(0, 0, 0))
assert(a.x == 1 and a.y == 2 and a.z == 3)
assert(xtrude.vec(1, 2).z == 0)
return true",
            "vec.lua",
        )
        .expect("vector script");
    assert!(ok);
}

#[test]
fn bad_arguments_are_lua_errors() {
    for (label, body) in [
        ("Few", "doc:add_face({ {0, 0}, {1, 0} })"),
        ("Flat", "doc:add_face({ {0, 0}, {1, 0}, {2, 0} })"),
        ("Edge", "doc:add_edge({1, 1}, {1, 1})"),
        (
            "Paint",
            "doc:set_paint(doc:add_face({ {0,0},{1,0},{1,1} }), 'red')",
        ),
        ("Group", "doc:make_group({})"),
        ("Kind", "doc:push_pull(xtrude.vec(0, 0, 0), 1)"),
    ] {
        let mut fx = common::fixture();
        let src = format!("local doc = ...\n{body}");
        assert!(!fx.run(label, &src), "{label} must fail");
        assert_eq!(fx.history.undo_len(), 0);
        assert_eq!(fx.scene.mesh().faces.len(), 0, "{label} rolled back");
    }
}

#[test]
fn add_edge_returns_its_edge() {
    let mut fx = common::fixture();
    let src = "local doc = ...
local e = doc:add_edge({0, 0}, {10, 0})
local a, b = table.unpack(e:vertices())
assert(a == xtrude.vec(0, 0, 0) and b == xtrude.vec(10, 0, 0))
local again = doc:add_edge({10, 0}, {0, 0})
assert(again == e)";
    assert!(fx.run("Edge", src), "{:?}", fx.history.last_error);
    assert_eq!(fx.scene.mesh().edges.len(), 1);
}

#[test]
fn failing_script_leaves_the_saved_file_byte_identical() {
    let mut fx = common::fixture();
    assert!(fx.run("Box", BOX));
    let dir = common::TmpDir::new();
    let (before, after) = (dir.0.join("before.xtrude"), dir.0.join("after.xtrude"));
    xtrude_io::document::save_scene(&fx.scene, &before).expect("save before");
    let src = format!(
        "{BOX}
local g = doc:make_group(doc:faces(), 'Box')
g:move(xtrude.vec(0, 0, 50))
doc:set_paint(g, '#123456')
error('late')
"
    );
    assert!(!fx.run("Broken", &src));
    xtrude_io::document::save_scene(&fx.scene, &after).expect("save after");
    let read = |p: &std::path::Path| std::fs::read(p).expect("saved file");
    assert!(read(&before) == read(&after), "the saved files differ");
}

#[test]
fn make_group_with_a_group_open_groups_inside_it() {
    let mut fx = common::fixture();
    assert!(fx.run("Box", BOX));
    let mut outer = xtrude_model::commands::MakeGroupCommand::new(
        fx.scene.mesh(),
        &fx.scene.mesh().face_ids(),
        &[],
        false,
        Some("Outer"),
    );
    assert!(fx.history.execute(&mut fx.scene, &mut outer));
    let oid = outer.group.expect("the outer group");
    fx.scene.begin_group_edit(oid);
    let before = fx.scene.compute_bounds();
    let src = "local doc = ...\ndoc:make_group(doc:faces(), 'Inner')";
    assert!(fx.run("Group", src), "{:?}", fx.history.last_error);
    assert_eq!(fx.scene.doc.groups.len(), 1, "nothing new at the top level");
    let o = fx.scene.doc.find_group(oid).expect("the outer group");
    let names: Vec<&str> = o.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Inner"]);
    assert_eq!(fx.scene.mesh().faces.len(), 0, "the faces went into it");
    assert_eq!(fx.scene.compute_bounds(), before, "where they were");
}
