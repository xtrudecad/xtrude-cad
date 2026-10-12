// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Deterministic plane rebuild for push/pull (the root-fix, "path C").
//!
//! One deterministic recompute per touched plane:
//!
//! 1. Gather the mesh edges lying on the plane.
//! 2. Run the planar arrangement to get every minimal bounded region.
//! 3. Classify each region by **which side of the plane holds material**,
//!    volumetrically (parity ray-casting from a sample point just off the
//!    region on each side). Material on exactly **one** side → a boundary
//!    face (wound toward the empty side); on neither → a phantom; on both →
//!    solid interior.
//! 4. Union each side's regions into the final face loops.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::arrangement::{
    interior_point as interior_point_2d, planar_arrangement, plane_basis, point_in_polygon,
    scan_interior_point, signed_area, P2,
};
use crate::math::{DVec3, VecExt, MIN_FACE_AREA};
use crate::mesh::{EdgeId, FaceId, Mesh};
use crate::mt19937::Mt19937;
use crate::orient::{ray_parity_outside, PackedTris, Triangle};

/// A point this close to the plane (along its normal) counts as on it.
pub const ON_PLANE: f64 = 1e-4;
const TOL: f64 = 1e-4;
/// Normal offset of the material sample points.
const SAMPLE_OFF: f64 = 1e-3;

/// A rebuilt face: `(outer, holes, is_partition)`.
pub type RebuiltFace = (Vec<DVec3>, Vec<Vec<DVec3>>, bool);

/// `(faces, edges)` of the solids a rebuild works in.
type SolidScope = (Vec<FaceId>, Vec<EdgeId>);

/// What [`RebuildCache::packed`] was built from: the shadowed faces and the
/// wanted components, both sorted.
type PackedKey = (Vec<FaceId>, Option<Vec<usize>>);

/// Per-face memos shared across one push's plane rebuilds. Safe because the
/// fixpoint loop never mutates a face in place: a rebuild removes faces and
/// adds new ones (fresh ids).
#[derive(Default)]
pub struct RebuildCache {
    /// Outlines of the strips a rebuild found inside the solid and dropped.
    pub dropped: Vec<(DVec3, DVec3)>,
    tris: HashMap<FaceId, Vec<Triangle>>,
    norm: HashMap<FaceId, DVec3>,
    cent: HashMap<FaceId, DVec3>,
    /// Bumped whenever a rebuild actually changed the mesh.
    pub token: u64,
    packed: Option<(u64, PackedKey, PackedTris)>,
    comp: Option<(u64, HashMap<FaceId, usize>)>,
    /// The operation's own faces: their components are the only solids a
    /// rebuild may touch.
    pub seed_faces: HashSet<FaceId>,
    scope: Option<(u64, Option<SolidScope>)>,
}

impl RebuildCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn components(&mut self, mesh: &Mesh) -> &HashMap<FaceId, usize> {
        let stale = !matches!(&self.comp, Some((t, _)) if *t == self.token);
        if stale {
            self.comp = Some((self.token, face_components(mesh)));
        }
        &self.comp.as_ref().expect("components").1
    }

    /// `(faces, edges)` of the solids the operation is working in, or `None`.
    pub fn scope(&mut self, mesh: &Mesh) -> Option<SolidScope> {
        if self.seed_faces.is_empty() {
            return None;
        }
        if let Some((t, s)) = &self.scope {
            if *t == self.token {
                return s.clone();
            }
        }
        let comp = self.components(mesh).clone();
        let live: HashSet<usize> = self
            .seed_faces
            .iter()
            .filter_map(|f| comp.get(f).copied())
            .collect();
        let result = if live.is_empty() {
            None
        } else {
            let faces: Vec<FaceId> = mesh
                .face_ids()
                .into_iter()
                .filter(|f| comp.get(f).is_some_and(|c| live.contains(c)))
                .collect();
            let fset: HashSet<FaceId> = faces.iter().copied().collect();
            let edges: Vec<EdgeId> = mesh
                .edges
                .iter()
                .filter(|(_, e)| e.faces.is_empty() || e.faces.iter().any(|f| fset.contains(f)))
                .map(|(id, _)| id)
                .collect();
            Some((faces, edges))
        };
        self.scope = Some((self.token, result.clone()));
        result
    }

    pub fn triangles(&mut self, mesh: &Mesh, f: FaceId) -> Vec<Triangle> {
        self.tris
            .entry(f)
            .or_insert_with(|| mesh.face_triangulate(f, None))
            .clone()
    }

    pub fn normal(&mut self, mesh: &Mesh, f: FaceId) -> DVec3 {
        *self
            .norm
            .entry(f)
            .or_insert_with(|| mesh.face_normal(f).unit_or_zero())
    }

    pub fn centroid(&mut self, mesh: &Mesh, f: FaceId) -> DVec3 {
        *self.cent.entry(f).or_insert_with(|| mesh.face_centroid(f))
    }

    fn packed(
        &mut self,
        mesh: &Mesh,
        shadowed: &HashSet<FaceId>,
        want: Option<&HashSet<usize>>,
    ) -> PackedTris {
        let mut sh: Vec<FaceId> = shadowed.iter().copied().collect();
        sh.sort();
        let mut wv: Option<Vec<usize>> = want.map(|w| w.iter().copied().collect());
        if let Some(w) = wv.as_mut() {
            w.sort();
        }
        let key = (sh, wv);
        if let Some((tok, k, val)) = &self.packed {
            if *tok == self.token && *k == key {
                return val.clone();
            }
        }
        let comp = if want.is_some() {
            Some(self.components(mesh).clone())
        } else {
            None
        };
        let mut lists = Vec::new();
        for f in mesh.face_ids() {
            if mesh.face(f).interior || shadowed.contains(&f) {
                continue;
            }
            if let (Some(w), Some(c)) = (want, comp.as_ref()) {
                if !c.get(&f).is_some_and(|x| w.contains(x)) {
                    continue;
                }
            }
            lists.push(self.triangles(mesh, f));
        }
        let val = PackedTris::from_lists(&lists);
        self.packed = Some((self.token, key, val.clone()));
        val
    }
}

fn face_components(mesh: &Mesh) -> HashMap<FaceId, usize> {
    let faces = mesh.face_ids();
    let idx: HashMap<FaceId, usize> = faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for (_, e) in mesh.edges.iter() {
        let ids: Vec<usize> = e.faces.iter().filter_map(|f| idx.get(f).copied()).collect();
        if let Some((&first, rest)) = ids.split_first() {
            for &j in rest {
                let ra = find(&mut parent, first);
                let rb = find(&mut parent, j);
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    faces
        .iter()
        .enumerate()
        .map(|(i, &f)| (f, find(&mut parent, i)))
        .collect()
}

type K2 = (i64, i64);

fn key2(p: P2) -> K2 {
    (
        crate::math::round_even_int(p.0 / TOL),
        crate::math::round_even_int(p.1 / TOL),
    )
}

fn on_plane(pos: DVec3, origin: DVec3, normal: DVec3) -> bool {
    (pos - origin).dot(normal).abs() < ON_PLANE
}

/// A point inside `outer_xy` but outside every hole.
pub fn region_test_point(outer_xy: &[P2], holes_xy: &[Vec<P2>]) -> P2 {
    let p = interior_point_2d(outer_xy);
    let inside =
        |q: P2| point_in_polygon(q, outer_xy) && !holes_xy.iter().any(|h| point_in_polygon(q, h));
    if inside(p) {
        return p;
    }
    let n = outer_xy.len();
    for i in 0..n {
        let (ax, ay) = outer_xy[i];
        let (bx, by) = outer_xy[(i + 1) % n];
        let (mx, my) = ((ax + bx) / 2.0, (ay + by) / 2.0);
        let (dx, dy) = (bx - ax, by - ay);
        let ln = dx.hypot(dy);
        if ln < TOL {
            continue;
        }
        let q = (mx - dy / ln * 1e-3, my + dx / ln * 1e-3);
        if inside(q) {
            return q;
        }
    }
    scan_interior_point(outer_xy, holes_xy).unwrap_or(p)
}

fn on_seg2(p: P2, a: P2, b: P2) -> bool {
    let tol = TOL / 2.0;
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (p.0 - a.0, p.1 - a.1);
    if (abx * apy - aby * apx).abs() > tol * 1f64.max(abx.abs() + aby.abs()) {
        return false;
    }
    let dot = apx * abx + apy * aby;
    -tol <= dot && dot <= abx * abx + aby * aby + tol
}

type Region = (Vec<P2>, Vec<Vec<P2>>);

/// Union 2D regions (outer CCW, holes CW) into `[(outer, holes)]`, dropping
/// every edge interior to the union. `keep_segs` are creases: union edges
/// lying on one never cancel.
fn union_outline(regions: &[Region], keep_segs: &[(P2, P2)]) -> Vec<Region> {
    let mut dir_count: BTreeMap<(K2, K2), i64> = BTreeMap::new();
    let mut coords: HashMap<K2, P2> = HashMap::new();
    for (outer, holes) in regions {
        for lp in std::iter::once(outer).chain(holes.iter()) {
            let n = lp.len();
            for i in 0..n {
                let (a, b) = (lp[i], lp[(i + 1) % n]);
                let (ka, kb) = (key2(a), key2(b));
                coords.insert(ka, a);
                coords.insert(kb, b);
                *dir_count.entry((ka, kb)).or_insert(0) += 1;
            }
        }
    }
    let is_crease = |ka: K2, kb: K2| -> bool {
        if keep_segs.is_empty() {
            return false;
        }
        let (pa, pb) = (coords[&ka], coords[&kb]);
        let mid = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
        keep_segs
            .iter()
            .any(|&(a, b)| on_seg2(mid, a, b) && on_seg2(pa, a, b) && on_seg2(pb, a, b))
    };
    let mut survivors: BTreeMap<(K2, K2), i64> = BTreeMap::new();
    for (&(ka, kb), &c) in &dir_count {
        if is_crease(ka, kb) {
            *survivors.entry((ka, kb)).or_insert(0) += c;
            continue;
        }
        let net = c - dir_count.get(&(kb, ka)).copied().unwrap_or(0);
        if net > 0 {
            *survivors.entry((ka, kb)).or_insert(0) += net;
        }
    }
    let mut nbrs: HashMap<K2, HashSet<K2>> = HashMap::new();
    for (&(ka, kb), &c) in &survivors {
        if c > 0 {
            nbrs.entry(ka).or_default().insert(kb);
            nbrs.entry(kb).or_default().insert(ka);
        }
    }
    let mut order: HashMap<K2, Vec<K2>> = HashMap::new();
    for (k, ns) in &nbrs {
        let (kx, ky) = coords[k];
        let mut ring: Vec<K2> = ns.iter().copied().collect();
        ring.sort_by(|m1, m2| {
            let a1 = (coords[m1].1 - ky).atan2(coords[m1].0 - kx);
            let a2 = (coords[m2].1 - ky).atan2(coords[m2].0 - kx);
            a1.partial_cmp(&a2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(m1.cmp(m2))
        });
        order.insert(*k, ring);
    }
    let starts: Vec<(K2, K2)> = survivors.keys().copied().collect();
    let mut loops: Vec<Vec<P2>> = Vec::new();
    for start in starts {
        while survivors.get(&start).copied().unwrap_or(0) > 0 {
            let (u, v) = start;
            *survivors.get_mut(&start).expect("start") -= 1;
            let mut lp: Vec<K2> = Vec::new();
            let (mut cu, mut cv) = (u, v);
            let mut ok = true;
            loop {
                lp.push(cu);
                let Some(ring) = order.get(&cv) else {
                    ok = false;
                    break;
                };
                let Some(idx) = ring.iter().position(|&x| x == cu) else {
                    ok = false;
                    break;
                };
                let m = ring.len();
                let mut w = None;
                for s in 1..=m {
                    let cand = ring[(idx + m * 2 - s) % m];
                    if survivors.get(&(cv, cand)).copied().unwrap_or(0) > 0 || (cv, cand) == start {
                        w = Some(cand);
                        break;
                    }
                }
                let Some(w) = w else {
                    ok = false;
                    break;
                };
                if lp.len() > 100_000 {
                    ok = false;
                    break;
                }
                cu = cv;
                cv = w;
                if (cu, cv) == start {
                    break;
                }
                *survivors.entry((cu, cv)).or_insert(0) -= 1;
            }
            if ok && lp.len() >= 3 {
                loops.push(lp.iter().map(|k| coords[k]).collect());
            }
        }
    }
    let mut outers: Vec<Vec<P2>> = loops
        .iter()
        .filter(|lp| signed_area(lp) > MIN_FACE_AREA)
        .cloned()
        .collect();
    let holes: Vec<Vec<P2>> = loops
        .iter()
        .filter(|lp| signed_area(lp) < -MIN_FACE_AREA)
        .cloned()
        .collect();
    outers.sort_by(|a, b| {
        signed_area(a)
            .partial_cmp(&signed_area(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut result: Vec<Region> = outers.into_iter().map(|o| (o, Vec::new())).collect();
    for h in holes {
        let hp = interior_point_2d(&h);
        let hkeys: HashSet<K2> = h.iter().map(|&p| key2(p)).collect();
        for (outer, hl) in result.iter_mut() {
            if outer.len() == h.len()
                && outer.iter().map(|&p| key2(p)).collect::<HashSet<K2>>() == hkeys
            {
                continue;
            }
            if point_in_polygon(hp, outer) {
                hl.push(h.clone());
                break;
            }
        }
    }
    result
}

fn coplanar_on(
    mesh: &Mesh,
    face: FaceId,
    origin: DVec3,
    normal: DVec3,
    cache: &mut Option<&mut RebuildCache>,
) -> bool {
    let fnm = match cache {
        Some(c) => c.normal(mesh, face),
        None => mesh.face_normal(face).unit_or_zero(),
    };
    if fnm.dot(normal).abs() <= 0.999 {
        return false;
    }
    let fc = match cache {
        Some(c) => c.centroid(mesh, face),
        None => mesh.face_centroid(face),
    };
    on_plane(fc, origin, normal)
}

fn fnormal(mesh: &Mesh, f: FaceId, cache: &mut Option<&mut RebuildCache>) -> DVec3 {
    match cache {
        Some(c) => c.normal(mesh, f),
        None => mesh.face_normal(f).unit_or_zero(),
    }
}

/// Options of a plane rebuild.
#[derive(Clone, Copy, Debug)]
pub struct RebuildOpts {
    pub keep_mode: bool,
    pub removing: bool,
    /// After [`apply_rebuild`] changed the plane, drop its loose debris
    /// ([`prune_plane_debris`]). [`rebuild_plane`] ignores it.
    pub prune: bool,
}

impl Default for RebuildOpts {
    fn default() -> Self {
        RebuildOpts {
            keep_mode: false,
            removing: true,
            prune: false,
        }
    }
}

/// Recompute the solid *boundary* faces of `mesh` on the plane
/// `(origin, normal)`. Pure: reads the mesh. `None` when the plane carries
/// too little to face.
pub fn rebuild_plane(
    mesh: &Mesh,
    origin: DVec3,
    normal: DVec3,
    fresh: &[FaceId],
    opts: RebuildOpts,
    mut op: Option<&mut Vec<(DVec3, DVec3)>>,
    mut cache: Option<&mut RebuildCache>,
) -> Option<Vec<RebuiltFace>> {
    let normal = normal.unit_or_zero();
    let (u, v) = plane_basis(normal);
    let scope = cache.as_deref_mut().and_then(|c| c.scope(mesh));
    let (scope_faces, scope_edges) = match scope {
        Some((f, e)) => (f, e),
        None => (mesh.face_ids(), mesh.edge_ids()),
    };
    let to2d = |p: DVec3| -> P2 {
        let d = p - origin;
        (d.dot(u), d.dot(v))
    };
    let to3d = |xy: P2| -> DVec3 { origin + u * xy.0 + v * xy.1 };

    let segs: Vec<(DVec3, DVec3)> = scope_edges
        .iter()
        .map(|&e| mesh.edge_ab(e))
        .filter(|&(a, b)| on_plane(a, origin, normal) && on_plane(b, origin, normal))
        .collect();
    if segs.len() < 3 {
        return None;
    }
    let regions_3d = planar_arrangement(&segs, origin, normal);
    if regions_3d.is_empty() {
        return None;
    }

    let fresh_set: Vec<FaceId> = {
        let mut seen = HashSet::new();
        fresh
            .iter()
            .copied()
            .filter(|&f| mesh.has_face(f) && seen.insert(f))
            .collect()
    };
    let fresh_hs: HashSet<FaceId> = fresh_set.iter().copied().collect();
    let mut shadowed: HashSet<FaceId> = HashSet::new();
    if opts.keep_mode {
        for &f in &fresh_set {
            if mesh.face(f).interior {
                continue;
            }
            let fnm = fnormal(mesh, f, &mut cache);
            let fc = match &mut cache {
                Some(c) => c.centroid(mesh, f),
                None => mesh.face_centroid(f),
            };
            let (fu, fv) = plane_basis(fnm);
            let fto2d = |p: DVec3| -> P2 {
                let d = p - fc;
                (d.dot(fu), d.dot(fv))
            };
            for &g in &scope_faces {
                if g == f || fresh_hs.contains(&g) {
                    continue;
                }
                let gn = fnormal(mesh, g, &mut cache);
                if gn.dot(fnm).abs() < 0.999 {
                    continue;
                }
                let gc = match &mut cache {
                    Some(c) => c.centroid(mesh, g),
                    None => mesh.face_centroid(g),
                };
                if (gc - fc).dot(fnm).abs() > ON_PLANE {
                    continue;
                }
                let outer: Vec<P2> = mesh.face_positions(g).into_iter().map(fto2d).collect();
                if point_in_polygon((0.0, 0.0), &outer)
                    && !mesh.face_hole_positions(g).iter().any(|h| {
                        point_in_polygon(
                            (0.0, 0.0),
                            &h.iter().map(|&p| fto2d(p)).collect::<Vec<_>>(),
                        )
                    })
                {
                    shadowed.insert(f);
                    break;
                }
            }
        }
    }

    let packed = match &mut cache {
        None => {
            let lists: Vec<Vec<Triangle>> = mesh
                .face_ids()
                .into_iter()
                .filter(|f| !mesh.face(*f).interior && !shadowed.contains(f))
                .map(|f| mesh.face_triangulate(f, None))
                .collect();
            PackedTris::from_lists(&lists)
        }
        Some(c) => {
            let comp = c.components(mesh).clone();
            let mut want: HashSet<usize> = HashSet::new();
            for &f in &scope_faces {
                if let Some(&cf) = comp.get(&f) {
                    let mut cc = Some(&mut **c);
                    if coplanar_on(mesh, f, origin, normal, &mut cc) {
                        want.insert(cf);
                    }
                }
            }
            for f in &fresh_set {
                if let Some(&cf) = comp.get(f) {
                    want.insert(cf);
                }
            }
            if want.is_empty() {
                c.packed(mesh, &shadowed, None)
            } else {
                c.packed(mesh, &shadowed, Some(&want))
            }
        }
    };
    let mut rng = Mt19937::new(54321);

    type Poly = ((Vec<P2>, Vec<Vec<P2>>), bool);
    let proj_polys = |faces: &mut dyn Iterator<Item = FaceId>,
                      cache: &mut Option<&mut RebuildCache>|
     -> Vec<Poly> {
        let mut out = Vec::new();
        for f in faces {
            if !coplanar_on(mesh, f, origin, normal, cache) {
                continue;
            }
            let outer: Vec<P2> = mesh.face_positions(f).into_iter().map(to2d).collect();
            let holes: Vec<Vec<P2>> = mesh
                .face_hole_positions(f)
                .into_iter()
                .map(|h| h.into_iter().map(to2d).collect())
                .collect();
            let plus = fnormal(mesh, f, cache).dot(normal) > 0.0;
            out.push(((outer, holes), plus));
        }
        out
    };
    let poly_covers = |poly: &(Vec<P2>, Vec<Vec<P2>>), pt: P2| -> bool {
        point_in_polygon(pt, &poly.0) && !poly.1.iter().any(|h| point_in_polygon(pt, h))
    };

    let fresh_not_interior: Vec<FaceId> = fresh_set
        .iter()
        .copied()
        .filter(|&f| !mesh.face(f).interior)
        .collect();
    let fresh_polys: Vec<Poly> = if opts.keep_mode {
        Vec::new()
    } else {
        proj_polys(&mut fresh_not_interior.iter().copied(), &mut cache)
    };
    let fresh_cover_polys = proj_polys(&mut fresh_not_interior.iter().copied(), &mut cache);
    let old_faces: Vec<FaceId> = scope_faces
        .iter()
        .copied()
        .filter(|f| !fresh_hs.contains(f) || mesh.face(*f).interior)
        .collect();
    let old_polys = proj_polys(&mut old_faces.iter().copied(), &mut cache);
    let old_boundary: Vec<FaceId> = scope_faces
        .iter()
        .copied()
        .filter(|f| !fresh_hs.contains(f) && !mesh.face(*f).interior)
        .collect();
    let old_boundary_polys = proj_polys(&mut old_boundary.iter().copied(), &mut cache);

    let mut solid_plus: Vec<Region> = Vec::new(); // material on +side
    let mut solid_minus: Vec<Region> = Vec::new();
    let mut partitions: Vec<Region> = Vec::new();
    let mut sheets: Vec<Region> = Vec::new();
    for (outer, holes) in &regions_3d {
        let outer_xy: Vec<P2> = outer.iter().map(|&p| to2d(p)).collect();
        let holes_xy: Vec<Vec<P2>> = holes
            .iter()
            .map(|h| h.iter().map(|&p| to2d(p)).collect())
            .collect();
        let ip_xy = region_test_point(&outer_xy, &holes_xy);
        let ip = to3d(ip_xy);
        let mat_plus =
            ray_parity_outside(ip + normal * SAMPLE_OFF, normal, &packed, None, &mut rng)
                == Some(false);
        let mat_minus =
            ray_parity_outside(ip - normal * SAMPLE_OFF, -normal, &packed, None, &mut rng)
                == Some(false);
        let region = (outer_xy.clone(), holes_xy.clone());
        if mat_plus != mat_minus {
            if mat_plus {
                solid_plus.push(region);
            } else {
                solid_minus.push(region);
            }
            continue;
        }
        let decl: HashSet<bool> = fresh_polys
            .iter()
            .filter(|(poly, _)| poly_covers(poly, ip_xy))
            .map(|(_, p)| *p)
            .collect();
        if mat_plus {
            if opts.removing && decl.len() == 1 {
                let side = !*decl.iter().next().expect("one");
                if side {
                    solid_plus.push(region);
                } else {
                    solid_minus.push(region);
                }
            } else if !opts.removing
                && !opts.keep_mode
                && old_boundary_polys.iter().any(|(poly, plus)| {
                    poly_covers(poly, ip_xy)
                        && fresh_polys
                            .iter()
                            .any(|(fpoly, fplus)| poly_covers(fpoly, ip_xy) && fplus != plus)
                })
            {
                let mut rim = Vec::new();
                for lp in std::iter::once(&outer_xy).chain(holes_xy.iter()) {
                    for i in 0..lp.len() {
                        rim.push((to3d(lp[i]), to3d(lp[(i + 1) % lp.len()])));
                    }
                }
                if let Some(op) = op.as_deref_mut() {
                    op.extend(rim.iter().copied());
                }
                if let Some(c) = cache.as_deref_mut() {
                    c.dropped.extend(rim);
                }
            } else if (!opts.removing || decl.is_empty())
                && (old_polys.iter().any(|(poly, _)| poly_covers(poly, ip_xy))
                    || (opts.keep_mode
                        && fresh_cover_polys
                            .iter()
                            .any(|(poly, _)| poly_covers(poly, ip_xy))))
            {
                partitions.push(region);
            }
        } else if !opts.removing && decl.len() == 1 {
            let side = !*decl.iter().next().expect("one");
            if side {
                solid_plus.push(region);
            } else {
                solid_minus.push(region);
            }
        } else if decl.is_empty() && old_polys.iter().any(|(poly, _)| poly_covers(poly, ip_xy)) {
            sheets.push(region);
        }
    }

    let mut op_rim_segs: Vec<(P2, P2)> = Vec::new();
    let has_op = op.is_some();
    if let Some(op) = op.as_deref() {
        for &(a3, b3) in op.iter() {
            if on_plane(a3, origin, normal) && on_plane(b3, origin, normal) {
                op_rim_segs.push((to2d(a3), to2d(b3)));
            }
        }
    }
    let mut keep_segs: Vec<(P2, P2)> = Vec::new();
    for &e in &scope_edges {
        let (a, b) = mesh.edge_ab(e);
        if !(on_plane(a, origin, normal) && on_plane(b, origin, normal)) {
            continue;
        }
        let ef = mesh.edge(e).faces.clone();
        if ef
            .iter()
            .any(|&f| fnormal(mesh, f, &mut cache).dot(normal).abs() < 0.999)
        {
            keep_segs.push((to2d(a), to2d(b)));
        } else if has_op && !ef.is_empty() {
            let (ea, eb) = (to2d(a), to2d(b));
            let on_rim = op_rim_segs
                .iter()
                .any(|&(sa, sb)| on_seg2(ea, sa, sb) && on_seg2(eb, sa, sb));
            if !on_rim {
                keep_segs.push((ea, eb));
            }
        }
    }

    let mut result = Vec::new();
    // Material on + first, then on −.
    for (mat_plus, regions) in [(true, &solid_plus), (false, &solid_minus)] {
        if regions.is_empty() {
            continue;
        }
        for (mut outer, mut holes) in union_outline(regions, &keep_segs) {
            if mat_plus {
                outer.reverse();
                for h in holes.iter_mut() {
                    h.reverse();
                }
            }
            result.push((
                outer.into_iter().map(to3d).collect(),
                holes
                    .into_iter()
                    .map(|h| h.into_iter().map(to3d).collect())
                    .collect(),
                false,
            ));
        }
    }
    for (group, is_part) in [(&partitions, true), (&sheets, false)] {
        if group.is_empty() {
            continue;
        }
        for (outer, holes) in union_outline(group, &keep_segs) {
            result.push((
                outer.into_iter().map(to3d).collect(),
                holes
                    .into_iter()
                    .map(|h| h.into_iter().map(to3d).collect())
                    .collect(),
                is_part,
            ));
        }
    }
    Some(result)
}

type CanonLoop = Vec<(i64, i64, i64)>;

fn canon_loop(lp: &[DVec3]) -> CanonLoop {
    let keys: Vec<(i64, i64, i64)> = lp
        .iter()
        .map(|p| {
            (
                crate::math::round_even_int(p.x / 1e-4),
                crate::math::round_even_int(p.y / 1e-4),
                crate::math::round_even_int(p.z / 1e-4),
            )
        })
        .collect();
    if keys.is_empty() {
        return keys;
    }
    let i = (0..keys.len()).min_by_key(|&j| keys[j]).unwrap_or(0);
    keys[i..].iter().chain(keys[..i].iter()).copied().collect()
}

fn canon_faces(faces: &[RebuiltFace]) -> HashSet<(CanonLoop, Vec<CanonLoop>, bool)> {
    faces
        .iter()
        .map(|(outer, holes, interior)| {
            let mut hs: Vec<CanonLoop> = holes.iter().map(|h| canon_loop(h)).collect();
            hs.sort();
            hs.dedup();
            (canon_loop(outer), hs, *interior)
        })
        .collect()
}

/// Rebuild one plane of `mesh` in place. Returns whether anything changed —
/// false when the plane already matches the rebuilt result, so callers can
/// iterate rebuilds to a fixpoint.
pub fn apply_rebuild(
    mesh: &mut Mesh,
    origin: DVec3,
    normal: DVec3,
    fresh: &[FaceId],
    opts: RebuildOpts,
    op: Option<&mut Vec<(DVec3, DVec3)>>,
    mut cache: Option<&mut RebuildCache>,
) -> bool {
    let normal = normal.unit_or_zero();
    let Some(rebuilt) = rebuild_plane(mesh, origin, normal, fresh, opts, op, cache.as_deref_mut())
    else {
        return false;
    };
    let scope = cache.as_deref_mut().and_then(|c| c.scope(mesh));
    let candidates = match scope {
        Some((f, _)) => f,
        None => mesh.face_ids(),
    };
    let old: Vec<FaceId> = candidates
        .into_iter()
        .filter(|&f| coplanar_on(mesh, f, origin, normal, &mut cache))
        .collect();
    let old_loops: Vec<RebuiltFace> = old
        .iter()
        .map(|&f| {
            (
                mesh.face_positions(f),
                mesh.face_hole_positions(f),
                mesh.face(f).interior,
            )
        })
        .collect();
    if canon_faces(&rebuilt) == canon_faces(&old_loops) {
        return false;
    }
    if let Some(c) = cache {
        c.token += 1;
    }
    let (u, v) = plane_basis(normal);
    let to2d = |p: DVec3| -> P2 {
        let d = p - origin;
        (d.dot(u), d.dot(v))
    };
    let donors: Vec<(Vec<P2>, Vec<Vec<P2>>, crate::attrs::FaceAttrs)> = old
        .iter()
        .filter(|&&f| !mesh.face(f).attrs.is_empty())
        .map(|&f| {
            (
                mesh.face_positions(f).into_iter().map(to2d).collect(),
                mesh.face_hole_positions(f)
                    .into_iter()
                    .map(|h| h.into_iter().map(to2d).collect())
                    .collect(),
                mesh.face(f).attrs.clone(),
            )
        })
        .collect();
    for &f in &old {
        mesh.remove_face(f);
    }
    for (outer, holes, interior) in rebuilt {
        let nf = mesh.add_face(&outer, &holes);
        mesh.faces[nf].interior = interior;
        if !donors.is_empty() {
            let o2: Vec<P2> = outer.iter().map(|&p| to2d(p)).collect();
            let h2: Vec<Vec<P2>> = holes
                .iter()
                .map(|h| h.iter().map(|&p| to2d(p)).collect())
                .collect();
            let ip = region_test_point(&o2, &h2);
            for (o_xy, hs_xy, attrs) in &donors {
                if point_in_polygon(ip, o_xy) && !hs_xy.iter().any(|h| point_in_polygon(ip, h)) {
                    mesh.faces[nf].attrs = attrs.clone();
                    break;
                }
            }
        }
    }
    if opts.prune {
        prune_plane_debris(mesh, origin, normal);
    }
    true
}

/// Drop edges that lie fully on the plane and face nothing, and vertices
/// left with no incidence at all.
pub fn prune_plane_debris(mesh: &mut Mesh, origin: DVec3, normal: DVec3) {
    for e in mesh.edge_ids() {
        if !mesh.edge(e).faces.is_empty() {
            continue;
        }
        let (a, b) = mesh.edge_ab(e);
        if on_plane(a, origin, normal) && on_plane(b, origin, normal) {
            mesh.remove_edge(e);
        }
    }
    mesh.prune_orphan_vertices();
}

/// Canonical `(key, origin, normal)` for the plane through `point` with
/// `normal`: a face and its flip map to one plane.
pub fn plane_key(point: DVec3, normal: DVec3) -> ((i64, i64, i64, i64), DVec3, DVec3) {
    let mut n = normal.unit_or_zero();
    let comps = [n.x, n.y, n.z];
    let mut big = 0;
    for i in 1..3 {
        if comps[i].abs() > comps[big].abs() {
            big = i;
        }
    }
    if comps[big] < 0.0 {
        n = -n;
    }
    let d = n.dot(point);
    let r = |x: f64| crate::math::round_even_int(x * 1000.0);
    ((r(n.x), r(n.y), r(n.z), r(d)), point, n)
}

/// Representative `(origin, normal)` for each distinct plane carrying a
/// crack — a boundary edge bordering a single face.
pub fn crack_planes(mesh: &Mesh) -> Vec<(DVec3, DVec3)> {
    let mut seen: indexmap::IndexMap<(i64, i64, i64, i64), (DVec3, DVec3)> =
        indexmap::IndexMap::new();
    for (_, e) in mesh.edges.iter() {
        if e.faces.len() != 1 {
            continue;
        }
        let f = e.faces[0];
        let (key, origin, n) = plane_key(mesh.face_centroid(f), mesh.face_normal(f));
        seen.entry(key).or_insert((origin, n));
    }
    seen.into_values().collect()
}

/// Every edge of `mesh` lying along one of `segments`.
pub fn edges_along(mesh: &Mesh, segments: &[(DVec3, DVec3)]) -> Vec<EdgeId> {
    if segments.is_empty() {
        return Vec::new();
    }
    let on = |p: DVec3, a: DVec3, b: DVec3| -> bool {
        let ab = b - a;
        let l2 = ab.length_squared();
        if l2 < 1e-18 {
            return (p - a).length() < ON_PLANE;
        }
        let t = (p - a).dot(ab) / l2;
        if !(-1e-9..=1.0 + 1e-9).contains(&t) {
            return false;
        }
        (a + ab * t - p).length() < ON_PLANE
    };
    mesh.edges
        .iter()
        .filter(|(id, _)| {
            let (ea, eb) = mesh.edge_ab(*id);
            segments.iter().any(|&(a, b)| on(ea, a, b) && on(eb, a, b))
        })
        .map(|(id, _)| id)
        .collect()
}

/// Representative `(origin, normal)` for each plane where one of the
/// operation's `new_faces` ended up coplanar-edge-adjacent to another face.
pub fn seam_planes(mesh: &Mesh, new_faces: &[FaceId]) -> Vec<(DVec3, DVec3)> {
    let mut seen: indexmap::IndexMap<(i64, i64, i64, i64), (DVec3, DVec3)> =
        indexmap::IndexMap::new();
    for &f in new_faces {
        if !mesh.has_face(f) {
            continue;
        }
        let nf = mesh.face_normal(f).unit_or_zero();
        for lp in mesh.face(f).loops() {
            let n = lp.len();
            for i in 0..n {
                let Some(e) = mesh.find_edge(lp[i], lp[(i + 1) % n]) else {
                    continue;
                };
                for &g in &mesh.edge(e).faces {
                    if g == f {
                        continue;
                    }
                    if nf.dot(mesh.face_normal(g).unit_or_zero()).abs() > 0.999 {
                        let (key, origin, cn) = plane_key(mesh.face_centroid(f), nf);
                        seen.entry(key).or_insert((origin, cn));
                    }
                }
            }
        }
    }
    seen.into_values().collect()
}
