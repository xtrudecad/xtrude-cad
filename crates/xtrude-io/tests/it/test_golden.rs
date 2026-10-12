// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Golden-file regression check.
//!
//! tests/golden/recipes.json lists modelling recipes; for each, the
//! folder holds a reference document and its exports
//! (<name>.igz/.stl/.obj/.mtl), snapshots of Xtrude CAD's own output.
//! Here each recipe is replayed on the engine and compared with the
//! reference document (same topology, positions within
//! 1e-5 m, same paint), and the reference document is opened and
//! re-exported as STL and OBJ and compared with the reference exports.
//!
//! A changed result fails. When a change to the engine or the exporters
//! deliberately changes a model, regenerate the references and review the
//! diff (only the models the change should touch):
//!
//! ```text
//! XTRUDE_UPDATE_GOLDEN=1 cargo test -p xtrude-io --test it test_golden
//! git diff --stat tests/golden
//! ```
//!
//! The replay test then writes every reference instead of comparing, and
//! the re-export test compares nothing. Never regenerate to make an
//! unexplained failure pass.

use crate::common;
use common::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use xtrude_core::attrs::FaceAttrs;
use xtrude_core::fillet::{apply_fillet, plan_fillet};
use xtrude_core::sweep::{orient_closed_path, sweep_profile};
use xtrude_core::topology::point_in_face_solid;
use xtrude_core::{DVec3, EdgeId, FaceId, Mesh};
use xtrude_io::{igz, obj, stl};
use xtrude_model::commands::*;
use xtrude_model::edits::{build_add_edge, build_add_edges, divide_edges};
use xtrude_model::entity::EntityRef;
use xtrude_model::group::{copy_group, world_mesh, Group};
use xtrude_model::history::{Command, FnCommand, History};
use xtrude_model::intersect::{segments_for, IntersectMode};
use xtrude_model::scene::Scene;
use xtrude_model::solids::SolidOperationCommand;
use xtrude_model::units::Units;

const TOL: f64 = 1e-5;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

fn golden(name: &str) -> PathBuf {
    golden_dir().join(name)
}

/// `XTRUDE_UPDATE_GOLDEN=1`: write the references instead of comparing.
fn updating() -> bool {
    std::env::var_os("XTRUDE_UPDATE_GOLDEN").is_some_and(|v| v == "1")
}

fn p3(v: &Value) -> DVec3 {
    let a = v.as_array().expect("a point");
    DVec3::new(
        a[0].as_f64().unwrap(),
        a[1].as_f64().unwrap(),
        a[2].as_f64().unwrap(),
    )
}

fn face_at(m: &Mesh, p: DVec3) -> FaceId {
    m.face_ids()
        .into_iter()
        .find(|&f| {
            let n = m.face_normal(f);
            (p - m.pos(m.face(f).outer[0])).dot(n).abs() <= 1e-6
                && point_in_face_solid(&m.poly_face(f), p)
        })
        .unwrap_or_else(|| panic!("no face at {p:?}"))
}

fn edge_at(m: &Mesh, p: DVec3) -> EdgeId {
    m.edge_ids()
        .into_iter()
        .find(|&e| {
            let (a, b) = m.edge_ab(e);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
            (a + ab * t - p).length() < 1e-6
        })
        .unwrap_or_else(|| panic!("no edge at {p:?}"))
}

fn box_mesh(lo: DVec3, hi: DVec3) -> Mesh {
    let (x0, y0, z0, x1, y1, z1) = (lo.x, lo.y, lo.z, hi.x, hi.y, hi.z);
    let p = |x, y, z| DVec3::new(x, y, z);
    let mut m = Mesh::new();
    for pts in [
        [p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0), p(x1, y0, z0)],
        [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
        [p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)],
        [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
        [p(x1, y1, z0), p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1)],
        [p(x0, y1, z0), p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1)],
    ] {
        m.add_face(&pts, &[]);
    }
    m
}

fn named(s: &Scene, name: &str) -> Vec<Group> {
    s.doc
        .groups
        .iter()
        .filter(|g| g.name == name)
        .cloned()
        .collect()
}

fn exec(s: &mut Scene, h: &mut History, cmd: &mut dyn Command, what: &str) {
    assert!(h.execute(s, cmd), "{what}: {:?}", h.last_error);
}

/// Replay one recipe on the engine.
fn replay(ops: &[Value]) -> Scene {
    let mut s = Scene::new();
    s.units = Units::new("m", 4);
    let mut h = History::new();
    for op in ops {
        let kind = op["op"].as_str().unwrap();
        let mid = s.active_mesh_id();
        match kind {
            "rect" | "polygon" => {
                let pts: Vec<DVec3> = op["pts"].as_array().unwrap().iter().map(p3).collect();
                let segs: Vec<(DVec3, DVec3)> = (0..pts.len())
                    .map(|i| (pts[i], pts[(i + 1) % pts.len()]))
                    .collect();
                let face: Box<dyn Command> = Box::new(AddFaceCommand::new(pts));
                let mut c = build_add_edges(&s, &segs, false, vec![face], None);
                exec(&mut s, &mut h, &mut *c, kind);
            }
            "circle" => {
                let (c0, r, n) = (
                    p3(&op["center"]),
                    op["radius"].as_f64().unwrap(),
                    op["segments"].as_u64().unwrap() as usize,
                );
                let pts: Vec<DVec3> = (0..n)
                    .map(|i| {
                        let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                        c0 + DVec3::new(r * a.cos(), r * a.sin(), 0.0)
                    })
                    .collect();
                let segs: Vec<(DVec3, DVec3)> =
                    (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect();
                let extra: Vec<Box<dyn Command>> = vec![
                    Box::new(AddFaceCommand::new(pts.clone())),
                    Box::new(TagCurveCommand { pts, closed: true }),
                ];
                let mut c = build_add_edges(&s, &segs, false, extra, None);
                exec(&mut s, &mut h, &mut *c, kind);
            }
            "line" => {
                let mut c = build_add_edge(&s, p3(&op["a"]), p3(&op["b"]), true, None);
                exec(&mut s, &mut h, &mut *c, kind);
            }
            "push" => {
                let f = face_at(s.mesh(), p3(&op["face"]));
                let mut d = op["dist"].as_f64().unwrap();
                if op.get("along").is_some() && s.mesh().face_normal(f).dot(p3(&op["along"])) < 0.0
                {
                    d = -d;
                }
                let keep = op.get("keep").and_then(Value::as_bool).unwrap_or(false);
                xtrude_model::pushpull::push_face(&mut s, &mut h, mid, f, d, keep);
            }
            "move" | "rotate" | "scale" => {
                let f = face_at(s.mesh(), p3(&op["face"]));
                let pos = s.mesh().face_positions(f);
                let mut c: Box<dyn Command> = match kind {
                    "move" => Box::new(MoveVerticesCommand {
                        src: pos,
                        delta: p3(&op["delta"]),
                    }),
                    "rotate" => Box::new(RotateVerticesCommand {
                        src: pos,
                        center: p3(&op["center"]),
                        axis: p3(&op["axis"]),
                        degrees: op["deg"].as_f64().unwrap(),
                    }),
                    _ => {
                        let fac = p3(&op["factor"]);
                        Box::new(ScaleVerticesCommand {
                            src: pos,
                            center: p3(&op["anchor"]),
                            factor: ScaleFactor::PerAxis([fac.x, fac.y, fac.z]),
                            axes: None,
                        })
                    }
                };
                exec(&mut s, &mut h, &mut *c, kind);
            }
            "fillet" => {
                let edges: Vec<EdgeId> = op["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| edge_at(s.mesh(), p3(p)))
                    .collect();
                let plan = plan_fillet(
                    s.mesh(),
                    &edges,
                    op["radius"].as_f64().unwrap(),
                    op["segments"].as_u64().unwrap() as usize,
                )
                .expect("fillet plan");
                let mut c = FnCommand::new("SnapshotMutation", move |sc: &mut Scene| {
                    apply_fillet(sc.mesh_mut(), &plan);
                    Ok(())
                });
                exec(&mut s, &mut h, &mut c, kind);
            }
            "sweep" => {
                let f = face_at(s.mesh(), p3(&op["face"]));
                let mut path: Vec<DVec3> = op["path"].as_array().unwrap().iter().map(p3).collect();
                let closed = op["closed"].as_bool().unwrap();
                if closed {
                    path = orient_closed_path(
                        &path,
                        s.mesh().face_centroid(f),
                        s.mesh().face_normal(f),
                    );
                }
                let mut c = FnCommand::new("SnapshotMutation", move |sc: &mut Scene| {
                    sweep_profile(sc.mesh_mut(), f, &path, closed);
                    Ok(())
                });
                exec(&mut s, &mut h, &mut c, kind);
            }
            "box_group" => {
                let id = s.doc.add_mesh(box_mesh(p3(&op["min"]), p3(&op["max"])));
                let g = Group::new(id, op["name"].as_str());
                exec(&mut s, &mut h, &mut InsertGroupCommand::new(g), kind);
            }
            "solid" => {
                let gs: Vec<_> = op["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| named(&s, n.as_str().unwrap())[0].id)
                    .collect();
                let solid = xtrude_core::solids::SolidOp::from_key(op["kind"].as_str().unwrap())
                    .expect("solid op");
                let mut c = SolidOperationCommand::new(solid, gs);
                exec(&mut s, &mut h, &mut c, kind);
            }
            "group" => {
                let m = s.mesh();
                let mut c = MakeGroupCommand::new(
                    m,
                    &m.face_ids(),
                    &m.edge_ids(),
                    op.get("component")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    op["name"].as_str(),
                );
                exec(&mut s, &mut h, &mut c, kind);
            }
            "copy_group" => {
                let mut g = named(&s, op["name"].as_str().unwrap())[0].clone();
                let copy = copy_group(&mut s.doc.meshes, &mut g, Some(p3(&op["delta"])), false);
                exec(&mut s, &mut h, &mut InsertGroupCommand::new(copy), kind);
            }
            "move_group" => {
                let idx = op.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let g = named(&s, op["name"].as_str().unwrap())[idx].id;
                exec(
                    &mut s,
                    &mut h,
                    &mut MoveGroupCommand {
                        group: g,
                        delta: p3(&op["delta"]),
                    },
                    kind,
                );
            }
            "paint" => {
                let f = face_at(s.mesh(), p3(&op["face"]));
                let c = p3(&op["color"]);
                exec(
                    &mut s,
                    &mut h,
                    &mut SetFaceColorCommand {
                        faces: vec![(mid, f)],
                        color: Some([c.x, c.y, c.z]),
                    },
                    kind,
                );
            }
            "erase" => {
                let none = Vec::new();
                let es: Vec<EdgeId> = op
                    .get("edges")
                    .and_then(Value::as_array)
                    .unwrap_or(&none)
                    .iter()
                    .map(|p| edge_at(s.mesh(), p3(p)))
                    .collect();
                let fs: Vec<FaceId> = op
                    .get("faces")
                    .and_then(Value::as_array)
                    .unwrap_or(&none)
                    .iter()
                    .map(|p| face_at(s.mesh(), p3(p)))
                    .collect();
                let mut c = EraseSelectionCommand::from_entities(s.mesh(), &es, &fs);
                exec(&mut s, &mut h, &mut c, kind);
            }
            "intersect" => {
                s.selection.clear();
                for n in op["groups"].as_array().unwrap() {
                    for g in named(&s, n.as_str().unwrap()) {
                        s.selection.insert(EntityRef::Group(g.id));
                    }
                }
                let segs = segments_for(&s, IntersectMode::Model);
                s.selection.clear();
                let mut c = build_add_edges(&s, &segs, true, vec![], None);
                exec(&mut s, &mut h, &mut *c, kind);
            }
            "divide" => {
                let e = edge_at(s.mesh(), p3(&op["edge"]));
                let n = op["n"].as_u64().unwrap() as usize;
                let mut c = FnCommand::new("SnapshotMutation", move |sc: &mut Scene| {
                    divide_edges(sc.mesh_mut(), &[e], n);
                    Ok(())
                });
                exec(&mut s, &mut h, &mut c, kind);
            }
            "flip" => {
                let m = s.mesh();
                let pos: Vec<DVec3> = m.verts.iter().map(|(_, v)| v.position).collect();
                let mut c = FlipVerticesCommand {
                    src: pos,
                    centre: p3(&op["centre"]),
                    axis: p3(&op["axis"]),
                    faces: m.face_ids(),
                };
                exec(&mut s, &mut h, &mut c, kind);
            }
            other => panic!("unknown op {other}"),
        }
    }
    s
}

// ---- Comparison ---------------------------------------------------------------

/// A face as vertex indices: outer loop, holes and paint colour.
type CanonFace = (Vec<usize>, Vec<Vec<usize>>, Option<[i64; 3]>);

/// A mesh as tolerance-matched vertex indices.
struct Canon {
    faces: Vec<CanonFace>,
    edges: Vec<(usize, usize, bool, bool, bool)>,
}

fn index_of(pool: &mut Vec<DVec3>, p: DVec3) -> usize {
    if let Some(i) = pool.iter().position(|q| (*q - p).length() <= TOL) {
        return i;
    }
    pool.push(p);
    pool.len() - 1
}

fn rotate_min(lp: Vec<usize>) -> Vec<usize> {
    if lp.is_empty() {
        return lp;
    }
    let k = (0..lp.len()).min_by_key(|&i| lp[i]).unwrap_or(0);
    let mut out = lp[k..].to_vec();
    out.extend_from_slice(&lp[..k]);
    out
}

fn color_key(a: &FaceAttrs) -> Option<[i64; 3]> {
    a.paint.color.map(|c| c.map(|x| (x * 1e6).round() as i64))
}

fn canon(m: &Mesh, pool: &mut Vec<DVec3>) -> Canon {
    let mut faces: Vec<_> = m
        .face_ids()
        .into_iter()
        .map(|f| {
            let outer = rotate_min(
                m.face(f)
                    .outer
                    .iter()
                    .map(|&v| index_of(pool, m.pos(v)))
                    .collect(),
            );
            let mut holes: Vec<Vec<usize>> = m
                .face(f)
                .holes
                .iter()
                .map(|h| rotate_min(h.iter().map(|&v| index_of(pool, m.pos(v))).collect()))
                .collect();
            holes.sort();
            (outer, holes, color_key(&m.face(f).attrs))
        })
        .collect();
    faces.sort();
    let mut edges: Vec<_> = m
        .edge_ids()
        .into_iter()
        .map(|e| {
            let (a, b) = m.edge_ab(e);
            let (i, j) = (index_of(pool, a), index_of(pool, b));
            let ed = m.edge(e);
            (i.min(j), i.max(j), ed.soft, ed.hidden, ed.curve.is_some())
        })
        .collect();
    edges.sort();
    Canon { faces, edges }
}

/// The objects of a scene to compare: the loose mesh, then each top-level
/// group's world mesh with its name and kind.
fn objects(s: &Scene) -> Vec<(String, bool, bool, Mesh)> {
    let mut out = vec![("<loose>".to_string(), false, false, s.loose_mesh().clone())];
    for g in &s.doc.groups {
        out.push((
            g.name.clone(),
            g.is_instance(),
            g.is_component(),
            world_mesh(&s.doc.meshes, g),
        ));
    }
    out
}

fn compare_scenes(name: &str, got: &Scene, golden: &Scene) -> Result<(), String> {
    let (a, b) = (objects(got), objects(golden));
    if a.len() != b.len() {
        return Err(format!(
            "{name}: {} objects vs the golden's {}",
            a.len(),
            b.len()
        ));
    }
    let mut pool: Vec<DVec3> = Vec::new();
    for ((na, ia, ca, ma), (nb, ib, cb, mb)) in a.iter().zip(&b) {
        if (na, ia, ca) != (nb, ib, cb) {
            return Err(format!(
                "{name}: object {na:?} instance={ia} component={ca} vs golden {nb:?} {ib} {cb}"
            ));
        }
        // The golden's positions first, so the shared pool indexes are its own
        let cb_ = canon(mb, &mut pool);
        let ca_ = canon(ma, &mut pool);
        if ca_.faces.len() != cb_.faces.len() || ca_.edges.len() != cb_.edges.len() {
            return Err(format!(
                "{name}/{na}: {} faces {} edges vs the golden's {} faces {} edges",
                ca_.faces.len(),
                ca_.edges.len(),
                cb_.faces.len(),
                cb_.edges.len()
            ));
        }
        if ca_.faces != cb_.faces {
            let first = ca_.faces.iter().zip(&cb_.faces).find(|(x, y)| x != y);
            return Err(format!("{name}/{na}: faces differ, first {first:?}"));
        }
        if ca_.edges != cb_.edges {
            let first = ca_.edges.iter().zip(&cb_.edges).find(|(x, y)| x != y);
            return Err(format!("{name}/{na}: edges differ, first {first:?}"));
        }
    }
    Ok(())
}

fn recipes() -> Vec<Value> {
    let text = std::fs::read_to_string(golden("recipes.json")).expect("recipes.json");
    let v: Value = serde_json::from_str(&text).expect("json");
    v["recipes"].as_array().unwrap().clone()
}

fn load_golden(name: &str) -> Scene {
    let mut s = Scene::new();
    igz::load_into(&mut s, &golden(&format!("{name}.igz")), None).expect("golden");
    s
}

fn recipe_name(r: &Value) -> String {
    r["name"].as_str().expect("a recipe name").to_string()
}

/// Each recipe whose replay differs from its reference document.
fn replay_failures() -> Vec<String> {
    let mut failures = Vec::new();
    for r in recipes() {
        let name = recipe_name(&r);
        let got = replay(r["ops"].as_array().unwrap());
        let golden = load_golden(&name);
        if let Err(e) = compare_scenes(&name, &got, &golden) {
            failures.push(e);
        }
    }
    failures
}

/// Replay every recipe and write its reference document, STL (mm) and
/// OBJ/MTL. Every recipe is replayed before anything is written, so a
/// recipe that fails leaves the references as they were. The exports are
/// made from the document as saved, as the re-export test makes them.
fn write_references() {
    let mut scenes: Vec<(String, Scene)> = recipes()
        .iter()
        .map(|r| {
            let name = recipe_name(r);
            let ops = r["ops"].as_array().unwrap();
            let scene = std::panic::catch_unwind(AssertUnwindSafe(|| replay(ops)))
                .unwrap_or_else(|_| panic!("recipe {name} does not replay: no reference written"));
            (name, scene)
        })
        .collect();
    for (name, scene) in &mut scenes {
        let mut next = 0;
        number_uids(&mut scene.doc.groups, &mut next);
        igz::save_scene(scene, &golden(&format!("{name}.igz")))
            .unwrap_or_else(|e| panic!("{name}.igz: {e}"));
        let saved = load_golden(name);
        stl::save_stl(&saved, &golden(&format!("{name}.stl")), "mm")
            .unwrap_or_else(|e| panic!("{name}.stl: {e}"));
        obj::save_obj(&saved, &golden(&format!("{name}.obj")))
            .unwrap_or_else(|e| panic!("{name}.obj: {e}"));
        eprintln!("wrote tests/golden/{name}.igz .stl .obj .mtl");
    }
}

/// Number the group uids in tree order: a new group's uid is random, and
/// regenerating an unchanged model must rewrite the same file.
fn number_uids(groups: &mut [Group], next: &mut u64) {
    for g in groups {
        *next += 1;
        g.uid = format!("{:016x}", *next);
        number_uids(&mut g.children, next);
    }
}

/// The engine reproduces every golden model.
#[test]
fn test_replays_every_golden_recipe() {
    if updating() {
        write_references();
        return;
    }
    let failures = replay_failures();
    assert!(
        failures.is_empty(),
        "{} of {} recipes differ:\n{}",
        failures.len(),
        recipes().len(),
        failures.join("\n")
    );
}

// ---- Re-exports of the golden documents ----------------------------------------

type Tri = [[f64; 3]; 3];

fn read_stl(path: &Path) -> Vec<Tri> {
    let b = std::fs::read(path).unwrap();
    let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
    (0..n)
        .map(|i| {
            let r = &b[84 + 50 * i..84 + 50 * (i + 1)];
            let f = |k: usize| {
                f32::from_le_bytes([r[4 * k], r[4 * k + 1], r[4 * k + 2], r[4 * k + 3]]) as f64
            };
            [[f(3), f(4), f(5)], [f(6), f(7), f(8)], [f(9), f(10), f(11)]]
        })
        .collect()
}

fn read_obj(path: &Path) -> Vec<Tri> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut v: Vec<[f64; 3]> = Vec::new();
    let mut tris = Vec::new();
    for line in text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        match p.first() {
            Some(&"v") => v.push([
                p[1].parse().unwrap(),
                p[2].parse().unwrap(),
                p[3].parse().unwrap(),
            ]),
            Some(&"f") => {
                let i: Vec<usize> = p[1..]
                    .iter()
                    .map(|t| t.split('/').next().unwrap().parse::<usize>().unwrap() - 1)
                    .collect();
                tris.push([v[i[0]], v[i[1]], v[i[2]]]);
            }
            _ => {}
        }
    }
    tris
}

/// Triangle soups equal up to order, triangle rotation and `tol`.
fn same_soup(a: &[Tri], b: &[Tri], tol: f64) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("{} triangles vs {}", a.len(), b.len()));
    }
    let close = |p: &[f64; 3], q: &[f64; 3]| (0..3).all(|k| (p[k] - q[k]).abs() <= tol);
    let same_tri = |x: &Tri, y: &Tri| (0..3).any(|r| (0..3).all(|k| close(&x[k], &y[(k + r) % 3])));
    let mut used = vec![false; b.len()];
    for (n, t) in a.iter().enumerate() {
        match (0..b.len()).find(|&j| !used[j] && same_tri(t, &b[j])) {
            Some(j) => used[j] = true,
            None => return Err(format!("triangle {n} {t:?} has no match")),
        }
    }
    Ok(())
}

/// Each reference document whose STL, OBJ or `.igz` re-export differs.
fn reexport_failures() -> Vec<String> {
    let tmp = TmpDir::new();
    let mut failures = Vec::new();
    for r in recipes() {
        let name = recipe_name(&r);
        let s = load_golden(&name);
        let out_stl = tmp.join(&format!("{name}.stl"));
        stl::save_stl(&s, &out_stl, "mm").unwrap();
        if let Err(e) = same_soup(
            &read_stl(&out_stl),
            &read_stl(&golden(&format!("{name}.stl"))),
            1e-3,
        ) {
            failures.push(format!("{name}.stl: {e}"));
        }
        let out_obj = tmp.join(&format!("{name}.obj"));
        obj::save_obj(&s, &out_obj).unwrap();
        if let Err(e) = same_soup(
            &read_obj(&out_obj),
            &read_obj(&golden(&format!("{name}.obj"))),
            1e-5,
        ) {
            failures.push(format!("{name}.obj: {e}"));
        }
        let out_igz = tmp.join(&format!("{name}.igz"));
        igz::save_scene(&s, &out_igz).unwrap();
        let mut back = Scene::new();
        igz::load_into(&mut back, &out_igz, None).unwrap();
        if let Err(e) = compare_scenes(&name, &back, &s) {
            failures.push(format!("{name}.igz round trip: {e}"));
        }
    }
    failures
}

/// Opening a reference document and exporting it gives the reference STL
/// and OBJ.
#[test]
fn test_goldens_reexport_identically() {
    if updating() {
        // The replay test writes the exports too: comparing here would race it.
        eprintln!("XTRUDE_UPDATE_GOLDEN=1: the replay test writes the exports; nothing compared");
        return;
    }
    let failures = reexport_failures();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- The reference set --------------------------------------------------------

/// Every recipe has its four reference files, and every file in the
/// folder belongs to a recipe.
#[test]
fn test_golden_files_match_the_recipes() {
    let mut expected: BTreeSet<String> = BTreeSet::from(["recipes.json".to_string()]);
    for r in recipes() {
        let name = recipe_name(&r);
        for ext in ["igz", "stl", "obj", "mtl"] {
            expected.insert(format!("{name}.{ext}"));
        }
    }
    let found: BTreeSet<String> = std::fs::read_dir(golden_dir())
        .expect("tests/golden")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(found, expected);
}

/// Each file in the golden folder with its size and modification time.
fn golden_stamps() -> Vec<(String, u64, SystemTime)> {
    let mut stamps: Vec<_> = std::fs::read_dir(golden_dir())
        .expect("tests/golden")
        .map(|e| {
            let e = e.expect("an entry");
            let meta = e.metadata().expect("metadata");
            (
                e.file_name().to_string_lossy().into_owned(),
                meta.len(),
                meta.modified().expect("a modification time"),
            )
        })
        .collect();
    stamps.sort();
    stamps
}

/// Without `XTRUDE_UPDATE_GOLDEN=1`, comparing leaves the references
/// untouched: updating them is opt-in.
#[test]
fn test_comparing_writes_no_reference() {
    if updating() {
        return;
    }
    let before = golden_stamps();
    let _ = replay_failures();
    let _ = reexport_failures();
    assert_eq!(golden_stamps(), before);
}
