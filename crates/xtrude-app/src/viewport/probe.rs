// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)

//! How the viewport turns pixels into world rays and picks what is under
//! the cursor.
//!
//! Every projection and pick of [`Viewport`] goes through its
//! [`ViewProbe`]. [`CameraProbe`] is the real one: it projects through the
//! orbit camera and picks from the pick cache. [`StandInProbe`] is the
//! probe of [`Viewport::stub`], the viewport tool tests drive tools with:
//! a scene and a history, but no camera to read pixels through. Tests
//! install their own probe with [`Viewport::set_probe`] to answer a pick
//! or a projection.
//!
//! The viewport holds its probe as an `Rc<dyn ViewProbe>` and clones the
//! `Rc` for each call, so a probe method can take the viewport itself
//! (mutably, for the picks that fill the pick cache) and a probe can call
//! back into the viewport, or into another probe, without a borrow
//! conflict.

use glam::DVec3;
use xtrude_core::camera::OrbitCamera;
use xtrude_core::math::VecExt;
use xtrude_core::{Axis, FaceId};
use xtrude_model::ids::{GroupId, MeshId, ObjId};

use super::{FaceHit, PickedEdge, Projection, Viewport};
use crate::input::ScreenPt;

/// The viewport's projection (world ↔ pixel) and picking.
pub trait ViewProbe {
    /// The camera the view looks through; `None` when there is none to
    /// project with (a stand-in).
    fn camera<'v>(&self, vp: &'v Viewport) -> Option<&'v OrbitCamera>;
    /// Whether [`Self::world_to_pixel`] can answer at all.
    fn projects(&self) -> bool;
    /// World point → widget pixel, `None` behind the camera.
    fn world_to_pixel(&self, vp: &Viewport, p: DVec3) -> Option<ScreenPt>;
    /// [`Self::world_to_pixel`] as one projection, to project many points
    /// in one pass (the pick's edges); `None` when the probe answers point
    /// by point.
    fn projection(&self, vp: &Viewport) -> Option<Projection>;
    /// Camera ray (origin, unit direction) through a pixel.
    fn pixel_to_ray(&self, vp: &Viewport, at: ScreenPt) -> Option<(DVec3, DVec3)>;
    /// Closest point on the lock line (`start`, `lock_dir`) to the camera
    /// ray through the pixel.
    fn project_to_lock_line(
        &self,
        vp: &Viewport,
        start: DVec3,
        lock_dir: DVec3,
        at: ScreenPt,
    ) -> DVec3;
    /// The eye the view is seen from; `None` without a camera.
    fn eye(&self, vp: &Viewport) -> Option<DVec3>;
    /// The loose face under the cursor.
    fn pick_face(&self, vp: &mut Viewport, at: ScreenPt) -> Option<(MeshId, FaceId)>;
    /// The front-most face across the loose mesh and every group.
    fn pick_face_any(&self, vp: &mut Viewport, at: ScreenPt) -> Option<FaceHit>;
    /// Like [`Self::pick_face_any`], through the placement the face was hit.
    fn pick_face_placement(&self, vp: &mut Viewport, at: ScreenPt) -> Option<FaceHit>;
    /// The group whose geometry is under the cursor.
    fn pick_group(&self, vp: &mut Viewport, at: ScreenPt) -> Option<GroupId>;
    /// The loose edge nearest the cursor; with `visible_only`, never a
    /// soft or hidden one.
    fn pick_edge(&self, vp: &mut Viewport, at: ScreenPt, visible_only: bool) -> Option<PickedEdge>;
    /// The edge under the cursor, loose or a group's.
    fn pick_edge_any(&self, vp: &mut Viewport, at: ScreenPt) -> Option<PickedEdge>;
    /// The construction guide nearest the cursor.
    fn pick_guide(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId>;
    /// The model axis line under the cursor.
    fn pick_axis(&self, vp: &Viewport, at: ScreenPt) -> Option<Axis>;
    /// The dimension nearest the cursor.
    fn pick_dimension(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId>;
    /// The section plane whose frame is nearest the cursor.
    fn pick_section_plane(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId>;
    /// The leader-text label under the cursor.
    fn pick_text_label(&self, vp: &mut Viewport, at: ScreenPt, rect_only: bool) -> Option<ObjId>;
}

/// The real probe: the orbit camera and the pick cache.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraProbe;

impl ViewProbe for CameraProbe {
    fn camera<'v>(&self, vp: &'v Viewport) -> Option<&'v OrbitCamera> {
        Some(&vp.camera)
    }
    fn projects(&self) -> bool {
        true
    }
    fn world_to_pixel(&self, vp: &Viewport, p: DVec3) -> Option<ScreenPt> {
        vp.camera_world_to_pixel(p)
    }
    fn projection(&self, vp: &Viewport) -> Option<Projection> {
        Some(vp.camera_projection())
    }
    fn pixel_to_ray(&self, vp: &Viewport, at: ScreenPt) -> Option<(DVec3, DVec3)> {
        vp.camera_pixel_to_ray(at)
    }
    fn project_to_lock_line(
        &self,
        vp: &Viewport,
        start: DVec3,
        lock_dir: DVec3,
        at: ScreenPt,
    ) -> DVec3 {
        let Some((ro, rd)) = vp.pixel_to_ray(at) else {
            return start;
        };
        let d1 = lock_dir.unit_or_zero();
        let r = start - ro;
        let b = d1.dot(rd);
        let d = d1.dot(r);
        let e = rd.dot(r);
        let denom = 1.0 - b * b;
        let t = if denom.abs() < 1e-6 {
            -d
        } else {
            (b * e - d) / denom
        };
        start + d1 * t
    }
    fn eye(&self, vp: &Viewport) -> Option<DVec3> {
        Some(vp.camera.eye())
    }
    fn pick_face(&self, vp: &mut Viewport, at: ScreenPt) -> Option<(MeshId, FaceId)> {
        vp.camera_pick_face(at)
    }
    fn pick_face_any(&self, vp: &mut Viewport, at: ScreenPt) -> Option<FaceHit> {
        vp.camera_pick_face_any(at)
    }
    fn pick_face_placement(&self, vp: &mut Viewport, at: ScreenPt) -> Option<FaceHit> {
        vp.camera_pick_face_placement(at)
    }
    fn pick_group(&self, vp: &mut Viewport, at: ScreenPt) -> Option<GroupId> {
        vp.camera_pick_group(at)
    }
    fn pick_edge(&self, vp: &mut Viewport, at: ScreenPt, visible_only: bool) -> Option<PickedEdge> {
        vp.camera_pick_edge(at, visible_only)
    }
    fn pick_edge_any(&self, vp: &mut Viewport, at: ScreenPt) -> Option<PickedEdge> {
        vp.camera_pick_edge_any(at)
    }
    fn pick_guide(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        vp.camera_pick_guide(at)
    }
    fn pick_axis(&self, vp: &Viewport, at: ScreenPt) -> Option<Axis> {
        vp.camera_pick_axis(at)
    }
    fn pick_dimension(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        vp.camera_pick_dimension(at)
    }
    fn pick_section_plane(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        vp.camera_pick_section_plane(at)
    }
    fn pick_text_label(&self, vp: &mut Viewport, at: ScreenPt, rect_only: bool) -> Option<ObjId> {
        vp.camera_pick_text_label(at, rect_only)
    }
}

/// The probe of a stand-in viewport: no camera, so no projection, no eye
/// and no face under the cursor. The other picks run the camera's
/// algorithms, which find nothing without a projection unless a test's
/// probe projects for them.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandInProbe;

impl ViewProbe for StandInProbe {
    fn camera<'v>(&self, _vp: &'v Viewport) -> Option<&'v OrbitCamera> {
        None
    }
    fn projects(&self) -> bool {
        false
    }
    fn world_to_pixel(&self, _vp: &Viewport, _p: DVec3) -> Option<ScreenPt> {
        None
    }
    fn projection(&self, _vp: &Viewport) -> Option<Projection> {
        None
    }
    fn pixel_to_ray(&self, _vp: &Viewport, _at: ScreenPt) -> Option<(DVec3, DVec3)> {
        None
    }
    fn project_to_lock_line(
        &self,
        vp: &Viewport,
        start: DVec3,
        lock_dir: DVec3,
        at: ScreenPt,
    ) -> DVec3 {
        CameraProbe.project_to_lock_line(vp, start, lock_dir, at)
    }
    fn eye(&self, _vp: &Viewport) -> Option<DVec3> {
        None
    }
    fn pick_face(&self, _vp: &mut Viewport, _at: ScreenPt) -> Option<(MeshId, FaceId)> {
        None
    }
    fn pick_face_any(&self, _vp: &mut Viewport, _at: ScreenPt) -> Option<FaceHit> {
        None
    }
    fn pick_face_placement(&self, _vp: &mut Viewport, _at: ScreenPt) -> Option<FaceHit> {
        None
    }
    fn pick_group(&self, vp: &mut Viewport, at: ScreenPt) -> Option<GroupId> {
        CameraProbe.pick_group(vp, at)
    }
    fn pick_edge(&self, vp: &mut Viewport, at: ScreenPt, visible_only: bool) -> Option<PickedEdge> {
        CameraProbe.pick_edge(vp, at, visible_only)
    }
    fn pick_edge_any(&self, vp: &mut Viewport, at: ScreenPt) -> Option<PickedEdge> {
        CameraProbe.pick_edge_any(vp, at)
    }
    fn pick_guide(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        CameraProbe.pick_guide(vp, at)
    }
    fn pick_axis(&self, vp: &Viewport, at: ScreenPt) -> Option<Axis> {
        CameraProbe.pick_axis(vp, at)
    }
    fn pick_dimension(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        CameraProbe.pick_dimension(vp, at)
    }
    fn pick_section_plane(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        CameraProbe.pick_section_plane(vp, at)
    }
    fn pick_text_label(&self, vp: &mut Viewport, at: ScreenPt, rect_only: bool) -> Option<ObjId> {
        CameraProbe.pick_text_label(vp, at, rect_only)
    }
}
