// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The integration tests of `xtrude-core`, as one test binary: one binary
//! per test file made the test build link the dependencies hundreds of
//! times.

mod snap_common;

mod test_axis_inference_is_magnetic;
mod test_axis_inference_on_screen;
mod test_camera_clip_depth;
mod test_camera_orbit;
mod test_camera_zoom;
mod test_center_inference;
mod test_chamfer;
mod test_close_under_axis_lock;
mod test_concave;
mod test_edge_level_and_extension_inference;
mod test_face_area;
mod test_fillet;
mod test_from_point_during_an_operation;
mod test_fuse;
mod test_heal;
mod test_local_axes_drawing;
mod test_mesh;
mod test_mesh_bulk;
mod test_orbit_pivot;
mod test_orient;
mod test_pan_depth;
mod test_perp_on_slope;
mod test_shared_point_prefers_the_drawing_context;
mod test_shift_lock_has_snaps;
mod test_small_loops;
mod test_snap_extension;
mod test_snap_from_point_first_click;
mod test_snap_intersection;
mod test_snap_level_with_point_across_faces;
mod test_snap_origin_under_lock;
mod test_snap_protractor_arm;
mod test_snap_under_lock_both_sides;
mod test_standard_views_exact;
mod test_text;
mod test_workplane;
mod test_zoom_extents;
mod test_zoom_stuck;
