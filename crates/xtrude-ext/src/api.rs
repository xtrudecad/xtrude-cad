// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Lua API, version 1: the `xtrude` table, vectors, and the `doc` a
//! script edits, with its face, edge and group handles.
//!
//! Lengths cross the boundary in the document's model units (millimetres
//! by default) and are metres inside. `doc` is the live [`Scene`] for the
//! length of one run (a scoped userdata); handles keep only ids, reach the
//! scene through the registry, and check on every use that what they name
//! still exists, since push/pull and stitching replace faces.

use std::collections::HashSet;
use std::path::PathBuf;

use mlua::{
    AnyUserData, FromLua, Function, Lua, LuaSerdeExt, MetaMethod, MultiValue, Table, UserData,
    UserDataFields, UserDataMethods, UserDataRegistry, Value,
};
use xtrude_core::attrs::{Paint, Rgb};
use xtrude_core::math::{DVec3, VecExt};
use xtrude_core::mesh::{EdgeId, FaceId, Key, Mesh};
use xtrude_core::topology::tkey;
use xtrude_io::{obj, stl};
use xtrude_model::commands::{
    find_edge_at, AddEdgeCommand, AddFaceCommand, GroupToComponentCommand, MakeGroupCommand,
    MoveGroupCommand, RotateGroupCommand, ScaleFactor, ScaleGroupCommand, SetFaceColorCommand,
    SetGroupMaterialCommand, SetPluginDataCommand,
};
use xtrude_model::i18n::tr;
use xtrude_model::pushpull::{push_face_in_scene, PushOutcome, PushPull};
use xtrude_model::{units, Command, EntityRef, GroupId, MeshId, Scene};

use mlua::serde::{DeserializeOptions, SerializeOptions};

/// The API version this build implements (`xtrude.api`).
pub const API_VERSION: i64 = 1;

/// Where the running script's `doc` is kept, for the handles' methods.
const DOC_KEY: &str = "xtrude.doc";

fn fail(message: String) -> mlua::Error {
    mlua::Error::RuntimeError(message)
}

/// Metres per model unit of `scene`.
fn scale_of(scene: &Scene) -> f64 {
    units::bare_scale_of(&scene.units.length).unwrap_or(1.0)
}

/// Runs `f` on the scene of the script being run.
fn with_scene<R>(lua: &Lua, f: impl FnOnce(&mut Scene) -> mlua::Result<R>) -> mlua::Result<R> {
    let doc: Option<AnyUserData> = lua.named_registry_value(DOC_KEY)?;
    let doc = doc.ok_or_else(|| fail(tr("no document: the script is not running", &[])))?;
    doc.borrow_mut_scoped::<Scene, _>(f)?
}

/// Where the `xtrude` table is kept, for the run to reach it even when a
/// script has replaced the global.
const XTRUDE_KEY: &str = "xtrude.table";

/// Asks the user where to save: `(title, filter)` → the path chosen, or
/// `None` when cancelled.
pub type SaveDialog<'a> = dyn FnMut(&str, &str) -> Option<PathBuf> + 'a;

/// What a run reaches outside the document.
pub(crate) struct Host<'a, 'd> {
    /// The extension whose `plugin_data` key `doc:data()` reads; empty for
    /// a one-off script, which keeps no data.
    pub extension: &'a str,
    pub save_dialog: Option<&'a mut SaveDialog<'d>>,
}

/// The state of the run in progress.
struct RunState {
    extension: String,
    /// The paths the user chose in this run (`xtrude.ask_save_path`): the
    /// only ones the exports write to.
    chosen: HashSet<PathBuf>,
}

/// Calls `func(doc, params)` with `doc` borrowing `scene` for the call.
pub(crate) fn call_with_doc(
    lua: &Lua,
    scene: &mut Scene,
    func: &Function,
    params: Value,
    host: Host<'_, '_>,
) -> mlua::Result<()> {
    let xtrude: Table = lua.named_registry_value(XTRUDE_KEY)?;
    let Host {
        extension,
        save_dialog,
    } = host;
    lua.set_app_data(RunState {
        extension: extension.to_string(),
        chosen: HashSet::new(),
    });
    let out = lua.scope(|scope| {
        if let Some(ask) = save_dialog {
            let ask = scope.create_function_mut(
                move |lua, (title, filter): (String, Option<String>)| {
                    let Some(path) = ask(&title, filter.as_deref().unwrap_or("")) else {
                        return Ok(None);
                    };
                    if let Some(mut run) = lua.app_data_mut::<RunState>() {
                        run.chosen.insert(path.clone());
                    }
                    Ok(Some(path.to_string_lossy().into_owned()))
                },
            )?;
            xtrude.raw_set("ask_save_path", ask)?;
        }
        let doc = scope.create_any_userdata_ref_mut(scene)?;
        lua.set_named_registry_value(DOC_KEY, &doc)?;
        let out = func.call::<()>((doc, params));
        lua.unset_named_registry_value(DOC_KEY)?;
        out
    });
    lua.remove_app_data::<RunState>();
    xtrude.raw_set("ask_save_path", no_save_dialog(lua)?)?;
    out
}

/// `xtrude.ask_save_path` with nobody to ask: always cancelled.
fn no_save_dialog(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|_, _: MultiValue| Ok(Value::Nil))
}

/// A path `xtrude.ask_save_path` gave in this run, else an error.
fn chosen_path(lua: &Lua, path: Option<String>) -> mlua::Result<PathBuf> {
    let Some(path) = path else {
        return Err(fail(tr(
            "no path to write to: the save dialog was cancelled",
            &[],
        )));
    };
    let path = PathBuf::from(path);
    let chosen = lua
        .app_data_ref::<RunState>()
        .is_some_and(|run| run.chosen.contains(&path));
    if chosen {
        Ok(path)
    } else {
        Err(fail(tr(
            "a script writes only to a path the user chose with xtrude.ask_save_path",
            &[],
        )))
    }
}

/// The running extension's id, for its document data.
fn data_key(lua: &Lua) -> mlua::Result<String> {
    let key = lua
        .app_data_ref::<RunState>()
        .map(|run| run.extension.clone())
        .unwrap_or_default();
    if key.is_empty() {
        return Err(fail(tr(
            "only an extension keeps data in the document",
            &[],
        )));
    }
    Ok(key)
}

/// Installs the `xtrude` table and the `doc` methods.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let xtrude = lua.create_table()?;
    xtrude.set(
        "vec",
        lua.create_function(|_, (x, y, z): (f64, f64, Option<f64>)| {
            Ok(LuaVec(DVec3::new(x, y, z.unwrap_or(0.0))))
        })?,
    )?;
    xtrude.set(
        "unit",
        lua.create_function(|lua, ()| {
            let doc: Option<AnyUserData> = lua.named_registry_value(DOC_KEY)?;
            match doc {
                Some(_) => with_scene(lua, |scene| Ok(scene.units.length.clone())),
                None => Ok(units::model_unit()),
            }
        })?,
    )?;
    xtrude.set(
        "version",
        lua.create_function(|_, ()| Ok(xtrude_model::VERSION))?,
    )?;
    xtrude.set("api", API_VERSION)?;
    xtrude.set("ask_save_path", no_save_dialog(lua)?)?;
    lua.set_named_registry_value(XTRUDE_KEY, &xtrude)?;
    lua.globals().set("xtrude", xtrude)?;
    lua.register_userdata_type::<Scene>(register_doc)
}

// ---- Vectors -------------------------------------------------------------------

/// `xtrude.vec(x, y, z)`: a value; points are vectors in model units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LuaVec(pub DVec3);

impl FromLua for LuaVec {
    /// A vector, or a table `{x, y[, z]}` / `{x = …, y = …, z = …}`.
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<LuaVec> {
        match &value {
            Value::UserData(ud) => {
                if let Ok(v) = ud.borrow::<LuaVec>() {
                    return Ok(*v);
                }
            }
            Value::Table(t) => {
                let named = (t.get::<Option<f64>>("x")?, t.get::<Option<f64>>("y")?);
                let (x, y, z) = match named {
                    (Some(x), Some(y)) => (x, y, t.get::<Option<f64>>("z")?),
                    _ => (t.get::<f64>(1)?, t.get::<f64>(2)?, t.get::<Option<f64>>(3)?),
                };
                return Ok(LuaVec(DVec3::new(x, y, z.unwrap_or(0.0))));
            }
            _ => {}
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "vec".into(),
            message: Some(tr("expected a vector or a table {x, y, z}", &[])),
        })
    }
}

impl UserData for LuaVec {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("x", |_, v| Ok(v.0.x));
        fields.add_field_method_get("y", |_, v| Ok(v.0.y));
        fields.add_field_method_get("z", |_, v| Ok(v.0.z));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("dot", |_, v, w: LuaVec| Ok(v.0.dot(w.0)));
        methods.add_method("cross", |_, v, w: LuaVec| Ok(LuaVec(v.0.cross(w.0))));
        methods.add_method("length", |_, v, ()| Ok(v.0.length()));
        methods.add_method("unit", |_, v, ()| Ok(LuaVec(v.0.unit_or_zero())));
        methods.add_meta_function(MetaMethod::Add, |_, (a, b): (LuaVec, LuaVec)| {
            Ok(LuaVec(a.0 + b.0))
        });
        methods.add_meta_function(MetaMethod::Sub, |_, (a, b): (LuaVec, LuaVec)| {
            Ok(LuaVec(a.0 - b.0))
        });
        methods.add_meta_method(MetaMethod::Unm, |_, v, ()| Ok(LuaVec(-v.0)));
        methods.add_meta_function(MetaMethod::Mul, |lua, (a, b): (Value, Value)| {
            scale_vec(lua, a, b)
        });
        methods.add_meta_function(MetaMethod::Div, |_, (a, k): (LuaVec, f64)| {
            Ok(LuaVec(a.0 / k))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (LuaVec, LuaVec)| Ok(a.0 == b.0));
        methods.add_meta_method(MetaMethod::ToString, |_, v, ()| {
            Ok(format!("vec({}, {}, {})", v.0.x, v.0.y, v.0.z))
        });
    }
}

/// `v * k` or `k * v`.
fn scale_vec(lua: &Lua, a: Value, b: Value) -> mlua::Result<LuaVec> {
    match (f64::from_lua(a.clone(), lua), f64::from_lua(b.clone(), lua)) {
        (Ok(k), _) => Ok(LuaVec(LuaVec::from_lua(b, lua)?.0 * k)),
        (_, Ok(k)) => Ok(LuaVec(LuaVec::from_lua(a, lua)?.0 * k)),
        _ => Err(fail(tr("a vector multiplies a number", &[]))),
    }
}

// ---- Handles -------------------------------------------------------------------

/// A face of one mesh (the loose mesh or a group's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceRef {
    pub mesh: MeshId,
    pub face: FaceId,
}

/// An edge of one mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeRef {
    pub mesh: MeshId,
    pub edge: EdgeId,
}

/// A group or component instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupRef(pub GroupId);

fn no_face() -> mlua::Error {
    fail(tr("face no longer exists", &[]))
}

fn no_edge() -> mlua::Error {
    fail(tr("edge no longer exists", &[]))
}

fn no_group() -> mlua::Error {
    fail(tr("group no longer exists", &[]))
}

impl FaceRef {
    /// The face's mesh, if the face still exists.
    fn mesh<'s>(&self, scene: &'s Scene) -> mlua::Result<&'s Mesh> {
        let alive = scene.doc.meshes.contains_key(&self.mesh)
            && scene.doc.mesh(self.mesh).has_face(self.face);
        alive.then(|| scene.doc.mesh(self.mesh)).ok_or_else(no_face)
    }
}

impl EdgeRef {
    fn mesh<'s>(&self, scene: &'s Scene) -> mlua::Result<&'s Mesh> {
        let alive = scene.doc.meshes.contains_key(&self.mesh)
            && scene.doc.mesh(self.mesh).has_edge(self.edge);
        alive.then(|| scene.doc.mesh(self.mesh)).ok_or_else(no_edge)
    }
}

impl GroupRef {
    fn mesh(&self, scene: &Scene) -> mlua::Result<MeshId> {
        scene
            .doc
            .find_group(self.0)
            .map(|g| g.mesh)
            .ok_or_else(no_group)
    }
}

macro_rules! handle_from_lua {
    ($t:ty, $name:literal) => {
        impl FromLua for $t {
            fn from_lua(value: Value, _: &Lua) -> mlua::Result<$t> {
                if let Value::UserData(ud) = &value {
                    if let Ok(h) = ud.borrow::<$t>() {
                        return Ok(*h);
                    }
                }
                Err(mlua::Error::FromLuaConversionError {
                    from: value.type_name(),
                    to: $name.into(),
                    message: None,
                })
            }
        }
    };
}

handle_from_lua!(FaceRef, "face");
handle_from_lua!(EdgeRef, "edge");
handle_from_lua!(GroupRef, "group");

fn positions_out(points: impl IntoIterator<Item = DVec3>, scale: f64) -> Vec<LuaVec> {
    points.into_iter().map(|p| LuaVec(p / scale)).collect()
}

impl UserData for FaceRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("vertices", |lua, h, ()| {
            with_scene(lua, |scene| {
                let pts = h.mesh(scene)?.face_positions(h.face);
                Ok(positions_out(pts, scale_of(scene)))
            })
        });
        methods.add_method("normal", |lua, h, ()| {
            with_scene(lua, |scene| Ok(LuaVec(h.mesh(scene)?.face_normal(h.face))))
        });
        methods.add_method("area", |lua, h, ()| {
            with_scene(lua, |scene| {
                let s = scale_of(scene);
                Ok(h.mesh(scene)?.face_area(h.face) / (s * s))
            })
        });
        methods.add_method("exists", |lua, h, ()| {
            with_scene(lua, |scene| Ok(h.mesh(scene).is_ok()))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (FaceRef, FaceRef)| Ok(a == b));
        methods.add_meta_method(MetaMethod::ToString, |_, h, ()| {
            Ok(format!("face {:?}", h.face))
        });
    }
}

impl UserData for EdgeRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("vertices", |lua, h, ()| {
            with_scene(lua, |scene| {
                let (a, b) = h.mesh(scene)?.edge_ab(h.edge);
                Ok(positions_out([a, b], scale_of(scene)))
            })
        });
        methods.add_method("exists", |lua, h, ()| {
            with_scene(lua, |scene| Ok(h.mesh(scene).is_ok()))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (EdgeRef, EdgeRef)| Ok(a == b));
        methods.add_meta_method(MetaMethod::ToString, |_, h, ()| {
            Ok(format!("edge {:?}", h.edge))
        });
    }
}

impl UserData for GroupRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("name", |lua, h, ()| {
            with_scene(lua, |scene| {
                let g = scene.doc.find_group(h.0).ok_or_else(no_group)?;
                Ok(g.name.clone())
            })
        });
        methods.add_method("faces", |lua, h, ()| {
            with_scene(lua, |scene| {
                let mesh = h.mesh(scene)?;
                Ok(scene
                    .doc
                    .mesh(mesh)
                    .face_ids()
                    .into_iter()
                    .map(|face| FaceRef { mesh, face })
                    .collect::<Vec<_>>())
            })
        });
        methods.add_method("move", |lua, h, v: LuaVec| {
            with_scene(lua, |scene| {
                h.mesh(scene)?;
                let delta = v.0 * scale_of(scene);
                apply(scene, &mut MoveGroupCommand { group: h.0, delta })
            })
        });
        methods.add_method(
            "rotate",
            |lua, h, (point, axis, degrees): (LuaVec, LuaVec, f64)| {
                with_scene(lua, |scene| {
                    h.mesh(scene)?;
                    let axis = axis.0.unit_or_zero();
                    if axis == DVec3::ZERO || !degrees.is_finite() {
                        return Err(fail(tr("rotate needs an axis and an angle", &[])));
                    }
                    let center = point.0 * scale_of(scene);
                    apply(
                        scene,
                        &mut RotateGroupCommand {
                            group: h.0,
                            center,
                            axis,
                            degrees,
                        },
                    )
                })
            },
        );
        methods.add_method("scale", |lua, h, (origin, factors): (LuaVec, Value)| {
            let factor = match factors {
                Value::Number(_) | Value::Integer(_) => {
                    ScaleFactor::Uniform(f64::from_lua(factors, lua)?)
                }
                other => {
                    let v = LuaVec::from_lua(other, lua)?.0;
                    ScaleFactor::PerAxis([v.x, v.y, v.z])
                }
            };
            let ok = match factor {
                ScaleFactor::Uniform(k) => k.is_finite() && k != 0.0,
                ScaleFactor::PerAxis(k) => k.iter().all(|k| k.is_finite() && *k != 0.0),
            };
            if !ok {
                return Err(fail(tr("scale factors must be non-zero numbers", &[])));
            }
            with_scene(lua, |scene| {
                h.mesh(scene)?;
                let center = origin.0 * scale_of(scene);
                apply(
                    scene,
                    &mut ScaleGroupCommand {
                        group: h.0,
                        center,
                        factor,
                        axes: None,
                    },
                )
            })
        });
        methods.add_method("exists", |lua, h, ()| {
            with_scene(lua, |scene| Ok(h.mesh(scene).is_ok()))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (GroupRef, GroupRef)| Ok(a == b));
        methods.add_meta_method(MetaMethod::ToString, |_, h, ()| {
            Ok(format!("group {:?}", h.0))
        });
    }
}

// ---- The document ----------------------------------------------------------------

/// Applies `cmd` inside the running script's command (no history step).
fn apply(scene: &mut Scene, cmd: &mut dyn Command) -> mlua::Result<()> {
    cmd.apply(scene).map_err(|e| fail(e.to_string()))
}

/// A list of points in model units, as metres; at least `min` of them.
fn points_in(points: Vec<LuaVec>, scale: f64, min: usize) -> mlua::Result<Vec<DVec3>> {
    if points.len() < min || points.iter().any(|p| !p.0.is_finite()) {
        return Err(fail(tr(
            "expected at least {n} points with finite coordinates",
            &[("n", &min)],
        )));
    }
    Ok(points.into_iter().map(|p| p.0 * scale).collect())
}

/// The loop's Newell normal is not zero: the points span an area.
fn spans_area(pts: &[DVec3]) -> bool {
    let n = (0..pts.len()).fold(DVec3::ZERO, |n, i| {
        n + pts[i].cross(pts[(i + 1) % pts.len()])
    });
    n.length() > 1e-12
}

/// What a list of handles names, split by kind.
#[derive(Default)]
struct Entities {
    faces: Vec<FaceRef>,
    edges: Vec<EdgeRef>,
    groups: Vec<GroupRef>,
}

fn entities_in(lua: &Lua, list: Vec<Value>) -> mlua::Result<Entities> {
    let mut out = Entities::default();
    for v in list {
        if let Ok(f) = FaceRef::from_lua(v.clone(), lua) {
            out.faces.push(f);
        } else if let Ok(e) = EdgeRef::from_lua(v.clone(), lua) {
            out.edges.push(e);
        } else {
            out.groups.push(
                GroupRef::from_lua(v, lua)
                    .map_err(|_| fail(tr("expected a list of faces, edges and groups", &[])))?,
            );
        }
    }
    Ok(out)
}

/// `#rrggbb` (or `rrggbb`) as an RGB colour in 0..1.
fn parse_hex(text: &str) -> Option<Rgb> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([
        f64::from(ch(0)?) / 255.0,
        f64::from(ch(2)?) / 255.0,
        f64::from(ch(4)?) / 255.0,
    ])
}

/// The face a push left at the far end: facing the push, and holding the
/// most of the base's corners moved by the push (the first one on a tie).
fn find_cap(mesh: &Mesh, pp: &PushPull) -> Option<FaceId> {
    let n = pp.normal?.unit_or_zero();
    let keys: HashSet<Key> = pp
        .cap_positions
        .iter()
        .map(|&p| tkey(p + n * pp.extrusion))
        .collect();
    let mut best: Option<(usize, FaceId)> = None;
    for f in mesh.face_ids() {
        if mesh.face_normal(f).dot(n) < 0.999 {
            continue;
        }
        let mut hits = mesh
            .face_positions(f)
            .iter()
            .filter(|&&p| keys.contains(&tkey(p)))
            .count();
        for hole in mesh.face_hole_positions(f) {
            hits += hole.iter().filter(|&&p| keys.contains(&tkey(p))).count();
        }
        if hits > 0 && best.is_none_or(|(b, _)| hits > b) {
            best = Some((hits, f));
        }
    }
    best.map(|(_, f)| f)
}

/// Selection entries as handles (faces, edges, groups).
fn handle_of(lua: &Lua, e: &EntityRef) -> mlua::Result<Option<AnyUserData>> {
    Ok(match *e {
        EntityRef::Face(mesh, face) => Some(lua.create_userdata(FaceRef { mesh, face })?),
        EntityRef::Edge(mesh, edge) => Some(lua.create_userdata(EdgeRef { mesh, edge })?),
        EntityRef::Group(g) => Some(lua.create_userdata(GroupRef(g))?),
        _ => None,
    })
}

/// Optional table field.
fn opt<T: FromLua>(opts: &Option<mlua::Table>, key: &str) -> mlua::Result<Option<T>> {
    match opts {
        Some(t) => t.get(key),
        None => Ok(None),
    }
}

fn register_doc(reg: &mut UserDataRegistry<Scene>) {
    reg.add_method_mut(
        "add_face",
        |_, scene, (points, opts): (Vec<LuaVec>, Option<mlua::Table>)| {
            let s = scale_of(scene);
            let outer = points_in(points, s, 3)?;
            if !spans_area(&outer) {
                return Err(fail(tr("the points do not span a face", &[])));
            }
            let holes: Vec<Vec<LuaVec>> = opt(&opts, "holes")?.unwrap_or_default();
            let holes = holes
                .into_iter()
                .map(|h| points_in(h, s, 3))
                .collect::<mlua::Result<Vec<_>>>()?;
            let mut cmd = AddFaceCommand::new(outer);
            cmd.holes = holes;
            apply(scene, &mut cmd)?;
            let mesh = scene.active_mesh_id();
            let face = cmd.face.ok_or_else(no_face)?;
            Ok(FaceRef { mesh, face })
        },
    );
    reg.add_method_mut("add_edge", |_, scene, (a, b): (LuaVec, LuaVec)| {
        let s = scale_of(scene);
        let pts = points_in(vec![a, b], s, 2)?;
        if (pts[0] - pts[1]).length() < 1e-9 {
            return Err(fail(tr("an edge needs two different points", &[])));
        }
        let mut cmd = AddEdgeCommand::new(pts[0], pts[1]);
        apply(scene, &mut cmd)?;
        let mesh = scene.active_mesh_id();
        let edge = cmd
            .edge
            .or_else(|| find_edge_at(scene.doc.mesh(mesh), pts[0], pts[1]))
            .ok_or_else(no_edge)?;
        Ok(EdgeRef { mesh, edge })
    });
    reg.add_method_mut(
        "push_pull",
        |_, scene, (face, distance, opts): (FaceRef, f64, Option<mlua::Table>)| {
            face.mesh(scene)?;
            if !distance.is_finite() {
                return Err(fail(tr("the distance must be a number", &[])));
            }
            let keep_base = opt::<bool>(&opts, "keep_base")?.unwrap_or(false);
            let dist = distance * scale_of(scene);
            let pushed = push_face_in_scene(scene, face.mesh, face.face, dist, keep_base)
                .map_err(|e| fail(e.to_string()))?;
            let Some(pp) = pushed else {
                return Ok(None);
            };
            if pp.outcome() == PushOutcome::Refused {
                return Err(fail(tr("push/pull refused: it would break the solid", &[])));
            }
            Ok(find_cap(scene.doc.mesh(face.mesh), &pp).map(|f| FaceRef {
                mesh: face.mesh,
                face: f,
            }))
        },
    );
    let make = |component: bool| {
        move |lua: &Lua, scene: &mut Scene, (list, name): (Vec<Value>, Option<String>)| {
            let ents = entities_in(lua, list)?;
            if component && ents.faces.is_empty() && ents.edges.is_empty() {
                if let [g] = ents.groups[..] {
                    g.mesh(scene)?;
                    apply(
                        scene,
                        &mut GroupToComponentCommand {
                            group: g.0,
                            name: name.filter(|n| !n.is_empty()),
                        },
                    )?;
                    return Ok(g);
                }
            }
            if !ents.groups.is_empty() {
                return Err(fail(tr("expected a list of faces and edges", &[])));
            }
            let active = scene.active_mesh_id();
            for f in &ents.faces {
                f.mesh(scene)?;
            }
            for e in &ents.edges {
                e.mesh(scene)?;
            }
            if ents.faces.iter().any(|f| f.mesh != active)
                || ents.edges.iter().any(|e| e.mesh != active)
            {
                return Err(fail(tr(
                    "only geometry of the context being edited can be grouped",
                    &[],
                )));
            }
            if ents.faces.is_empty() && ents.edges.is_empty() {
                return Err(fail(tr("nothing to group", &[])));
            }
            let faces: Vec<FaceId> = ents.faces.iter().map(|f| f.face).collect();
            let edges: Vec<EdgeId> = ents.edges.iter().map(|e| e.edge).collect();
            let mut cmd =
                MakeGroupCommand::new(scene.mesh(), &faces, &edges, component, name.as_deref());
            apply(scene, &mut cmd)?;
            cmd.group.map(GroupRef).ok_or_else(no_group)
        }
    };
    reg.add_method_mut("make_group", make(false));
    reg.add_method_mut("make_component", make(true));
    reg.add_method_mut(
        "set_paint",
        |lua, scene, (target, color): (Value, Option<String>)| {
            let color = match color {
                None => None,
                Some(text) => Some(parse_hex(&text).ok_or_else(|| {
                    fail(tr(
                        "expected a colour as \"#rrggbb\", got \"{text}\"",
                        &[("text", &text)],
                    ))
                })?),
            };
            if let Ok(f) = FaceRef::from_lua(target.clone(), lua) {
                f.mesh(scene)?;
                return apply(
                    scene,
                    &mut SetFaceColorCommand {
                        faces: vec![(f.mesh, f.face)],
                        color,
                    },
                );
            }
            let g = GroupRef::from_lua(target, lua)
                .map_err(|_| fail(tr("set_paint takes a face or a group", &[])))?;
            g.mesh(scene)?;
            let material = color.map(|c| Paint {
                color: Some(c),
                ..Paint::default()
            });
            apply(
                scene,
                &mut SetGroupMaterialCommand {
                    group: g.0,
                    material,
                },
            )
        },
    );
    reg.add_method("selection", |lua, scene, ()| {
        let mut out = Vec::new();
        for e in &scene.selection {
            if let Some(h) = handle_of(lua, e)? {
                out.push(h);
            }
        }
        Ok(out)
    });
    reg.add_method_mut("select", |lua, scene, list: Vec<Value>| {
        let ents = entities_in(lua, list)?;
        let mut sel = Vec::new();
        for f in ents.faces {
            f.mesh(scene)?;
            sel.push(EntityRef::Face(f.mesh, f.face));
        }
        for e in ents.edges {
            e.mesh(scene)?;
            sel.push(EntityRef::Edge(e.mesh, e.edge));
        }
        for g in ents.groups {
            g.mesh(scene)?;
            sel.push(EntityRef::Group(g.0));
        }
        scene.selection.clear();
        scene.selection.extend(sel);
        scene.bump_view();
        Ok(())
    });
    reg.add_method("faces", |_, scene, ()| {
        let mesh = scene.active_mesh_id();
        Ok(scene
            .mesh()
            .face_ids()
            .into_iter()
            .map(|face| FaceRef { mesh, face })
            .collect::<Vec<_>>())
    });
    reg.add_method("data", |lua, scene, ()| {
        let key = data_key(lua)?;
        match scene.doc.plugin_data.get(&key) {
            None => Ok(Value::Nil),
            Some(v) => lua.to_value_with(
                v,
                SerializeOptions::new()
                    .serialize_none_to_null(false)
                    .serialize_unit_to_null(false),
            ),
        }
    });
    reg.add_method_mut("set_data", |lua, scene, data: Value| {
        let key = data_key(lua)?;
        let value = match data {
            Value::Nil => None,
            Value::Table(_) => Some(lua.from_value_with::<serde_json::Value>(
                data,
                DeserializeOptions::new().sort_keys(true),
            )?),
            _ => return Err(fail(tr("set_data takes a table, or nil to remove it", &[]))),
        };
        apply(scene, &mut SetPluginDataCommand { key, value })
    });
    reg.add_method_mut(
        "export_stl",
        |lua, scene, (path, opts): (Option<String>, Option<mlua::Table>)| {
            let path = chosen_path(lua, path)?;
            let selection_only = opt::<bool>(&opts, "selection_only")?.unwrap_or(false);
            let write =
                |scene: &Scene| stl::save_stl(scene, &path, "mm").map_err(|e| fail(e.to_string()));
            if !selection_only {
                return write(scene);
            }
            // only the selection: the document is cut down for the write,
            // then put back as it was
            let whole = scene.doc.clone();
            keep_selection(scene);
            let out = write(scene);
            scene.doc = whole;
            out
        },
    );
    reg.add_method("export_obj", |lua, scene, path: Option<String>| {
        let path = chosen_path(lua, path)?;
        obj::save_obj(scene, &path).map_err(|e| fail(e.to_string()))
    });
    reg.add_method("bounds", |_, scene, ()| {
        let s = scale_of(scene);
        Ok(match scene.compute_bounds() {
            Some((lo, hi)) => (Some(LuaVec(lo / s)), Some(LuaVec(hi / s))),
            None => (None, None),
        })
    });
}

/// Leaves in the document only the selected top-level groups and the
/// selected loose faces.
fn keep_selection(scene: &mut Scene) {
    let mut groups = HashSet::new();
    let mut faces = HashSet::new();
    for e in &scene.selection {
        match *e {
            EntityRef::Group(g) => {
                groups.insert(g);
            }
            EntityRef::Face(MeshId::LOOSE, f) => {
                faces.insert(f);
            }
            _ => {}
        }
    }
    scene.doc.groups.retain(|g| groups.contains(&g.id));
    let loose = scene.doc.mesh_mut(MeshId::LOOSE);
    for f in loose.face_ids() {
        if !faces.contains(&f) {
            loose.remove_face(f);
        }
    }
}
