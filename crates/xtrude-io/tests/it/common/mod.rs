// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Helpers shared by the xtrude-io tests.

pub mod skp;
pub mod step;

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The repository's shared test data (`tests/data`).
pub fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

/// A scratch directory removed when dropped.
pub struct TmpDir(pub PathBuf);

impl TmpDir {
    pub fn new() -> TmpDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "xtrude-io-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TmpDir(p)
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The `scene` block of a document (plain or ZIP container).
pub fn payload(path: &Path) -> Value {
    use std::io::Read;
    let raw = std::fs::read(path).expect("readable");
    let text = if raw.starts_with(b"PK\x03\x04") {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(raw)).expect("zip");
        let mut s = String::new();
        z.by_name("document.json")
            .expect("entry")
            .read_to_string(&mut s)
            .expect("utf-8");
        s
    } else {
        String::from_utf8(raw).expect("utf-8")
    };
    let v: Value = serde_json::from_str(&text).expect("json");
    v["scene"].clone()
}

pub fn v3(x: f64, y: f64, z: f64) -> xtrude_core::DVec3 {
    xtrude_core::DVec3::new(x, y, z)
}

/// Save `scene` to a scratch file and load it into a fresh scene.
pub fn roundtrip(scene: &xtrude_model::scene::Scene) -> xtrude_model::scene::Scene {
    let tmp = TmpDir::new();
    let path = tmp.join("doc.igz");
    xtrude_io::igz::save_scene(scene, &path).expect("save");
    let mut back = xtrude_model::scene::Scene::new();
    xtrude_io::igz::load_into(&mut back, &path, None).expect("load");
    back
}

pub fn quad(
    m: &mut xtrude_core::Mesh,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    z: f64,
) -> xtrude_core::FaceId {
    m.add_face(
        &[v3(x0, y0, z), v3(x1, y0, z), v3(x1, y1, z), v3(x0, y1, z)],
        &[],
    )
}

/// Draw a rectangle the way the Rectangle tool does.
pub fn rectangle(
    s: &mut xtrude_model::scene::Scene,
    h: &mut xtrude_model::history::History,
    corners: &[xtrude_core::DVec3],
) {
    use xtrude_model::history::Command;
    let segs: Vec<(xtrude_core::DVec3, xtrude_core::DVec3)> = (0..corners.len())
        .map(|i| (corners[i], corners[(i + 1) % corners.len()]))
        .collect();
    let face: Box<dyn Command> = Box::new(xtrude_model::commands::AddFaceCommand::new(
        corners.to_vec(),
    ));
    let mut cmd = xtrude_model::edits::build_add_edges(s, &segs, false, vec![face], None);
    h.execute(s, &mut *cmd);
}

/// A `4 × 4 × 3` box drawn and pushed like the user would.
pub fn drawn_cube(s: &mut xtrude_model::scene::Scene, h: &mut xtrude_model::history::History) {
    rectangle(
        s,
        h,
        &[
            v3(0., 0., 0.),
            v3(4., 0., 0.),
            v3(4., 4., 0.),
            v3(0., 4., 0.),
        ],
    );
    let f = s.mesh().face_ids()[0];
    let d = if s.mesh().face_normal(f).z > 0.0 {
        3.0
    } else {
        -3.0
    };
    let mid = s.active_mesh_id();
    xtrude_model::pushpull::push_face(s, h, mid, f, d, false);
}

/// A binary STL of `triangles`.
pub fn write_binary_stl(path: &std::path::Path, triangles: &[[[f64; 3]; 3]], header: &[u8]) {
    let mut out = header[..header.len().min(80)].to_vec();
    out.resize(80, 0);
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for t in triangles {
        for _ in 0..3 {
            out.extend_from_slice(&0f32.to_le_bytes());
        }
        for p in t {
            for x in p {
                out.extend_from_slice(&(*x as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    std::fs::write(path, out).expect("write stl");
}

/// `(min x, max x, min y, max y, min z, max z)` of the loose mesh's faces.
pub fn face_bounds(s: &xtrude_model::scene::Scene) -> [f64; 6] {
    let m = s.mesh();
    let pts: Vec<xtrude_core::DVec3> = m
        .face_ids()
        .iter()
        .flat_map(|&f| m.face_positions(f))
        .collect();
    let mut b = [f64::MAX, f64::MIN, f64::MAX, f64::MIN, f64::MAX, f64::MIN];
    for p in pts {
        for (i, c) in [p.x, p.y, p.z].into_iter().enumerate() {
            b[2 * i] = b[2 * i].min(c);
            b[2 * i + 1] = b[2 * i + 1].max(c);
        }
    }
    b
}

pub fn approx6(a: [f64; 6], b: [f64; 6]) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| (x - y).abs() <= 1e-6 * y.abs().max(1.0))
}
