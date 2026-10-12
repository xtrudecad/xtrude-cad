// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The viewport's preferences.

use xtrude_model::settings::{self, keys};

use super::{NavStyle, RestMode};

/// How the viewport picks, snaps and navigates, and how it draws: the
/// pixel thresholds, the inference angle, the navigation style, the wheel
/// and orbit directions,
/// anti-aliasing, and how the model outside an open group reads.
///
/// The thresholds are fixed; the rest is read from the settings when the
/// viewport is made, and the Preferences dialog sets it.
#[derive(Debug)]
pub struct ViewPrefs {
    pub snap_threshold_px: f64,
    pub edge_snap_threshold_px: f64,
    pub(crate) pick_threshold_px: f64,
    pub(crate) inference_angle_deg: f64,
    /// Written to the settings by [`super::Viewport::set_edit_rest_mode`].
    pub(super) edit_rest_mode: RestMode,
    pub invert_wheel: bool,
    pub invert_orbit_y: bool,
    /// Which buttons and modifiers move the camera.
    pub nav_style: NavStyle,
    /// Anti-aliasing samples (Preferences; 0 = off).
    pub msaa: u32,
}

impl ViewPrefs {
    /// The preferences as the settings hold them.
    pub fn load() -> ViewPrefs {
        ViewPrefs {
            snap_threshold_px: 9.0,
            edge_snap_threshold_px: 14.0,
            pick_threshold_px: 8.0,
            inference_angle_deg: 3.0,
            edit_rest_mode: load_edit_rest_mode(),
            invert_wheel: settings::read(&keys::INVERT_WHEEL),
            invert_orbit_y: settings::read(&keys::INVERT_ORBIT_Y),
            nav_style: NavStyle::from_key(&settings::read(&keys::NAV_STYLE)),
            msaa: crate::window::preferences::saved_msaa(),
        }
    }
}

fn load_edit_rest_mode() -> RestMode {
    RestMode::from_key(&settings::read(&keys::EDIT_REST_MODE)).unwrap_or(RestMode::Fade)
}
