// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Xtrude CAD document model: scene, groups and components, layers, materials,
//! annotations, units, and the command history (one atomic undo step per
//! user action).

pub mod commands;
pub mod crash;
pub mod definitions;
pub mod dimension;
pub mod edits;
pub mod entity;
pub mod explode;
pub mod group;
pub mod guide;
pub mod history;
pub mod i18n;
pub mod ids;
pub mod insert;
pub mod intersect;
pub mod layers;
pub mod materials;
pub mod model_tree;
pub mod parts;
pub mod paths;
pub mod pieces;
pub mod purge;
pub mod pushpull;
pub mod saved_views;
pub mod scene;
pub mod section;
pub mod select_ops;
pub mod settings;
pub mod solids;
pub mod style;
pub mod text3d;
pub mod textlabel;
pub mod units;

/// The application version: the workspace `version` in `Cargo.toml`, the
/// single source of truth. Xtrude CAD numbers its own releases; it is not
/// IngeTrazo's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use entity::EntityRef;
pub use history::{CmdError, CmdResult, Command, History};
pub use ids::{GroupId, MeshId};
pub use scene::{Doc, Scene};
