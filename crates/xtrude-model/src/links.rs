// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The links a robot description makes of the model: one per top-level
//! group or component copy, its nested groups merged into it, named after
//! it. A link name holds only `[A-Za-z0-9_]` and no two are the same, so
//! a group's name may come out changed; the model tree shows where.

use crate::ids::{GroupId, MeshId};
use crate::scene::Doc;

/// The root link that geometry outside every group becomes.
pub const BASE_LINK: &str = "base_link";

/// The name a link takes when its group's name gives none.
const UNNAMED: &str = "link";

/// One link: its top-level group and the name it goes out under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub group: GroupId,
    pub name: String,
}

/// `name` with every character outside `[A-Za-z0-9_]` turned into `_`;
/// [`UNNAMED`] when nothing is left.
pub fn sanitize(name: &str) -> String {
    let out: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() {
        UNNAMED.to_string()
    } else {
        out
    }
}

/// Every link of the document, top-level groups in their order. A name
/// already taken (by an earlier link, or [`BASE_LINK`] when loose
/// geometry holds faces) takes the first free `_2`, `_3`… suffix.
pub fn links(doc: &Doc) -> Vec<Link> {
    let mut taken: Vec<String> = Vec::new();
    if !doc.mesh(MeshId::LOOSE).faces.is_empty() {
        taken.push(BASE_LINK.to_string());
    }
    doc.groups
        .iter()
        .map(|g| {
            let base = sanitize(&g.name);
            let mut name = base.clone();
            let mut n = 2;
            while taken.contains(&name) {
                name = format!("{base}_{n}");
                n += 1;
            }
            taken.push(name.clone());
            Link { group: g.id, name }
        })
        .collect()
}
