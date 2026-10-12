// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What a selection can hold.
//!
//! A selection mixes edges, faces, groups, dimensions, labels, guides and
//! section planes. It holds typed references; an entity removed from the
//! document simply stops resolving.

use xtrude_core::mesh::{EdgeId, FaceId};

use crate::ids::{GroupId, MeshId, ObjId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityRef {
    Edge(MeshId, EdgeId),
    Face(MeshId, FaceId),
    Group(GroupId),
    Dimension(ObjId),
    TextLabel(ObjId),
    Guide(ObjId),
    SectionPlane(ObjId),
}

impl EntityRef {
    pub fn is_edge(&self) -> bool {
        matches!(self, EntityRef::Edge(..))
    }

    pub fn is_face(&self) -> bool {
        matches!(self, EntityRef::Face(..))
    }

    pub fn is_group(&self) -> bool {
        matches!(self, EntityRef::Group(..))
    }

    pub fn as_face(&self) -> Option<(MeshId, FaceId)> {
        match *self {
            EntityRef::Face(m, f) => Some((m, f)),
            _ => None,
        }
    }

    pub fn as_group(&self) -> Option<GroupId> {
        match *self {
            EntityRef::Group(g) => Some(g),
            _ => None,
        }
    }
}
