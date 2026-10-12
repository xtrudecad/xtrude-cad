// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Polygon triangulation, including polygons with holes.
//!
//! The renderer and the face picker both need triangles, and a face that
//! has been divided by inner loops (a "donut", or a plate with two
//! holes) cannot be fan-triangulated. This module turns an outer loop plus
//! zero or more hole loops into a triangle list, working in the face's own
//! plane.
//!
//! The holed / concave path is a faithful translation of the **earcut**
//! ear-clipping algorithm (Mapbox, ISC license), without z-order hashing.
//! Convex hole-free faces keep a trivial O(n) fan.
//! The doubly linked list lives in an index arena.

use crate::math::{DVec3, VecExt};

pub type Point2 = (f64, f64);
pub type Tri3 = [DVec3; 3];

const EPS: f64 = 1e-9;

/// Two orthonormal in-plane axes derived from a plane normal.
///
/// `u` is world +X projected onto the plane (or +Y when the normal is
/// nearly +X, to avoid the degenerate projection); `v = normal × u`.
pub fn plane_axes(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.unit_or_zero();
    let mut reference = DVec3::X;
    let mut u = reference - n * reference.dot(n);
    if u.length() < 0.1 {
        reference = DVec3::Y;
        u = reference - n * reference.dot(n);
    }
    let u = u.unit_or_zero();
    let v = n.cross(u).unit_or_zero();
    (u, v)
}

fn cross3(o: Point2, a: Point2, b: Point2) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// Whether a simple 2D polygon is convex — every turn goes the same way.
///
/// Collinear vertices (a split point left on a straight edge) are ignored,
/// so a rectangle that gained a mid-edge vertex still counts as convex and
/// keeps the cheap fan path. Triangles are trivially convex.
pub fn is_convex(poly: &[Point2]) -> bool {
    let n = poly.len();
    if n < 4 {
        return true;
    }
    let mut sign = 0i32;
    for i in 0..n {
        let cr = cross3(poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        if cr > EPS {
            if sign < 0 {
                return false;
            }
            sign = 1;
        } else if cr < -EPS {
            if sign > 0 {
                return false;
            }
            sign = -1;
        }
    }
    true
}

fn newell_unit(vertices: &[DVec3]) -> DVec3 {
    let n = crate::math::newell(vertices);
    if n.length() < 1e-9 {
        return DVec3::Z;
    }
    n.unit_or_zero()
}

// ---- earcut (after Mapbox earcut, no z-order hashing) ----------------------

type NodeId = usize;

#[derive(Clone, Copy)]
struct Node {
    i: usize,
    x: f64,
    y: f64,
    prev: NodeId,
    next: NodeId,
    steiner: bool,
}

struct Ear {
    nodes: Vec<Node>,
}

impl Ear {
    fn new() -> Self {
        Ear { nodes: Vec::new() }
    }

    #[inline]
    fn n(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    fn insert_node(&mut self, i: usize, x: f64, y: f64, last: Option<NodeId>) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            i,
            x,
            y,
            prev: id,
            next: id,
            steiner: false,
        });
        if let Some(last) = last {
            let last_next = self.nodes[last].next;
            self.nodes[id].next = last_next;
            self.nodes[id].prev = last;
            self.nodes[last_next].prev = id;
            self.nodes[last].next = id;
        }
        id
    }

    fn remove_node(&mut self, p: NodeId) {
        let (prev, next) = (self.nodes[p].prev, self.nodes[p].next);
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
    }

    #[inline]
    fn area(&self, p: NodeId, q: NodeId, r: NodeId) -> f64 {
        let (p, q, r) = (self.n(p), self.n(q), self.n(r));
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    #[inline]
    fn equals(&self, a: NodeId, b: NodeId) -> bool {
        let (a, b) = (self.n(a), self.n(b));
        a.x == b.x && a.y == b.y
    }

    fn build_ring(&mut self, points: &[Point2], idxs: &[usize], want_ccw: bool) -> Option<NodeId> {
        // signed area > 0 means clockwise here (shoelace with (x2-x1)(y1+y2)).
        let mut s = 0.0;
        let n = idxs.len();
        for k in 0..n {
            let a = points[idxs[k]];
            let b = points[idxs[(k + 1) % n]];
            s += (b.0 - a.0) * (a.1 + b.1);
        }
        let clockwise = s > 0.0;
        let seq: Vec<usize> = if clockwise != want_ccw {
            idxs.to_vec()
        } else {
            idxs.iter().rev().copied().collect()
        };
        let mut last: Option<NodeId> = None;
        for i in seq {
            last = Some(self.insert_node(i, points[i].0, points[i].1, last));
        }
        if let Some(l) = last {
            let nx = self.n(l).next;
            if self.equals(l, nx) {
                self.remove_node(l);
                last = Some(self.n(l).next);
            }
        }
        last
    }

    fn filter_points(&mut self, start: Option<NodeId>, end: Option<NodeId>) -> Option<NodeId> {
        let start = start?;
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let node = *self.n(p);
            if !node.steiner
                && (self.equals(p, node.next) || self.area(node.prev, p, node.next) == 0.0)
            {
                self.remove_node(p);
                p = node.prev;
                end = p;
                if p == self.n(p).next {
                    break;
                }
                again = true;
            } else {
                p = node.next;
            }
            if !again && p == end {
                break;
            }
        }
        Some(end)
    }

    fn is_ear(&self, ear: NodeId) -> bool {
        let a = self.n(ear).prev;
        let b = ear;
        let c = self.n(ear).next;
        if self.area(a, b, c) >= 0.0 {
            return false; // reflex, not an ear
        }
        let (na, nb, nc) = (self.n(a), self.n(b), self.n(c));
        let (ax, bx, cx) = (na.x, nb.x, nc.x);
        let (ay, by, cy) = (na.y, nb.y, nc.y);
        let x0 = ax.min(bx).min(cx);
        let y0 = ay.min(by).min(cy);
        let x1 = ax.max(bx).max(cx);
        let y1 = ay.max(by).max(cy);
        let mut p = nc.next;
        while p != a {
            let np = self.n(p);
            if x0 <= np.x
                && np.x <= x1
                && y0 <= np.y
                && np.y <= y1
                && point_in_triangle(ax, ay, bx, by, cx, cy, np.x, np.y)
                && self.area(np.prev, p, np.next) >= 0.0
            {
                return false;
            }
            p = np.next;
        }
        true
    }

    fn earcut_linked(&mut self, ear: Option<NodeId>, triangles: &mut Vec<[usize; 3]>, pass: u8) {
        let Some(mut ear) = ear else { return };
        let mut stop = ear;
        while self.n(ear).prev != self.n(ear).next {
            let prev = self.n(ear).prev;
            let nxt = self.n(ear).next;
            if self.is_ear(ear) {
                triangles.push([self.n(prev).i, self.n(ear).i, self.n(nxt).i]);
                self.remove_node(ear);
                ear = self.n(nxt).next;
                stop = self.n(nxt).next;
                continue;
            }
            ear = nxt;
            if ear == stop {
                // No ear found — recover from degeneracies.
                match pass {
                    0 => {
                        let f = self.filter_points(Some(ear), None);
                        self.earcut_linked(f, triangles, 1);
                    }
                    1 => {
                        let f = self.filter_points(Some(ear), None);
                        let e = f.and_then(|f| self.cure_local_intersections(f, triangles));
                        self.earcut_linked(e, triangles, 2);
                    }
                    2 => self.split_earcut(ear, triangles),
                    _ => {}
                }
                return;
            }
        }
    }

    fn cure_local_intersections(
        &mut self,
        start: NodeId,
        triangles: &mut Vec<[usize; 3]>,
    ) -> Option<NodeId> {
        let mut start = start;
        let mut p = start;
        loop {
            let a = self.n(p).prev;
            let pn = self.n(p).next;
            let b = self.n(pn).next;
            if !self.equals(a, b)
                && self.intersects(a, p, pn, b)
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                triangles.push([self.n(a).i, self.n(p).i, self.n(b).i]);
                self.remove_node(p);
                let pn2 = self.n(p).next;
                self.remove_node(pn2);
                p = b;
                start = b;
            }
            p = self.n(p).next;
            if p == start {
                break;
            }
        }
        self.filter_points(Some(p), None)
    }

    fn split_earcut(&mut self, start: NodeId, triangles: &mut Vec<[usize; 3]>) {
        let mut a = start;
        loop {
            let mut b = self.n(self.n(a).next).next;
            while b != self.n(a).prev {
                if self.n(a).i != self.n(b).i && self.is_valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let an = self.n(a).next;
                    let a2 = self.filter_points(Some(a), Some(an));
                    let cn = self.n(c).next;
                    let c2 = self.filter_points(Some(c), Some(cn));
                    self.earcut_linked(a2, triangles, 0);
                    self.earcut_linked(c2, triangles, 0);
                    return;
                }
                b = self.n(b).next;
            }
            a = self.n(a).next;
            if a == start {
                break;
            }
        }
    }

    fn eliminate_holes(
        &mut self,
        points: &[Point2],
        hole_indices: &[usize],
        outer: NodeId,
    ) -> NodeId {
        let mut queue: Vec<NodeId> = Vec::new();
        let total = points.len();
        for k in 0..hole_indices.len() {
            let start = hole_indices[k];
            let end = if k + 1 < hole_indices.len() {
                hole_indices[k + 1]
            } else {
                total
            };
            let idxs: Vec<usize> = (start..end).collect();
            if let Some(h) = self.build_ring(points, &idxs, false) {
                if h == self.n(h).next {
                    self.nodes[h].steiner = true;
                }
                queue.push(self.get_leftmost(h));
            }
        }
        queue.sort_by(|&a, &b| {
            let (na, nb) = (self.n(a), self.n(b));
            na.x.partial_cmp(&nb.x)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(na.y.partial_cmp(&nb.y).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut outer = outer;
        for hole in queue {
            outer = self.eliminate_hole(hole, outer);
        }
        outer
    }

    fn eliminate_hole(&mut self, hole: NodeId, outer: NodeId) -> NodeId {
        let Some(bridge) = self.find_hole_bridge(hole, outer) else {
            return outer;
        };
        let bridge_reverse = self.split_polygon(bridge, hole);
        let brn = self.n(bridge_reverse).next;
        self.filter_points(Some(bridge_reverse), Some(brn));
        let bn = self.n(bridge).next;
        self.filter_points(Some(bridge), Some(bn)).unwrap_or(outer)
    }

    fn get_leftmost(&self, start: NodeId) -> NodeId {
        let mut p = start;
        let mut leftmost = start;
        loop {
            let (np, nl) = (self.n(p), self.n(leftmost));
            if np.x < nl.x || (np.x == nl.x && np.y < nl.y) {
                leftmost = p;
            }
            p = np.next;
            if p == start {
                break;
            }
        }
        leftmost
    }

    fn find_hole_bridge(&self, hole: NodeId, outer: NodeId) -> Option<NodeId> {
        let mut p = outer;
        let (hx, hy) = (self.n(hole).x, self.n(hole).y);
        let mut qx = f64::NEG_INFINITY;
        let mut m: Option<NodeId> = None;
        // Find the edge whose intersection with a +x ray from the hole is closest.
        loop {
            let np = self.n(p);
            let nn = self.n(np.next);
            if hy <= np.y && hy >= nn.y && nn.y != np.y {
                let x = np.x + (hy - np.y) * (nn.x - np.x) / (nn.y - np.y);
                if hx >= x && x > qx {
                    qx = x;
                    m = Some(if np.x < nn.x { p } else { np.next });
                    if x == hx {
                        return m;
                    }
                }
            }
            p = np.next;
            if p == outer {
                break;
            }
        }
        let mut m = m?;
        // Look for a reflex vertex inside the candidate triangle, closer in angle.
        let stop = m;
        let (mx, my) = (self.n(m).x, self.n(m).y);
        let mut tan_min = f64::INFINITY;
        let mut p = m;
        loop {
            let np = self.n(p);
            if hx >= np.x
                && np.x >= mx
                && hx != np.x
                && point_in_triangle(
                    if hy < my { hx } else { qx },
                    hy,
                    mx,
                    my,
                    if hy < my { qx } else { hx },
                    hy,
                    np.x,
                    np.y,
                )
            {
                let tan = (hy - np.y).abs() / (hx - np.x);
                let nm = self.n(m);
                if self.locally_inside(p, hole)
                    && (tan < tan_min
                        || (tan == tan_min
                            && (np.x > nm.x || (np.x == nm.x && self.sector_contains(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = np.next;
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    fn sector_contains(&self, m: NodeId, p: NodeId) -> bool {
        self.area(self.n(m).prev, m, self.n(p).prev) < 0.0
            && self.area(self.n(p).next, m, self.n(m).next) < 0.0
    }

    fn locally_inside(&self, a: NodeId, b: NodeId) -> bool {
        let (ap, an) = (self.n(a).prev, self.n(a).next);
        if self.area(ap, a, an) < 0.0 {
            self.area(a, b, an) >= 0.0 && self.area(a, ap, b) >= 0.0
        } else {
            self.area(a, b, ap) < 0.0 || self.area(a, an, b) < 0.0
        }
    }

    fn middle_inside(&self, a: NodeId, b: NodeId) -> bool {
        let mut p = a;
        let mut inside = false;
        let px = (self.n(a).x + self.n(b).x) / 2.0;
        let py = (self.n(a).y + self.n(b).y) / 2.0;
        loop {
            let np = self.n(p);
            let nn = self.n(np.next);
            if ((np.y > py) != (nn.y > py))
                && nn.y != np.y
                && (px < (nn.x - np.x) * (py - np.y) / (nn.y - np.y) + np.x)
            {
                inside = !inside;
            }
            p = np.next;
            if p == a {
                break;
            }
        }
        inside
    }

    fn is_valid_diagonal(&self, a: NodeId, b: NodeId) -> bool {
        let (na, nb) = (self.n(a), self.n(b));
        self.n(na.next).i != nb.i
            && self.n(na.prev).i != nb.i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                && (self.area(na.prev, a, nb.prev) != 0.0 || self.area(a, nb.prev, b) != 0.0))
                || (self.equals(a, b)
                    && self.area(na.prev, a, na.next) > 0.0
                    && self.area(nb.prev, b, nb.next) > 0.0))
    }

    fn on_segment(&self, p: NodeId, q: NodeId, r: NodeId) -> bool {
        let (p, q, r) = (self.n(p), self.n(q), self.n(r));
        p.x.max(r.x) >= q.x && q.x >= p.x.min(r.x) && p.y.max(r.y) >= q.y && q.y >= p.y.min(r.y)
    }

    fn intersects(&self, p1: NodeId, q1: NodeId, p2: NodeId, q2: NodeId) -> bool {
        let o1 = sign(self.area(p1, q1, p2));
        let o2 = sign(self.area(p1, q1, q2));
        let o3 = sign(self.area(p2, q2, p1));
        let o4 = sign(self.area(p2, q2, q1));
        if o1 != o2 && o3 != o4 {
            return true;
        }
        if o1 == 0 && self.on_segment(p1, p2, q1) {
            return true;
        }
        if o2 == 0 && self.on_segment(p1, q2, q1) {
            return true;
        }
        if o3 == 0 && self.on_segment(p2, p1, q2) {
            return true;
        }
        if o4 == 0 && self.on_segment(p2, q1, q2) {
            return true;
        }
        false
    }

    fn intersects_polygon(&self, a: NodeId, b: NodeId) -> bool {
        let mut p = a;
        let (ai, bi) = (self.n(a).i, self.n(b).i);
        loop {
            let np = self.n(p);
            let nni = self.n(np.next).i;
            if np.i != ai
                && nni != ai
                && np.i != bi
                && nni != bi
                && self.intersects(p, np.next, a, b)
            {
                return true;
            }
            p = np.next;
            if p == a {
                break;
            }
        }
        false
    }

    fn split_polygon(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let na = *self.n(a);
        let nb = *self.n(b);
        let a2 = self.nodes.len();
        self.nodes.push(Node {
            i: na.i,
            x: na.x,
            y: na.y,
            prev: a2,
            next: a2,
            steiner: false,
        });
        let b2 = self.nodes.len();
        self.nodes.push(Node {
            i: nb.i,
            x: nb.x,
            y: nb.y,
            prev: b2,
            next: b2,
            steiner: false,
        });
        let an = na.next;
        let bp = nb.prev;
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[a2].next = an;
        self.nodes[an].prev = a2;
        self.nodes[b2].next = a2;
        self.nodes[a2].prev = b2;
        self.nodes[bp].next = b2;
        self.nodes[b2].prev = bp;
        b2
    }
}

#[inline]
fn sign(x: f64) -> i32 {
    (x > 0.0) as i32 - (x < 0.0) as i32
}

// Earcut's innermost test: eight plain coordinates, inlined on the hot path.
#[allow(clippy::too_many_arguments)]
#[inline]
fn point_in_triangle(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
    px: f64,
    py: f64,
) -> bool {
    (cx - px) * (ay - py) - (ax - px) * (cy - py) >= 0.0
        && (ax - px) * (by - py) - (bx - px) * (ay - py) >= 0.0
        && (bx - px) * (cy - py) - (cx - px) * (by - py) >= 0.0
}

/// Triangulate the polygon described by `points` + `hole_indices`.
///
/// `points` is the outer ring followed by each hole's vertices; each entry
/// of `hole_indices` is the start offset of a hole. Returns triangles as
/// triples of indices into `points`.
pub fn earcut(points: &[Point2], hole_indices: &[usize]) -> Vec<[usize; 3]> {
    let has_holes = !hole_indices.is_empty();
    let outer_len = if has_holes {
        hole_indices[0]
    } else {
        points.len()
    };
    let mut ear = Ear::new();
    let idxs: Vec<usize> = (0..outer_len).collect();
    let mut triangles = Vec::new();
    let Some(mut outer) = ear.build_ring(points, &idxs, true) else {
        return triangles;
    };
    if ear.n(outer).next == ear.n(outer).prev {
        return triangles;
    }
    if has_holes {
        outer = ear.eliminate_holes(points, hole_indices, outer);
    }
    ear.earcut_linked(Some(outer), &mut triangles, 0);
    triangles
}

/// Triangulate `outer` (a planar loop) minus `holes`.
///
/// Returns 3D triangles. A convex hole-free loop fans in O(n); anything
/// concave or holed goes through earcut.
pub fn triangulate(outer: &[DVec3], holes: &[Vec<DVec3>], normal: Option<DVec3>) -> Vec<Tri3> {
    if outer.len() < 3 {
        return Vec::new();
    }
    let holes: Vec<&Vec<DVec3>> = holes.iter().filter(|h| h.len() >= 3).collect();
    let normal = normal.unwrap_or_else(|| newell_unit(outer));
    let (u, v) = plane_axes(normal);
    let origin = outer[0];
    let proj = |p: DVec3| -> Point2 {
        let rel = p - origin;
        (rel.dot(u), rel.dot(v))
    };
    let pts_outer: Vec<Point2> = outer.iter().map(|&p| proj(p)).collect();
    if holes.is_empty() && is_convex(&pts_outer) {
        return (1..outer.len() - 1)
            .map(|i| [outer[0], outer[i], outer[i + 1]])
            .collect();
    }
    let mut master3: Vec<DVec3> = outer.to_vec();
    let mut pts2 = pts_outer;
    let mut hole_indices = Vec::with_capacity(holes.len());
    for h in holes {
        hole_indices.push(master3.len());
        master3.extend_from_slice(h);
        pts2.extend(h.iter().map(|&p| proj(p)));
    }
    earcut(&pts2, &hole_indices)
        .into_iter()
        .map(|[a, b, c]| [master3[a], master3[b], master3[c]])
        .collect()
}
