// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Face attributes, typed.
//!
//! Keys the engine knows: `color` (RGB floats 0–1), `opacity`, `mat` (the
//! material registry name), `back` (`True` = the back mirrors the front, a
//! dict = the back's own paint), `layer`, `hidden`. [`PAINT_KEYS`] — colour,
//! material and opacity — is what paint inheritance carries.

/// RGB floats 0–1.
pub type Rgb = [f64; 3];

/// The paint of one side of a face (`PAINT_KEYS = ("color", "mat", "opacity")`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paint {
    pub color: Option<Rgb>,
    pub mat: Option<String>,
    pub opacity: Option<f64>,
}

impl Paint {
    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.mat.is_none() && self.opacity.is_none()
    }
}

/// What the back of a face shows when it is not the style's default.
#[derive(Clone, Debug, PartialEq)]
pub enum Back {
    /// `attrs["back"] = True`: the back mirrors the front.
    Mirror,
    /// `attrs["back"] = {...}`: the back's own paint.
    Own(Paint),
}

/// The attributes a face carries through the engine's face churn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceAttrs {
    pub paint: Paint,
    pub back: Option<Back>,
    pub layer: Option<String>,
    pub hidden: bool,
    /// The OBJ smoothing group a face came in with (the `"_s"` key):
    /// private to the import, cleared before the model is anyone's.
    pub smooth: Option<String>,
}

/// The names of the paint keys, for code that reports them.
pub const PAINT_KEYS: [&str; 3] = ["color", "mat", "opacity"];

impl FaceAttrs {
    /// Whether nothing is set at all.
    pub fn is_empty(&self) -> bool {
        self.paint.is_empty()
            && self.back.is_none()
            && self.layer.is_none()
            && !self.hidden
            && self.smooth.is_none()
    }

    /// `attrs.update(other)` — keys present in `other` overwrite.
    pub fn update(&mut self, other: &FaceAttrs) {
        if other.paint.color.is_some() {
            self.paint.color = other.paint.color;
        }
        if other.paint.mat.is_some() {
            self.paint.mat = other.paint.mat.clone();
        }
        if other.paint.opacity.is_some() {
            self.paint.opacity = other.paint.opacity;
        }
        if other.back.is_some() {
            self.back = other.back.clone();
        }
        if other.layer.is_some() {
            self.layer = other.layer.clone();
        }
        if other.hidden {
            self.hidden = true;
        }
        if other.smooth.is_some() {
            self.smooth = other.smooth.clone();
        }
    }

    pub fn color(&self) -> Option<Rgb> {
        self.paint.color
    }

    pub fn opacity(&self) -> Option<f64> {
        self.paint.opacity
    }
}
