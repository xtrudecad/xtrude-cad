// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Vector and matrix helpers over `glam`'s `f64` types.
//!
//! The model is `f64`. A few helpers here behave differently from glam's
//! own, on purpose, and the engine's tolerances were tuned against them:
//! [`VecExt::unit_or_zero`] has its own zero and unit bands,
//! [`rotation_deg`] makes quarter turns exact, and
//! [`MatExt::project_point`] divides by `w`.

pub use glam::{DMat3, DMat4, DQuat, DVec2, DVec3, DVec4};

/// The band, around 0 and around 1, inside which [`VecExt::unit_or_zero`]
/// treats a length as zero or as already unit.
pub const FUZZY_ZERO: f64 = 1e-5;

/// The smallest area (m²) a loop needs to be a face or a hole: the square
/// of the 0.1 mm length tolerance the plane rebuilds snap points to
/// (0.01 mm²). A smaller loop is a sliver between points that are one
/// tolerance apart; a screw hole of a few mm² is well above it.
pub const MIN_FACE_AREA: f64 = 1e-4 * 1e-4;

/// Shorthand constructor.
#[inline]
pub const fn v3(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// Vector helpers.
pub trait VecExt {
    /// The unit vector along `self`, with two bands of [`FUZZY_ZERO`]:
    /// `self` unchanged when its length is within it of 1, and the zero
    /// vector when its length is at most it. Callers guard on the zero
    /// result to detect degenerate input (a collapsed edge, a sliver
    /// normal), so the bands are part of the contract; glam's
    /// `normalize_or_zero` / `try_normalize` use other thresholds.
    fn unit_or_zero(self) -> Self;
    /// Rounded key used by tests and topology (`round(x, n)` per component).
    fn rounded(self, decimals: i32) -> (f64, f64, f64);
}

impl VecExt for DVec3 {
    #[inline]
    fn unit_or_zero(self) -> Self {
        let len = self.length();
        if (len - 1.0).abs() <= FUZZY_ZERO {
            self
        } else if len <= FUZZY_ZERO {
            DVec3::ZERO
        } else {
            self / len
        }
    }

    fn rounded(self, decimals: i32) -> (f64, f64, f64) {
        (
            round_even(self.x, decimals),
            round_even(self.y, decimals),
            round_even(self.z, decimals),
        )
    }
}

/// `x` rounded to `n` decimals, half to even on the decimal scaling.
///
/// Rounding the exact binary value would differ only at representation
/// edges that do not matter for the keys this is used for.
pub fn round_even(x: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    let y = x * scale;
    let r = y.round();
    // Half-way cases go to even.
    let rounded = if (y - y.trunc()).abs() == 0.5 {
        2.0 * (y / 2.0).round()
    } else {
        r
    };
    let out = rounded / scale;
    if out == 0.0 {
        0.0 // fold -0.0 so keys compare equal
    } else {
        out
    }
}

/// `x` rounded to an integer, half to even (banker's rounding).
pub fn round_even_int(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Matrix helpers over `DMat4`.
pub trait MatExt {
    /// Transform a point, dividing by `w` unless it is 0 or 1, so the
    /// same call serves affine transforms and projections.
    fn project_point(&self, p: DVec3) -> DVec3;
    /// Transform a direction: the upper 3×3 only (no translation).
    fn transform_vector(&self, v: DVec3) -> DVec3;
    /// The inverse, or `None` when the determinant is below 1e-300 in
    /// magnitude or not finite.
    fn try_inverse(&self) -> Option<DMat4>;
    /// The 16 values, column-major.
    fn data(&self) -> [f64; 16];
    /// Build from `data()`-ordered (column-major) values.
    fn from_data(vals: &[f64; 16]) -> DMat4;
    /// Whether every element is within `eps` of `other`'s.
    fn fuzzy_eq(&self, other: &DMat4, eps: f64) -> bool;
}

impl MatExt for DMat4 {
    #[inline]
    fn project_point(&self, p: DVec3) -> DVec3 {
        let r = *self * p.extend(1.0);
        if r.w == 1.0 || r.w == 0.0 {
            r.truncate()
        } else {
            r.truncate() / r.w
        }
    }

    #[inline]
    fn transform_vector(&self, v: DVec3) -> DVec3 {
        self.transform_vector3(v)
    }

    fn try_inverse(&self) -> Option<DMat4> {
        let det = self.determinant();
        if det.abs() < 1e-300 || !det.is_finite() {
            return None;
        }
        Some(self.inverse())
    }

    fn data(&self) -> [f64; 16] {
        self.to_cols_array()
    }

    fn from_data(vals: &[f64; 16]) -> DMat4 {
        DMat4::from_cols_array(vals)
    }

    fn fuzzy_eq(&self, other: &DMat4, eps: f64) -> bool {
        self.to_cols_array()
            .iter()
            .zip(other.to_cols_array().iter())
            .all(|(a, b)| (a - b).abs() <= eps)
    }
}

/// A rotation of `degrees` about `axis` through the origin. Multiples of
/// 90° use exact sines and cosines (no 6e-17 residue), which keeps rotated
/// geometry on its grid. A zero-length axis gives the identity.
pub fn rotation_deg(axis: DVec3, degrees: f64) -> DMat4 {
    let (s, c) = if degrees == 90.0 || degrees == -270.0 {
        (1.0, 0.0)
    } else if degrees == -90.0 || degrees == 270.0 {
        (-1.0, 0.0)
    } else if degrees == 180.0 || degrees == -180.0 {
        (0.0, -1.0)
    } else if degrees == 0.0 || degrees == 360.0 || degrees == -360.0 {
        (0.0, 1.0)
    } else {
        let a = degrees.to_radians();
        (a.sin(), a.cos())
    };
    let len = axis.length();
    if len < 1e-300 {
        return DMat4::IDENTITY;
    }
    let (x, y, z) = (axis.x / len, axis.y / len, axis.z / len);
    let ic = 1.0 - c;
    // Column-major construction of the standard axis-angle matrix.
    DMat4::from_cols(
        DVec4::new(x * x * ic + c, y * x * ic + z * s, x * z * ic - y * s, 0.0),
        DVec4::new(x * y * ic - z * s, y * y * ic + c, y * z * ic + x * s, 0.0),
        DVec4::new(x * z * ic + y * s, y * z * ic - x * s, z * z * ic + c, 0.0),
        DVec4::new(0.0, 0.0, 0.0, 1.0),
    )
}

/// `m * translation(t)`: the translation is applied first, in `m`'s local
/// frame.
pub fn translated(m: DMat4, t: DVec3) -> DMat4 {
    m * DMat4::from_translation(t)
}

/// A rotation about an axis through `center`: translate(c) · rotate ·
/// translate(-c).
pub fn rotation_about(center: DVec3, axis: DVec3, degrees: f64) -> DMat4 {
    DMat4::from_translation(center) * rotation_deg(axis, degrees) * DMat4::from_translation(-center)
}

/// Newell's (un-normalised) normal of a closed polygon, in double precision.
pub fn newell(points: &[DVec3]) -> DVec3 {
    let n = points.len();
    let mut acc = DVec3::ZERO;
    for i in 0..n {
        let c = points[i];
        let nx = points[(i + 1) % n];
        acc.x += (c.y - nx.y) * (c.z + nx.z);
        acc.y += (c.z - nx.z) * (c.x + nx.x);
        acc.z += (c.x - nx.x) * (c.y + nx.y);
    }
    acc
}

/// Eigenvectors of a symmetric 3×3 matrix (cyclic Jacobi), as unit row
/// vectors, up to order and sign.
// The rotations index rows and columns together, as the algorithm is written.
#[allow(clippy::needless_range_loop)]
pub fn symmetric_eigenvectors(m: [[f64; 3]; 3]) -> [DVec3; 3] {
    let mut a = m;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _sweep in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-15 {
            break;
        }
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let akp = a[k][p];
                let akq = a[k][q];
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let apk = a[p][k];
                let aqk = a[q][k];
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for row in v.iter_mut() {
                let vkp = row[p];
                let vkq = row[q];
                row[p] = c * vkp - s * vkq;
                row[q] = s * vkp + c * vkq;
            }
        }
    }
    // Columns of v are the eigenvectors.
    [
        DVec3::new(v[0][0], v[1][0], v[2][0]).normalize_or_zero(),
        DVec3::new(v[0][1], v[1][1], v[2][1]).normalize_or_zero(),
        DVec3::new(v[0][2], v[1][2], v[2][2]).normalize_or_zero(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_or_zero_bands() {
        assert_eq!(v3(1e-6, 0.0, 0.0).unit_or_zero(), DVec3::ZERO);
        assert_eq!(v3(2.0, 0.0, 0.0).unit_or_zero(), v3(1.0, 0.0, 0.0));
        // The zero band: at most FUZZY_ZERO is zero, just above is scaled.
        assert_eq!(v3(0.0, 0.0, 0.99e-5).unit_or_zero(), DVec3::ZERO);
        assert_eq!(v3(0.0, 0.0, 1e-5).unit_or_zero(), DVec3::ZERO);
        assert_eq!(v3(0.0, 0.0, 1.01e-5).unit_or_zero(), v3(0.0, 0.0, 1.0));
        // The unit band: a length within FUZZY_ZERO of 1 is left as it is.
        let near = v3(0.0, 1.0 + 0.9e-5, 0.0);
        assert_eq!(near.unit_or_zero(), near);
        let near = v3(0.6, 0.8, 0.0) * (1.0 - 0.9e-5);
        assert_eq!(near.unit_or_zero(), near);
        let outside = v3(0.0, 1.0 + 2e-5, 0.0);
        assert_eq!(outside.unit_or_zero(), v3(0.0, 1.0, 0.0));
    }

    #[test]
    fn try_inverse_of_singular_is_none() {
        let flat = DMat4::from_scale(v3(1.0, 1.0, 0.0));
        assert_eq!(flat.try_inverse(), None);
        assert_eq!(DMat4::ZERO.try_inverse(), None);
        let nan = DMat4::from_translation(v3(f64::NAN, 0.0, 0.0)) * flat;
        assert_eq!(nan.try_inverse(), None);
        let m = DMat4::from_translation(v3(1.0, 2.0, 3.0)) * DMat4::from_scale(v3(2.0, 2.0, 2.0));
        let inv = m.try_inverse().expect("invertible");
        assert!((m * inv).fuzzy_eq(&DMat4::IDENTITY, 1e-15));
    }

    #[test]
    fn project_point_divides_by_w() {
        let t = DMat4::from_translation(v3(1.0, 2.0, 3.0));
        assert_eq!(t.project_point(v3(1.0, 1.0, 1.0)), v3(2.0, 3.0, 4.0));
        let mut m = DMat4::IDENTITY;
        m.w_axis.w = 2.0;
        assert_eq!(m.project_point(v3(2.0, 4.0, 6.0)), v3(1.0, 2.0, 3.0));
        assert_eq!(t.transform_vector(v3(1.0, 1.0, 1.0)), v3(1.0, 1.0, 1.0));
    }

    #[test]
    fn quarter_turns_are_exact() {
        let m = rotation_deg(DVec3::Z, 90.0);
        assert_eq!(m.project_point(v3(1.0, 0.0, 0.0)), v3(0.0, 1.0, 0.0));
        let m = rotation_deg(DVec3::Z, 180.0);
        assert_eq!(m.project_point(v3(1.0, 2.0, 0.0)), v3(-1.0, -2.0, 0.0));
    }

    #[test]
    fn rotation_matches_glam_for_generic_angles() {
        let m = rotation_deg(v3(1.0, 2.0, 3.0), 33.0);
        let q = DMat4::from_axis_angle(v3(1.0, 2.0, 3.0).normalize(), 33f64.to_radians());
        assert!(m.fuzzy_eq(&q, 1e-12));
    }

    #[test]
    fn round_even_is_bankers() {
        assert_eq!(round_even_int(0.5), 0);
        assert_eq!(round_even_int(1.5), 2);
        assert_eq!(round_even_int(2.5), 2);
        assert_eq!(round_even_int(-0.5), 0);
        assert_eq!(round_even(1.23456, 2), 1.23);
    }

    #[test]
    fn round_even_halves_on_the_decimal_scale() {
        assert_eq!(round_even(0.125, 2), 0.12);
        assert_eq!(round_even(0.375, 2), 0.38);
        assert_eq!(round_even(-0.125, 2), -0.12);
        assert_eq!(round_even(2.5, 0), 2.0);
        assert_eq!(round_even(3.5, 0), 4.0);
        // 0.135 is stored slightly above 0.135 and scales to exactly 13.5,
        // so it rounds as a half.
        assert_eq!(round_even(0.135, 2), 0.14);
    }

    #[test]
    fn round_even_folds_negative_zero() {
        let r = round_even(-0.001, 2);
        assert_eq!(r, 0.0);
        assert!(r.is_sign_positive());
        assert!(round_even(-0.0, 3).is_sign_positive());
    }

    /// The body `round_even_int` had before it used `round_ties_even`.
    fn round_even_int_by_hand(x: f64) -> i64 {
        let r = x.round();
        if (x - x.trunc()).abs() == 0.5 {
            (2.0 * (x / 2.0).round()) as i64
        } else {
            r as i64
        }
    }

    #[test]
    fn round_even_int_matches_the_hand_written_rule() {
        let mut xs = vec![
            0.0,
            -0.0,
            0.49999999999999994,
            -0.49999999999999994,
            0.5000000000000001,
            -0.5000000000000001,
            1e15 + 0.5,
            -1e15 - 0.5,
            4503599627370495.5,
            -4503599627370495.5,
            9007199254740993.0,
            1e300,
            -1e300,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        for i in -200..=200 {
            let h = f64::from(i) + 0.5;
            xs.extend([h, h - 1e-9, h + 1e-9, f64::from(i) * 0.37]);
        }
        for x in xs {
            assert_eq!(round_even_int(x), round_even_int_by_hand(x), "x = {x:?}");
        }
    }
}
