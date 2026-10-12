// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What a tool shows live while it works, apart from its own overlay.

use std::collections::HashSet;

use glam::DMat4;
use xtrude_core::{EdgeId, FaceId};
use xtrude_model::ids::{GroupId, MeshId};
use xtrude_model::style::Style;

/// A tool's live preview in the frame: groups drawn through a moving
/// placement (Move / Rotate / Paste drags), faces and edges taken out of
/// the normal pass, and a display style standing in for the scene's.
///
/// `external` and `matrix` describe the moving `groups`: a preview begins
/// with both reset (`begin`) and ends with all three
/// cleared (`end`).
#[derive(Debug, Default)]
pub struct LivePreview {
    /// Groups moving in a live preview: drawn through `matrix` instead of
    /// their own placement.
    pub groups: Vec<GroupId>,
    /// The previewed groups are the clipboard's templates, not the
    /// scene's (Paste).
    pub external: bool,
    pub matrix: Option<DMat4>,
    /// Faces hidden from the normal pass while a tool previews.
    pub(super) suppressed_faces: HashSet<(MeshId, FaceId)>,
    /// Edges hidden from the normal pass while a tool previews.
    pub(super) suppressed_edges: HashSet<(MeshId, EdgeId)>,
    /// Bumped whenever the hidden faces or edges change: the render cache
    /// rebuilds on it.
    pub(super) suppressed_serial: u64,
    /// The display style this frame draws with instead of the scene's.
    pub style_override: Option<Style>,
}

impl LivePreview {
    pub(super) fn begin(&mut self, groups: &[GroupId], external: bool) {
        self.groups = groups.to_vec();
        self.external = external;
        self.matrix = None;
    }

    pub(super) fn end(&mut self) {
        self.groups.clear();
        self.external = false;
        self.matrix = None;
    }
}
