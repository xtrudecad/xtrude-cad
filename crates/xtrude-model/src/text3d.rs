// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! 3D Text as the app inserts it: a container group with ONE NESTED GROUP
//! PER LETTER, carrying the parameters it was made from in `Group::text3d`
//! so the text stays EDITABLE (see [`EditText3DCommand`]). The glyph
//! geometry itself is [`xtrude_core::text3d`].

use serde_json::{Map, Value};
use sha1::{Digest, Sha1};
use xtrude_core::math::DMat4;
use xtrude_core::stable_json::{self, round_exact, Json};
use xtrude_core::text3d::build_text_letters;

use crate::group::{add_mesh, Group, MeshTable};
use crate::history::{CmdError, CmdResult, Command};
use crate::ids::GroupId;
use crate::scene::Scene;

/// What the 3D Text dialog asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct TextParams {
    pub text: String,
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    pub height: f64,
    pub thickness: f64,
}

impl Default for TextParams {
    fn default() -> Self {
        TextParams {
            text: String::new(),
            font: String::new(),
            bold: true,
            italic: false,
            height: 0.25,
            thickness: 0.05,
        }
    }
}

impl TextParams {
    /// `text_params(text, font)` with the dialog's defaults.
    pub fn new(text: &str, font: &str) -> Self {
        TextParams {
            text: text.to_string(),
            font: font.to_string(),
            ..Self::default()
        }
    }

    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("text".into(), Value::from(self.text.clone()));
        m.insert("font".into(), Value::from(self.font.clone()));
        m.insert("bold".into(), Value::from(self.bold));
        m.insert("italic".into(), Value::from(self.italic));
        m.insert("height".into(), Value::from(self.height));
        m.insert("thickness".into(), Value::from(self.thickness));
        m
    }

    /// The parameters a stored `text3d` record holds (defaults for the
    /// missing ones).
    pub fn from_map(m: &Map<String, Value>) -> Self {
        let d = Self::default();
        TextParams {
            text: m
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            font: m
                .get("font")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            bold: m.get("bold").and_then(Value::as_bool).unwrap_or(d.bold),
            italic: m.get("italic").and_then(Value::as_bool).unwrap_or(d.italic),
            height: m.get("height").and_then(Value::as_f64).unwrap_or(d.height),
            thickness: m
                .get("thickness")
                .and_then(Value::as_f64)
                .unwrap_or(d.thickness),
        }
    }

    /// The container's name: the text, trimmed, at most 24 characters.
    pub fn name(&self) -> String {
        self.text.trim().chars().take(24).collect()
    }
}

/// One nested group per letter, each an instance over its own mesh in the
/// text's frame — `xform` is the pose every letter starts from.
fn letter_groups(meshes: &mut MeshTable, params: &TextParams, xform: DMat4) -> Vec<Group> {
    build_text_letters(
        &params.text,
        &params.font,
        params.bold,
        params.italic,
        params.height,
        params.thickness,
    )
    .into_iter()
    .map(|(ch, mesh)| {
        let id = add_mesh(meshes, mesh);
        let mut g = Group::new(id, Some(&ch.to_string()));
        g.xform = Some(xform);
        g
    })
    .collect()
}

/// What the letters ARE, as a short digest: names in order, the vertex
/// cloud of each (local coordinates, 0.1 mm), face and edge counts, and the
/// paint on each face: the first 16 hex digits of the SHA-1 of the parts
/// dumped by `xtrude_core::stable_json`. The bytes never change, so a
/// fingerprint stored in a saved file verifies.
pub fn letters_fingerprint(meshes: &MeshTable, children: &[Group]) -> String {
    let mut parts = Vec::new();
    for k in children {
        let empty = xtrude_core::Mesh::new();
        let m = meshes.get(&k.mesh).map(|m| &**m).unwrap_or(&empty);
        let mut cloud: Vec<[f64; 3]> = m
            .verts
            .iter()
            .map(|(_, v)| {
                let p = v.position;
                [
                    round_exact(p.x, 4),
                    round_exact(p.y, 4),
                    round_exact(p.z, 4),
                ]
            })
            .collect();
        cloud.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut paint: Vec<String> = m
            .faces
            .iter()
            .map(|(_, f)| {
                let a = &f.attrs;
                let color = match a.paint.color {
                    Some(c) => {
                        Json::List(c.iter().map(|&x| Json::Float(round_exact(x, 3))).collect())
                    }
                    None => Json::Null,
                };
                let mat = match &a.paint.mat {
                    Some(s) => Json::Str(s.clone()),
                    None => Json::Null,
                };
                stable_json::dump(
                    &Json::Dict(vec![
                        ("color".into(), color),
                        ("mat".into(), mat),
                        ("texture".into(), Json::Bool(false)),
                        ("back".into(), Json::Bool(a.back.is_some())),
                    ]),
                    true,
                )
            })
            .collect();
        paint.sort();
        parts.push(Json::List(vec![
            Json::Str(k.name.clone()),
            Json::Int(m.faces.len() as i64),
            Json::Int(m.edges.len() as i64),
            Json::List(
                cloud
                    .into_iter()
                    .map(|p| Json::List(p.iter().map(|&x| Json::Float(x)).collect()))
                    .collect(),
            ),
            Json::List(paint.into_iter().map(Json::Str).collect()),
        ]));
    }
    let text = stable_json::dump(&Json::List(parts), false);
    let digest = Sha1::digest(text.as_bytes());
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..16]
        .to_string()
}

/// The `text3d` record for a text made of `children`: the parameters plus
/// the letters' fingerprint.
pub fn text_state(
    meshes: &MeshTable,
    params: &TextParams,
    children: &[Group],
) -> Map<String, Value> {
    let mut state = params.to_map();
    state.insert(
        "hash".into(),
        Value::from(letters_fingerprint(meshes, children)),
    );
    state
}

/// Where the text sits, as the pose its letters share: the first letter's
/// matrix (identity when the container has no letters).
pub fn text_frame(group: &Group) -> DMat4 {
    group
        .children
        .iter()
        .find_map(|k| k.xform)
        .unwrap_or(DMat4::IDENTITY)
}

/// True while the letters are exactly what the dialog generated — so
/// re-editing can regenerate them without destroying anything. A record
/// without a fingerprint (older documents) is trusted.
pub fn text_is_pristine(meshes: &MeshTable, group: &Group) -> bool {
    let Some(hash) = group
        .text3d
        .as_ref()
        .and_then(|s| s.get("hash"))
        .and_then(Value::as_str)
        .filter(|h| !h.is_empty())
    else {
        return true;
    };
    if letters_fingerprint(meshes, &group.children) != hash {
        return false;
    }
    let first = text_frame(group);
    group
        .children
        .iter()
        .all(|k| k.xform.unwrap_or(DMat4::IDENTITY) == first)
}

/// The 3D text as the app inserts it: a container (instance at identity,
/// empty mesh of its own) whose children are the letters. `None` when the
/// text has no geometry.
pub fn make_text_group(meshes: &mut MeshTable, params: &TextParams) -> Option<Group> {
    let kids = letter_groups(meshes, params, DMat4::IDENTITY);
    if kids.is_empty() {
        return None;
    }
    let own = add_mesh(meshes, xtrude_core::Mesh::new());
    let mut g = Group::new(own, Some(&params.name()));
    g.text3d = Some(text_state(meshes, params, &kids));
    g.adopt(kids);
    g.component = false; // a 3D text is a group of letters
    Some(g)
}

/// The letters for `params` laid out where `group`'s current letters are
/// — the new children, NOT yet assigned.
pub fn rebuild_text_group(
    meshes: &mut MeshTable,
    group: &Group,
    params: &TextParams,
) -> Vec<Group> {
    letter_groups(meshes, params, text_frame(group))
}

/// Re-edit a 3D text: the container keeps its identity, pose and own mesh;
/// its letters are laid out again from the new parameters where the old
/// ones stood, and the name follows the text.
pub struct EditText3DCommand {
    pub group: GroupId,
    pub params: TextParams,
}

impl Command for EditText3DCommand {
    fn name(&self) -> &str {
        "EditText3DCommand"
    }

    fn apply(&mut self, scene: &mut Scene) -> CmdResult {
        let g = scene
            .doc
            .find_group(self.group)
            .ok_or_else(|| CmdError::msg("group is not in the scene"))?
            .clone();
        let kids = rebuild_text_group(&mut scene.doc.meshes, &g, &self.params);
        let state = text_state(&scene.doc.meshes, &self.params, &kids);
        let name = self.params.name();
        let g = scene.doc.find_group_mut(self.group).expect("found above");
        g.children = kids;
        g.text3d = Some(state);
        if !name.is_empty() {
            g.name = name;
        }
        scene.version += 1;
        Ok(())
    }
}
