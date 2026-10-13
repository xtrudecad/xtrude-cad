// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The integration tests of `xtrude-io`, as one test binary: one binary
//! per test file made the test build link the dependencies hundreds of
//! times.

mod common;

mod test_3mf_export;
mod test_app_version;
mod test_clipboard_windows;
mod test_component_copies_share_nested;
mod test_convert_step;
mod test_copies_split_on_load;
mod test_curve;
mod test_dae_import;
mod test_dimension_anchors;
mod test_dimensions;
mod test_document;
mod test_document_camera;
mod test_dxf_import;
mod test_explode_view;
mod test_export;
mod test_export_material_names;
mod test_face_sides;
mod test_face_split;
mod test_gltf_dae_export;
mod test_golden;
mod test_group_axes;
mod test_group_edit;
mod test_group_material;
mod test_group_vs_component;
mod test_hide_edges;
mod test_hide_objects;
mod test_igz_dangling_proto;
mod test_igz_errors;
mod test_insert_document;
mod test_layers;
mod test_materials;
mod test_materials_registry;
mod test_measure_eraser;
mod test_mesh;
mod test_model_units;
mod test_nested_instances;
mod test_new_document_units;
mod test_non_finite_values;
mod test_obj_import;
mod test_obj_parts;
mod test_plugin_data;
mod test_print_materials;
mod test_saved_views;
mod test_sections;
mod test_skp_backend;
mod test_skp_layer_visibility;
mod test_skp_tree_import;
mod test_step_import;
mod test_step_payload;
mod test_stl_import;
mod test_styles;
mod test_text;
mod test_text3d_letters;
mod test_upstream_documents_open;
mod test_value_box_chords;
