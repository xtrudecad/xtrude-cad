// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What the frame on screen shows, read from its depth. The shell reads the
//! depth back from the GPU and hands it over with
//! [`Viewport::set_depth_snap`]; the zoom, the pan and the overlay's
//! annotations then answer with a lookup instead of a ray cast against the
//! whole model.

use glam::{DMat4, DVec3, DVec4};
use xtrude_core::math::MatExt;

use super::Viewport;
use crate::input::Input;

/// The depth of the frame on screen, rows top to bottom (0 near … 1 far,
/// the GL window depth), and the camera it was drawn with.
#[derive(Clone, Debug)]
pub struct DepthSnap {
    depth: Vec<f32>,
    width: u32,
    height: u32,
    view_proj: DMat4,
    inverse: DMat4,
    eye: DVec3,
    forward: DVec3,
}

impl DepthSnap {
    fn at(&self, ix: usize, iy: usize) -> f64 {
        self.depth[iy * self.width as usize + ix] as f64
    }

    /// The world point at NDC `(nx, ny)` and window depth `d`.
    fn unproject(&self, nx: f64, ny: f64, d: f64) -> Option<DVec3> {
        let q = self.inverse * DVec4::new(nx, ny, d * 2.0 - 1.0, 1.0);
        (q.w.abs() >= 1e-12).then(|| q.truncate() / q.w)
    }
}

/// What the frame shows under a pixel ([`Viewport::depth_world_at`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DepthSeen {
    Hit(DVec3),
    Background,
}

impl Viewport {
    fn view_proj(&self) -> DMat4 {
        self.camera.projection_matrix() * self.camera.view_matrix()
    }

    /// A frame of `size` device pixels is about to be drawn with the
    /// camera as it is now: its depth may be read while the camera stays
    /// here (and the last snapshot is dropped).
    pub(crate) fn note_painted_view(&mut self, size: [u32; 2]) {
        self.nav.depth_snap = None;
        self.nav.painted_view = Some((self.view_proj(), size));
    }

    /// The overlay's annotations ask «is this point hidden?» this frame:
    /// the shell reads the depth back once after drawing.
    pub fn annotations_need_depth(&self) -> bool {
        let mode = &self.effective_style().face_mode;
        if mode == "xray" || mode == "wireframe" {
            return false;
        }
        let doc = &self.scene.doc;
        !doc.dimensions.is_empty() || !doc.text_labels.is_empty()
    }

    /// The frame on screen is still what the camera sees and its depth has
    /// not been read yet: the shell reads it before a zoom or an orbit
    /// asks [`Self::depth_world_at`].
    pub fn wants_depth(&self) -> bool {
        let Some((mvp, _)) = self.nav.painted_view else {
            return false;
        };
        mvp == self.view_proj()
            && self
                .nav
                .depth_snap
                .as_ref()
                .is_none_or(|s| s.view_proj != mvp)
    }

    /// [`Self::wants_depth`], for an input about to be handled: a press
    /// (it may start a pan that grabs the point under it), a move that
    /// starts a camera drag with no button (the Touchpad style), or a
    /// wheel notch that picks its focus. The notches of a run that reuse
    /// the last focus read nothing.
    pub fn wants_depth_for(&self, ev: &Input) -> bool {
        match ev {
            Input::Press(_) => self.wants_depth(),
            Input::Move(e) => self.starts_move_drag(e) && self.wants_depth(),
            Input::Wheel { x, y, .. } => self.wants_depth() && self.wheel_picks(*x, *y),
            _ => false,
        }
    }

    /// The depth the frame just drawn holds, `width × height` values, rows
    /// top to bottom. Taken with the camera as it is.
    pub fn set_depth_snap(&mut self, depth: Vec<f32>, width: u32, height: u32) {
        if depth.len() != (width as usize) * (height as usize) || width == 0 || height == 0 {
            self.nav.depth_snap = None;
            return;
        }
        let view_proj = self.view_proj();
        let Some(inverse) = view_proj.try_inverse() else {
            return;
        };
        self.nav.depth_snap = Some(DepthSnap {
            depth,
            width,
            height,
            view_proj,
            inverse,
            eye: self.camera.eye(),
            forward: self.camera.forward(),
        });
    }

    /// The depth of the frame on screen, once read back.
    pub fn depth_snap(&self) -> Option<&DepthSnap> {
        self.nav.depth_snap.as_ref()
    }

    /// What the last frame shows under widget pixel `(x, y)`, read from
    /// its depth, or `None` when there is no frame to read (the camera
    /// moved since, an export, no read-back). The zoom and the pan's
    /// grabbed point ask this first: what is on screen, hidden objects and
    /// cuts included, without building the pick index.
    pub fn depth_world_at(&self, x: f64, y: f64) -> Option<DepthSeen> {
        let (painted, _) = self.nav.painted_view?;
        if self.view_proj() != painted {
            return None;
        }
        let snap = self
            .nav
            .depth_snap
            .as_ref()
            .filter(|s| s.view_proj == painted)?;
        let (lo, hi) = snap
            .depth
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &d| {
                (a.min(d), b.max(d))
            });
        if lo >= hi {
            return None; // a blank read-back: no depth to read
        }
        let (w, h) = (snap.width as f64, snap.height as f64);
        let dpr = w / self.width().max(1.0);
        let (fx, fy) = ((x * dpr).floor(), (y * dpr).floor());
        if fx < 0.0 || fy < 0.0 || fx >= w || fy >= h {
            return None;
        }
        let (ix, iy) = (fx as usize, fy as usize);
        let d = snap.at(ix, iy);
        if d >= 1.0 {
            return Some(DepthSeen::Background);
        }
        let nx = (fx + 0.5) / w * 2.0 - 1.0;
        let ny = 1.0 - (fy + 0.5) / h * 2.0;
        snap.unproject(nx, ny, d).map(DepthSeen::Hit)
    }

    /// [`Self::is_occluded`] for what the overlay draws this frame, read
    /// from the depth the frame just rendered. The point is hidden when the
    /// nearest surface at its pixel lies clearly in front of it along the
    /// view, with a tolerance so a dimension ending ON a face is not hidden
    /// by that face. Without a snapshot it asks the rays.
    pub fn occluded_on_screen(&mut self, world: DVec3) -> bool {
        let Some(snap) = self.nav.depth_snap.as_ref() else {
            return self.is_occluded(world);
        };
        let c = snap.view_proj * world.extend(1.0);
        if c.w <= 1e-9 {
            return false;
        }
        let (nx, ny) = (c.x / c.w, c.y / c.w);
        let (w, h) = (snap.width as f64, snap.height as f64);
        let (px, py) = ((nx + 1.0) * 0.5 * w, (1.0 - ny) * 0.5 * h);
        if px < 0.0 || py < 0.0 || px >= w || py >= h {
            return false;
        }
        let (ix, iy) = (px as usize, py as usize);
        // The FARTHEST depth of the 3×3 around the pixel: a point on a
        // silhouette must not be hidden by the face next to it.
        let mut d = 0.0f64;
        for yy in iy.saturating_sub(1)..(iy + 2).min(snap.height as usize) {
            for xx in ix.saturating_sub(1)..(ix + 2).min(snap.width as usize) {
                d = d.max(snap.at(xx, yy));
            }
        }
        if d >= 1.0 {
            return false; // background: nothing in front
        }
        let Some(surf) = snap.unproject(nx, ny, d) else {
            return false;
        };
        let along_pt = (world - snap.eye).dot(snap.forward);
        let along_surf = (surf - snap.eye).dot(snap.forward);
        let tol = (0.003 * along_pt.abs()).max(0.02);
        along_surf < along_pt - tol
    }
}
