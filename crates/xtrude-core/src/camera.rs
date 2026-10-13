// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Orbital camera for the 3D viewport.
//!
//! Z-up convention: X red (east), Y green (north), Z blue (up). The camera
//! orbits around a `target` point in spherical coordinates (`yaw`, `pitch`,
//! `distance`). Both perspective and parallel ("orthographic") projections
//! are supported. Matrices are right-handed and act on column vectors; the
//! projections map to OpenGL clip space (z in [-1, 1]).

use glam::{DMat4, DVec3, DVec4};

/// How close the camera may get to what it is looking at (2 cm: close
/// enough for the face of a figure or the rim of a wheel).
pub const MIN_DISTANCE: f64 = 0.02;

/// Ceiling: past this the model is a dot and the depth buffer is noise.
pub const MAX_DISTANCE: f64 = 10000.0;

/// Camera that orbits around a target point.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: DVec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub up: DVec3,
    pub fov_deg: f64,
    pub aspect: f64,
    pub znear: f64,
    pub zfar: f64,
    pub perspective: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        OrbitCamera {
            target: DVec3::ZERO,
            distance: 20.0,
            yaw: (-45.0f64).to_radians(),
            pitch: 30.0f64.to_radians(),
            up: DVec3::Z,
            fov_deg: 45.0,
            aspect: 1.0,
            znear: 0.1,
            zfar: 10000.0,
            perspective: true,
        }
    }
}

fn clamp_pitch(p: f64) -> f64 {
    p.min(89.0f64.to_radians()).max((-89.0f64).to_radians())
}

/// Standard views: `(yaw, pitch)` presets (Z-up). Top and Bottom are
/// EXACTLY vertical; [`OrbitCamera::up_vector`] keeps lookAt from
/// degenerating there.
pub fn standard_view(name: &str) -> Option<(f64, f64)> {
    let r = f64::to_radians;
    Some(match name {
        "top" => (r(-90.0), r(90.0)),
        "bottom" => (r(-90.0), r(-90.0)),
        "front" => (r(-90.0), 0.0),
        "back" => (r(90.0), 0.0),
        "right" => (0.0, 0.0),
        "left" => (r(180.0), 0.0),
        "iso" => (r(-45.0), r(30.0)),
        _ => return None,
    })
}

impl OrbitCamera {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- Derived state ------------------------------------------------------

    pub fn eye(&self) -> DVec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target
            + DVec3::new(
                self.distance * cp * cy,
                self.distance * cp * sy,
                self.distance * sp,
            )
    }

    /// Unit view direction, eye → target.
    pub fn forward(&self) -> DVec3 {
        let d = self.target - self.eye();
        if d.length_squared() > 1e-18 {
            d.normalize()
        } else {
            DVec3::Y
        }
    }

    /// The up vector every projection must use: `up` itself, unless the
    /// sight line runs along it (exact Top and Bottom), where it is the
    /// limit of the view a hair short of vertical.
    pub fn up_vector(&self) -> DVec3 {
        let f = self.forward();
        if f.dot(self.up).abs() < 0.9999 {
            return self.up;
        }
        let sign = if self.pitch > 0.0 { -1.0 } else { 1.0 };
        DVec3::new(sign * self.yaw.cos(), sign * self.yaw.sin(), 0.0)
    }

    // ---- First person -------------------------------------------------------

    /// Put the eye at `eye` looking along `direction` (any length). A
    /// near-vertical look keeps just off the pole, like [`Self::orbit`].
    pub fn look_from(&mut self, eye: DVec3, direction: DVec3) {
        let mut d = direction;
        if d.length_squared() < 1e-18 {
            d = self.forward();
        }
        let d = d.normalize();
        let pitch = clamp_pitch((-d.z).clamp(-1.0, 1.0).asin());
        let yaw = if pitch.cos().abs() > 1e-9 {
            (-d.y).atan2(-d.x)
        } else {
            self.yaw
        };
        self.yaw = yaw;
        self.pitch = pitch;
        self.target = eye + d * self.distance;
    }

    /// Turn the head: the eye stays put, the look swings `d_yaw_deg` to the
    /// right and `d_pitch_deg` upward.
    pub fn turn(&mut self, d_yaw_deg: f64, d_pitch_deg: f64) {
        let eye = self.eye();
        let d = self.forward();
        let yaw = d.y.atan2(d.x) - d_yaw_deg.to_radians();
        let pitch = clamp_pitch(d.z.clamp(-1.0, 1.0).asin() + d_pitch_deg.to_radians());
        let cp = pitch.cos();
        self.look_from(eye, DVec3::new(cp * yaw.cos(), cp * yaw.sin(), pitch.sin()));
    }

    pub fn view_matrix(&self) -> DMat4 {
        DMat4::look_at_rh(self.eye(), self.target, self.up_vector())
    }

    /// The perspective near plane: it follows the camera in when it comes
    /// right up to something.
    pub fn near(&self) -> f64 {
        self.znear.min((self.distance * 0.02).max(1e-4))
    }

    pub fn projection_matrix(&self) -> DMat4 {
        if self.perspective {
            DMat4::perspective_rh_gl(
                self.fov_deg.to_radians(),
                self.aspect,
                self.near(),
                self.zfar,
            )
        } else {
            let half_h = self.distance * (self.fov_deg.to_radians() / 2.0).tan();
            let half_w = half_h * self.aspect;
            DMat4::orthographic_rh_gl(-half_w, half_w, -half_h, half_h, -self.zfar, self.zfar)
        }
    }

    /// The part of segment `a`–`b` between the near and far planes, cut in
    /// `f64`, or `None` when none of it is. The GPU clips in `f32`: on a
    /// segment kilometres long that crosses the eye's plane, its cut lands
    /// centimetres off, which a near plane of a few millimetres turns into
    /// a line flung across the screen. The cut keeps 1 % inside the near
    /// plane so the GPU has nothing left to clip there.
    pub fn clip_to_depth(&self, a: DVec3, b: DVec3) -> Option<(DVec3, DVec3)> {
        let (lo, hi) = if self.perspective {
            (self.near() * 1.01, self.zfar)
        } else {
            (-self.zfar, self.zfar)
        };
        let (eye, f) = (self.eye(), self.forward());
        let (da, db) = ((a - eye).dot(f), (b - eye).dot(f));
        // The range of t in [0, 1] whose depth da + t (db - da) is in [lo, hi].
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let dd = db - da;
        for (bound, keep_above) in [(lo, true), (hi, false)] {
            if dd.abs() < 1e-12 {
                let inside = if keep_above { da >= bound } else { da <= bound };
                if !inside {
                    return None;
                }
                continue;
            }
            let t = (bound - da) / dd;
            if keep_above == (dd > 0.0) {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
        (t0 < t1).then(|| (a + (b - a) * t0, a + (b - a) * t1))
    }

    // ---- Navigation ---------------------------------------------------------

    /// Turn the view about the target, the centre of the view, Blender
    /// style: both axes grab the model (drag down and it tips down — you
    /// come up over it), the same gesture as [`Self::pan`].
    pub fn orbit(&mut self, dx_pixels: f64, dy_pixels: f64, viewport_h: i32) {
        let scale = std::f64::consts::PI / viewport_h.max(1) as f64;
        self.yaw -= dx_pixels * scale;
        self.pitch = clamp_pitch(self.pitch + dy_pixels * scale);
    }

    /// Slide the view by a drag of `(dx, dy)` pixels. `depth` is how far in
    /// front of the eye the grabbed point lies (perspective only), so it
    /// stays under the cursor.
    pub fn pan(&mut self, dx_pixels: f64, dy_pixels: f64, viewport_h: i32, depth: Option<f64>) {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let forward = DVec3::new(-cp * cy, -cp * sy, -sp);
        let right = crate::math::VecExt::unit_or_zero(forward.cross(self.up_vector()));
        let screen_up = crate::math::VecExt::unit_or_zero(right.cross(forward));
        let mut span = self.distance;
        if let Some(d) = depth {
            if self.perspective && d.is_finite() && d > 1e-6 {
                span = d;
            }
        }
        let world_per_pixel =
            2.0 * span * (self.fov_deg.to_radians() / 2.0).tan() / viewport_h.max(1) as f64;
        self.target -= right * (dx_pixels * world_per_pixel);
        self.target += screen_up * (dy_pixels * world_per_pixel);
    }

    pub fn zoom(&mut self, steps: f64) {
        let factor = 0.9f64.powf(steps);
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Zoom keeping the world point `focus` fixed on screen. Zooming in
    /// keeps sliding toward the focus at the distance floor; zooming out
    /// retreats the eye by at least `min_step`.
    pub fn zoom_to(&mut self, steps: f64, focus: DVec3, min_step: f64) {
        let factor = 0.9f64.powf(steps);
        let new_distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        let eye_before = self.eye();
        self.target = focus + (self.target - focus) * factor;
        self.distance = new_distance;
        if steps < 0.0 && min_step > 0.0 {
            let moved = (self.eye() - eye_before).length();
            if moved < min_step {
                let back = self.eye() - self.target;
                if back.length() > 1e-9 {
                    self.target += back.normalize() * (min_step - moved);
                }
            }
        }
    }

    pub fn set_aspect(&mut self, w: i32, h: i32) {
        self.aspect = w.max(1) as f64 / h.max(1) as f64;
    }

    pub fn toggle_projection(&mut self) {
        self.perspective = !self.perspective;
    }

    // ---- Navigation presets ------------------------------------------------

    /// Center the camera on the AABB and back up enough to frame its
    /// bounding sphere.
    pub fn fit_to(&mut self, min_pt: DVec3, max_pt: DVec3, margin: f64) {
        let center = (min_pt + max_pt) * 0.5;
        let diag = (max_pt - min_pt).length().max(MIN_DISTANCE);
        self.target = center;
        let fov = self.fov_deg.to_radians();
        self.distance = (diag * margin / (2.0 * (fov / 2.0).tan())).max(MIN_DISTANCE);
    }

    /// Zoom Extents: frame the box as the camera looks at it NOW (default
    /// margin 1.1).
    pub fn fit_box(&mut self, min_pt: DVec3, max_pt: DVec3, margin: f64) {
        let f = self.forward();
        let r = f.cross(self.up_vector());
        if r.length_squared() < 1e-18 {
            self.fit_to(min_pt, max_pt, 1.3);
            return;
        }
        let r = r.normalize();
        let u = r.cross(f);
        let corners = box_corners(min_pt, max_pt);
        let center = (min_pt + max_pt) * 0.5;
        let pr: Vec<f64> = corners.iter().map(|c| (*c - center).dot(r)).collect();
        let pu: Vec<f64> = corners.iter().map(|c| (*c - center).dot(u)).collect();
        let (pr_lo, pr_hi) = min_max(&pr);
        let (pu_lo, pu_hi) = min_max(&pu);
        let target = center + r * ((pr_hi + pr_lo) * 0.5) + u * ((pu_hi + pu_lo) * 0.5);
        let half_w = (pr_hi - pr_lo) * 0.5;
        let half_h = (pu_hi - pu_lo) * 0.5;
        let t = (self.fov_deg.to_radians() / 2.0).tan();
        let aspect = if self.aspect > 1e-6 { self.aspect } else { 1.0 };
        let half = half_h.max(half_w / aspect).max(MIN_DISTANCE) * margin;
        let mut distance = half / t;
        if self.perspective {
            let toward = corners
                .iter()
                .map(|c| -(*c - target).dot(f))
                .fold(f64::NEG_INFINITY, f64::max);
            distance += toward.max(0.0);
        }
        self.target = target;
        self.distance = distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
        if self.perspective {
            self.settle_perspective_fit(&corners, margin);
        }
    }

    /// Measure the real projection: centre the box on screen, then walk
    /// the distance in until its wider half-span sits at `1 / margin` of
    /// the window.
    fn settle_perspective_fit(&mut self, corners: &[DVec3], margin: f64) {
        let goal = 1.0 / margin;
        let t = (self.fov_deg.to_radians() / 2.0).tan();
        for _ in 0..8 {
            let mvp = self.projection_matrix() * self.view_matrix();
            let mut xs = Vec::with_capacity(corners.len());
            let mut ys = Vec::with_capacity(corners.len());
            for c in corners {
                let p = mvp * DVec4::new(c.x, c.y, c.z, 1.0);
                if p.w <= 1e-9 {
                    return;
                }
                xs.push(p.x / p.w);
                ys.push(p.y / p.w);
            }
            let (x_lo, x_hi) = min_max(&xs);
            let (y_lo, y_hi) = min_max(&ys);
            let cx = (x_hi + x_lo) * 0.5;
            let cy = (y_hi + y_lo) * 0.5;
            let reach = ((x_hi - x_lo) * 0.5).max((y_hi - y_lo) * 0.5);
            if reach <= 0.0 {
                return;
            }
            if (reach - goal).abs() < 0.005 && cx.abs() < 0.005 && cy.abs() < 0.005 {
                return;
            }
            let f = self.forward();
            let r = crate::math::VecExt::unit_or_zero(f.cross(self.up_vector()));
            let u = r.cross(f);
            let half_h = self.distance * t;
            self.target += r * (cx * half_h * self.aspect) + u * (cy * half_h);
            let nearest = corners
                .iter()
                .map(|c| -(*c - self.target).dot(f))
                .fold(f64::NEG_INFINITY, f64::max);
            let gap = self.distance - nearest;
            self.distance = (nearest + gap * reach / goal).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
    }

    /// Apply a standard view preset; unknown names are ignored.
    pub fn set_view(&mut self, name: &str) {
        if let Some((yaw, pitch)) = standard_view(name) {
            self.yaw = yaw;
            self.pitch = pitch;
        }
    }
}

fn box_corners(lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    let mut out = Vec::with_capacity(8);
    for x in [lo.x, hi.x] {
        for y in [lo.y, hi.y] {
            for z in [lo.z, hi.z] {
                out.push(DVec3::new(x, y, z));
            }
        }
    }
    out
}

fn min_max(v: &[f64]) -> (f64, f64) {
    v.iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        })
}
