// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Runtime identities for document objects.
//!
//! Groups, dimensions, labels, guides and section planes are compared by
//! identity: each gets a process-unique id; a copy is a new
//! object with a new id, and a snapshot restored by undo keeps the ids it
//! had, so selections survive undo/redo.

use std::sync::atomic::{AtomicU64, Ordering};

/// Process-unique object id.
pub type ObjId = u64;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> ObjId {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Identity of a mesh in the scene's mesh table. `LOOSE` is the loose mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshId(pub u64);

impl MeshId {
    pub const LOOSE: MeshId = MeshId(0);

    pub fn fresh() -> MeshId {
        MeshId(next_id())
    }
}

/// Identity of a group (a node of the group tree).
pub type GroupId = ObjId;
