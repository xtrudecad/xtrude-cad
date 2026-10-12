// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! `FakeProbe`: a stand-in viewport's projection and picks, answered by
//! the test.

use std::cell::RefCell;
use std::rc::Rc;

use glam::DVec3;
use xtrude_app::input::ScreenPt;
use xtrude_app::viewport::{FaceHit, PickedEdge, Projection, StandInProbe, ViewProbe, Viewport};
use xtrude_core::camera::OrbitCamera;
use xtrude_core::{Axis, FaceId};
use xtrude_model::ids::{GroupId, MeshId, ObjId};

type At<T> = Box<dyn Fn(f64, f64) -> Option<T>>;
type ToPixel = Box<dyn Fn(DVec3) -> Option<(f64, f64)>>;
/// `(start, lock direction, x, y)` → the point on the lock line.
type OntoLockLine = Box<dyn Fn(DVec3, DVec3, f64, f64) -> DVec3>;
/// `(x, y, rect_only)` → the text label there.
type TextLabelAt = Box<dyn Fn(f64, f64, bool) -> Option<ObjId>>;

/// What the test answers; `None` leaves the stand-in's own answer.
#[derive(Default)]
struct Answers {
    world_to_pixel: Option<ToPixel>,
    pixel_to_ray: Option<At<(DVec3, DVec3)>>,
    project_to_lock_line: Option<OntoLockLine>,
    eye: Option<DVec3>,
    /// The face every face pick answers.
    pick: Option<FaceHit>,
    pick_group: Option<At<GroupId>>,
    pick_edge: Option<At<PickedEdge>>,
    pick_edge_any: Option<At<PickedEdge>>,
    pick_guide: Option<At<ObjId>>,
    pick_axis: Option<At<Axis>>,
    pick_dimension: Option<At<ObjId>>,
    pick_section_plane: Option<At<ObjId>>,
    pick_text_label: Option<TextLabelAt>,
}

/// A stand-in's probe ([`StandInProbe`]) with the answers a test sets.
/// Clones share the answers, so a test changes them while the viewport
/// holds the probe.
#[derive(Clone, Default)]
pub struct FakeProbe(Rc<RefCell<Answers>>);

thread_local! {
    /// Each fake installed on a viewport, with the `Rc` the viewport holds
    /// (kept alive here, so its address names that fake only).
    static INSTALLED: RefCell<Vec<(Rc<dyn ViewProbe>, FakeProbe)>> = RefCell::default();
}

impl FakeProbe {
    /// The fake `vp` projects and picks through, installed first unless
    /// it already has one.
    pub fn of(vp: &mut Viewport) -> FakeProbe {
        let current = vp.probe();
        let found = INSTALLED.with(|all| {
            all.borrow()
                .iter()
                .find(|(p, _)| Rc::ptr_eq(p, &current))
                .map(|(_, f)| f.clone())
        });
        if let Some(fake) = found {
            return fake;
        }
        let fake = FakeProbe::default();
        let rc: Rc<dyn ViewProbe> = Rc::new(fake.clone());
        vp.set_probe(Rc::clone(&rc));
        INSTALLED.with(|all| all.borrow_mut().push((rc, fake.clone())));
        fake
    }

    pub fn world_to_pixel(&self, f: impl Fn(DVec3) -> Option<(f64, f64)> + 'static) -> &Self {
        self.0.borrow_mut().world_to_pixel = Some(Box::new(f));
        self
    }
    pub fn pixel_to_ray(&self, f: impl Fn(f64, f64) -> Option<(DVec3, DVec3)> + 'static) -> &Self {
        self.0.borrow_mut().pixel_to_ray = Some(Box::new(f));
        self
    }
    pub fn project_to_lock_line(
        &self,
        f: impl Fn(DVec3, DVec3, f64, f64) -> DVec3 + 'static,
    ) -> &Self {
        self.0.borrow_mut().project_to_lock_line = Some(Box::new(f));
        self
    }
    /// The eye of the stand-in's camera; `None`: no camera at all.
    pub fn eye(&self, eye: Option<DVec3>) -> &Self {
        self.0.borrow_mut().eye = eye;
        self
    }
    /// The face every face pick answers.
    pub fn pick(&self, hit: Option<FaceHit>) -> &Self {
        self.0.borrow_mut().pick = hit;
        self
    }
    pub fn pick_group(&self, f: impl Fn(f64, f64) -> Option<GroupId> + 'static) -> &Self {
        self.0.borrow_mut().pick_group = Some(Box::new(f));
        self
    }
    pub fn pick_edge(&self, f: impl Fn(f64, f64) -> Option<PickedEdge> + 'static) -> &Self {
        self.0.borrow_mut().pick_edge = Some(Box::new(f));
        self
    }
    /// What `pick_edge_any` answers (a group's edge as a world pseudo-edge).
    pub fn pick_edge_any(&self, f: impl Fn(f64, f64) -> Option<PickedEdge> + 'static) -> &Self {
        self.0.borrow_mut().pick_edge_any = Some(Box::new(f));
        self
    }
    pub fn pick_guide(&self, f: impl Fn(f64, f64) -> Option<ObjId> + 'static) -> &Self {
        self.0.borrow_mut().pick_guide = Some(Box::new(f));
        self
    }
    pub fn pick_axis(&self, f: impl Fn(f64, f64) -> Option<Axis> + 'static) -> &Self {
        self.0.borrow_mut().pick_axis = Some(Box::new(f));
        self
    }
    pub fn pick_dimension(&self, f: impl Fn(f64, f64) -> Option<ObjId> + 'static) -> &Self {
        self.0.borrow_mut().pick_dimension = Some(Box::new(f));
        self
    }
    pub fn pick_section_plane(&self, f: impl Fn(f64, f64) -> Option<ObjId> + 'static) -> &Self {
        self.0.borrow_mut().pick_section_plane = Some(Box::new(f));
        self
    }
    /// `(x, y, rect_only)` → the leader text there.
    pub fn pick_text_label(&self, f: impl Fn(f64, f64, bool) -> Option<ObjId> + 'static) -> &Self {
        self.0.borrow_mut().pick_text_label = Some(Box::new(f));
        self
    }
}

impl ViewProbe for FakeProbe {
    fn camera<'v>(&self, _vp: &'v Viewport) -> Option<&'v OrbitCamera> {
        None
    }
    fn projects(&self) -> bool {
        self.0.borrow().world_to_pixel.is_some()
    }
    fn world_to_pixel(&self, _vp: &Viewport, p: DVec3) -> Option<ScreenPt> {
        let a = self.0.borrow();
        a.world_to_pixel
            .as_ref()
            .and_then(|f| f(p))
            .map(ScreenPt::from)
    }
    fn projection(&self, _vp: &Viewport) -> Option<Projection> {
        None
    }
    fn pixel_to_ray(&self, _vp: &Viewport, at: ScreenPt) -> Option<(DVec3, DVec3)> {
        let a = self.0.borrow();
        a.pixel_to_ray.as_ref().and_then(|f| f(at.x, at.y))
    }
    fn project_to_lock_line(
        &self,
        vp: &Viewport,
        start: DVec3,
        lock_dir: DVec3,
        at: ScreenPt,
    ) -> DVec3 {
        if let Some(f) = &self.0.borrow().project_to_lock_line {
            return f(start, lock_dir, at.x, at.y);
        }
        StandInProbe.project_to_lock_line(vp, start, lock_dir, at)
    }
    fn eye(&self, _vp: &Viewport) -> Option<DVec3> {
        self.0.borrow().eye
    }
    fn pick_face(&self, _vp: &mut Viewport, _at: ScreenPt) -> Option<(MeshId, FaceId)> {
        self.0.borrow().pick.as_ref().map(|h| (h.mesh, h.face))
    }
    fn pick_face_any(&self, _vp: &mut Viewport, _at: ScreenPt) -> Option<FaceHit> {
        self.0.borrow().pick.clone()
    }
    fn pick_face_placement(&self, _vp: &mut Viewport, _at: ScreenPt) -> Option<FaceHit> {
        self.0.borrow().pick.clone()
    }
    fn pick_group(&self, vp: &mut Viewport, at: ScreenPt) -> Option<GroupId> {
        if let Some(f) = &self.0.borrow().pick_group {
            return f(at.x, at.y);
        }
        StandInProbe.pick_group(vp, at)
    }
    fn pick_edge(&self, vp: &mut Viewport, at: ScreenPt, visible_only: bool) -> Option<PickedEdge> {
        if let Some(f) = &self.0.borrow().pick_edge {
            return f(at.x, at.y);
        }
        StandInProbe.pick_edge(vp, at, visible_only)
    }
    fn pick_edge_any(&self, vp: &mut Viewport, at: ScreenPt) -> Option<PickedEdge> {
        if let Some(f) = &self.0.borrow().pick_edge_any {
            return f(at.x, at.y);
        }
        StandInProbe.pick_edge_any(vp, at)
    }
    fn pick_guide(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        if let Some(f) = &self.0.borrow().pick_guide {
            return f(at.x, at.y);
        }
        StandInProbe.pick_guide(vp, at)
    }
    fn pick_axis(&self, vp: &Viewport, at: ScreenPt) -> Option<Axis> {
        if let Some(f) = &self.0.borrow().pick_axis {
            return f(at.x, at.y);
        }
        StandInProbe.pick_axis(vp, at)
    }
    fn pick_dimension(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        if let Some(f) = &self.0.borrow().pick_dimension {
            return f(at.x, at.y);
        }
        StandInProbe.pick_dimension(vp, at)
    }
    fn pick_section_plane(&self, vp: &mut Viewport, at: ScreenPt) -> Option<ObjId> {
        if let Some(f) = &self.0.borrow().pick_section_plane {
            return f(at.x, at.y);
        }
        StandInProbe.pick_section_plane(vp, at)
    }
    fn pick_text_label(&self, vp: &mut Viewport, at: ScreenPt, rect_only: bool) -> Option<ObjId> {
        if let Some(f) = &self.0.borrow().pick_text_label {
            return f(at.x, at.y, rect_only);
        }
        StandInProbe.pick_text_label(vp, at, rect_only)
    }
}
