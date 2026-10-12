// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The integration tests of `xtrude-model`, as one test binary: one binary
//! per test file made the test build link the dependencies hundreds of
//! times.

mod common;

mod test_arrangement;
mod test_automerge;
mod test_cap_rebuild;
mod test_component_edit;
mod test_coplanar_merge;
mod test_crossing_edges;
mod test_delete_edge_face;
mod test_divide;
mod test_edge_flags_travel;
mod test_edge_split;
mod test_erase_leaves_no_orphan_vertices;
mod test_face_attrs;
mod test_face_chord_split;
mod test_face_split;
mod test_face_subdivide;
mod test_fuzz_engine;
mod test_fuzz_regressions;
mod test_group_material;
mod test_group_vs_component;
mod test_insert_document;
mod test_local_axes_drawing;
mod test_make_group_in_open_group;
mod test_make_independent_nested;
mod test_materials_registry;
mod test_model_tree;
mod test_move;
mod test_move_undo;
mod test_offset;
mod test_orient_faces;
mod test_paint;
mod test_parts;
mod test_plugin_data;
mod test_purge;
mod test_pushpull_accepts;
mod test_pushpull_against_wall;
mod test_pushpull_in_scene;
mod test_pushpull_orient;
mod test_pushpull_slanted;
mod test_pushpull_wall_cut;
mod test_rectangle_across_a_face;
mod test_reversed_faces_survive_drawing;
mod test_saved_views;
mod test_simplify_mesh;
mod test_sliver_wildcard_plane;
mod test_solids;
mod test_split_pieces;
mod test_style_editor;
mod test_styles;
mod test_sweep_revolution;
mod test_text3d_letters;
mod test_topology_engine_basics;
mod test_transactional_history;
mod test_undo_clean;
mod test_user_subdivisions;
