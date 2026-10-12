// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The DRAWING AXES: the one red/green/blue every tool and inference reads.
//!
//! At the top level they are the world's. Inside a group or component they
//! are that context's own axes. The viewport calls [`sync`] with
//! the scene's drawing frame before it asks the engine anything.
//!
//! Design note: they are thread-local, not process-wide (tests run on
//! parallel threads).

use std::cell::RefCell;

use crate::math::{DMat4, DVec3, DVec4, MatExt, VecExt};

#[derive(Clone, Copy, Debug, PartialEq)]
struct AxesState {
    x: DVec3,
    y: DVec3,
    z: DVec3,
    origin: DVec3,
    current: Option<DMat4>,
}

const WORLD: AxesState = AxesState {
    x: DVec3::X,
    y: DVec3::Y,
    z: DVec3::Z,
    origin: DVec3::ZERO,
    current: None,
};

thread_local! {
    static AXES: RefCell<AxesState> = const { RefCell::new(WORLD) };
}

pub fn is_world() -> bool {
    AXES.with(|a| a.borrow().current.is_none())
}

pub fn origin() -> DVec3 {
    AXES.with(|a| a.borrow().origin)
}

/// One of the three drawing axes: red, green, blue. Ordered X < Y < Z,
/// which the snapping tie-breaks rely on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// All three, in order.
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// The current drawing axis as a unit vector (see [`axis`]).
    pub fn vector(self) -> DVec3 {
        axis(self)
    }

    /// The world axis as a unit vector, whatever the drawing axes are.
    pub fn world(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }

    /// The axis name as shown to the user.
    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }

    /// The axis colour: red, green or blue.
    pub fn color(self) -> crate::snap::Rgb {
        match self {
            Axis::X => crate::snap::COLOR_AXIS_X,
            Axis::Y => crate::snap::COLOR_AXIS_Y,
            Axis::Z => crate::snap::COLOR_AXIS_Z,
        }
    }

    /// `'x'`, `'y'` or `'z'`: the spelling in documents (a dimension's
    /// `"axis"`) and in action ids (`flip_along:x`).
    pub fn as_char(self) -> char {
        match self {
            Axis::X => 'x',
            Axis::Y => 'y',
            Axis::Z => 'z',
        }
    }

    /// The axis spelled `'x'`, `'y'` or `'z'`.
    pub fn from_char(c: char) -> Option<Axis> {
        match c {
            'x' => Some(Axis::X),
            'y' => Some(Axis::Y),
            'z' => Some(Axis::Z),
            _ => None,
        }
    }
}

/// The current drawing axis as a unit vector.
pub fn axis(name: Axis) -> DVec3 {
    AXES.with(|a| {
        let s = a.borrow();
        match name {
            Axis::X => s.x,
            Axis::Y => s.y,
            Axis::Z => s.z,
        }
    })
}

/// `(origin, x, y, z)` of a frame matrix, axes made unit and square to each
/// other (red first, green squared to it, blue from both; a mirrored
/// placement keeps its handedness). `None` is the world.
pub fn frame_axes(m: Option<&DMat4>) -> (DVec3, DVec3, DVec3, DVec3) {
    let Some(m) = m else {
        return (DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z);
    };
    let o = m.project_point(DVec3::ZERO);
    let mut x = m.transform_vector(DVec3::X);
    let mut y = m.transform_vector(DVec3::Y);
    let z = m.transform_vector(DVec3::Z);
    if x.length() < 1e-12 {
        x = DVec3::X;
    }
    x = x.unit_or_zero();
    y -= x * y.dot(x);
    if y.length() < 1e-12 {
        y = z.cross(x);
    }
    y = y.unit_or_zero();
    let mut zz = x.cross(y).unit_or_zero();
    if zz.dot(z) < 0.0 {
        zz = -zz;
    }
    (o, x, y, zz)
}

/// Make the drawing axes `frame`'s, or the world's for `None`.
pub fn sync(frame: Option<&DMat4>) {
    AXES.with(|a| {
        let mut s = a.borrow_mut();
        if s.current.as_ref() == frame {
            return;
        }
        match frame {
            None => *s = WORLD,
            Some(m) => {
                let (o, x, y, z) = frame_axes(Some(m));
                *s = AxesState {
                    x,
                    y,
                    z,
                    origin: o,
                    current: Some(*m),
                };
            }
        }
    });
}

/// A world DIRECTION in drawing-axes components.
pub fn to_local(v: DVec3) -> DVec3 {
    DVec3::new(
        v.dot(axis(Axis::X)),
        v.dot(axis(Axis::Y)),
        v.dot(axis(Axis::Z)),
    )
}

/// Drawing-axes components back to a world direction.
pub fn to_world(v: DVec3) -> DVec3 {
    axis(Axis::X) * v.x + axis(Axis::Y) * v.y + axis(Axis::Z) * v.z
}

/// `triangulate::plane_axes` for DRAWING: the in-plane `u` is the drawing
/// red projected (green when the plane faces red).
pub fn plane_axes(normal: DVec3) -> (DVec3, DVec3) {
    if is_world() {
        return crate::triangulate::plane_axes(normal);
    }
    let n = normal.unit_or_zero();
    let mut reference = axis(Axis::X);
    let mut u = reference - n * reference.dot(n);
    if u.length() < 0.1 {
        reference = axis(Axis::Y);
        u = reference - n * reference.dot(n);
    }
    let u = u.unit_or_zero();
    let v = n.cross(u).unit_or_zero();
    (u, v)
}

/// The world matrix whose columns are `x`, `y`, `z` and whose translation
/// is `o`.
pub fn frame_matrix(o: DVec3, x: DVec3, y: DVec3, z: DVec3) -> DMat4 {
    DMat4::from_cols(
        x.extend(0.0),
        y.extend(0.0),
        z.extend(0.0),
        DVec4::new(o.x, o.y, o.z, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_spelling_round_trips() {
        for a in Axis::ALL {
            assert_eq!(Axis::from_char(a.as_char()), Some(a));
        }
        assert_eq!(Axis::X.as_char(), 'x');
        assert_eq!(Axis::Y.as_char(), 'y');
        assert_eq!(Axis::Z.as_char(), 'z');
        assert_eq!(Axis::from_char('X'), None);
        assert_eq!(Axis::from_char('w'), None);
    }

    #[test]
    fn axes_order_like_their_letters() {
        assert!(Axis::X < Axis::Y && Axis::Y < Axis::Z);
        assert_eq!(Axis::ALL, [Axis::X, Axis::Y, Axis::Z]);
    }

    #[test]
    fn axis_vectors_names_and_colours() {
        assert_eq!(Axis::Y.world(), DVec3::Y);
        assert_eq!(Axis::Z.vector(), DVec3::Z);
        assert_eq!(Axis::X.name(), "X");
        assert_eq!(Axis::Y.color(), crate::snap::COLOR_AXIS_Y);
    }
}
