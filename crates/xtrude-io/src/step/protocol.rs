// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! What the STEP helper writes on stdout: the meshed file as a
//! [`RawStep`], encoded little-endian behind a magic and a version.
//!
//! The helper (`xtrude-step`) encodes, [`super::helper`] decodes; both use
//! this module, so the two sides cannot drift apart. The decoder checks
//! every count against the bytes left and every index against what it
//! points into: a broken or hostile stream is an error, never a panic or a
//! huge allocation.
//!
//! Besides stdout, the helper writes progress lines on stderr
//! ([`PROGRESS_PREFIX`] `fraction text`), its error message otherwise, and
//! exits with one of the `EXIT_*` codes.

use std::fmt;

use xtrude_core::attrs::Rgb;
use xtrude_core::math::DMat4;
use xtrude_core::DVec3;

/// The first bytes of the stream.
pub const MAGIC: &[u8; 10] = b"XTRUDESTEP";
/// The stream layout's version, bumped on any change.
pub const VERSION: u16 = 1;

/// The helper read and meshed the file.
pub const EXIT_OK: i32 = 0;
/// The file could not be read (the message is on stderr).
pub const EXIT_FAILED: i32 = 1;
/// The command line was wrong.
pub const EXIT_USAGE: i32 = 2;
/// The helper was built without OpenCascade.
pub const EXIT_NO_OCCT: i32 = 3;

/// A stderr line that reports progress: `@progress 0.5 Meshing…`.
pub const PROGRESS_PREFIX: &str = "@progress ";

/// One B-rep face, meshed: nodes in metres in its part's frame, and
/// triangles of three node indices, counter-clockwise seen from outside.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawFace {
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// The face lies on a plane, so its triangles make one flat polygon.
    pub planar: bool,
    pub color: Option<Rgb>,
}

/// One placement of a part: inside an assembly, or at the top level.
#[derive(Clone, Debug, PartialEq)]
pub struct RawInstance {
    /// An index into [`RawStep::parts`].
    pub part: usize,
    pub name: String,
    /// Part frame → parent frame, translation in metres.
    pub xform: DMat4,
}

/// One shape of the file, listed once however often it is placed: a part
/// with faces, an assembly with children, or (rarely) both.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawPart {
    pub name: String,
    pub color: Option<Rgb>,
    pub faces: Vec<RawFace>,
    pub children: Vec<RawInstance>,
    /// Every shell is closed and no face lies outside a shell.
    pub closed: bool,
    /// B-rep faces OpenCascade could not mesh.
    pub failed_faces: u32,
}

/// A meshed STEP file, as the helper reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawStep {
    pub parts: Vec<RawPart>,
    /// The top-level shapes, in file order.
    pub roots: Vec<RawInstance>,
}

/// Why a stream could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad STEP helper output: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

// ---- Encode ----------------------------------------------------------------------

/// The stream for `step`.
pub fn encode(step: &RawStep) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    put_len(&mut out, step.parts.len());
    for part in &step.parts {
        put_str(&mut out, &part.name);
        put_color(&mut out, part.color);
        out.push(u8::from(part.closed));
        out.extend_from_slice(&part.failed_faces.to_le_bytes());
        put_len(&mut out, part.faces.len());
        for face in &part.faces {
            out.push(u8::from(face.planar));
            put_color(&mut out, face.color);
            put_len(&mut out, face.positions.len());
            for p in &face.positions {
                put_f64s(&mut out, &p.to_array());
            }
            put_len(&mut out, face.triangles.len());
            for t in &face.triangles {
                for i in t {
                    out.extend_from_slice(&i.to_le_bytes());
                }
            }
        }
        put_instances(&mut out, &part.children);
    }
    put_instances(&mut out, &step.roots);
    out
}

fn put_len(out: &mut Vec<u8>, n: usize) {
    // Counts beyond u32 cannot come from one STEP file.
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_f64s(out: &mut Vec<u8>, values: &[f64]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_len(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

fn put_color(out: &mut Vec<u8>, color: Option<Rgb>) {
    match color {
        Some(c) => {
            out.push(1);
            put_f64s(out, &c);
        }
        None => out.push(0),
    }
}

fn put_instances(out: &mut Vec<u8>, instances: &[RawInstance]) {
    put_len(out, instances.len());
    for inst in instances {
        put_len(out, inst.part);
        put_str(out, &inst.name);
        // The 3×4 rows: the bottom row of a placement is always 0 0 0 1.
        let m = inst.xform.transpose().to_cols_array();
        put_f64s(out, &m[..12]);
    }
}

// ---- Decode ----------------------------------------------------------------------

/// The [`RawStep`] a stream holds.
pub fn decode(bytes: &[u8]) -> Result<RawStep, DecodeError> {
    let mut r = Reader { bytes, pos: 0 };
    if r.take(MAGIC.len())? != MAGIC {
        return Err(DecodeError("not a STEP helper stream".into()));
    }
    let version = u16::from_le_bytes([r.u8()?, r.u8()?]);
    if version != VERSION {
        return Err(DecodeError(format!(
            "stream version {version}, expected {VERSION}"
        )));
    }
    // The smallest part: empty name, no colour, flags, no faces/children.
    let n_parts = r.count(4 + 1 + 1 + 4 + 4 + 4)?;
    let mut parts = Vec::with_capacity(n_parts);
    for _ in 0..n_parts {
        let name = r.string()?;
        let color = r.color()?;
        let closed = r.u8()? != 0;
        let failed_faces = r.u32()?;
        let n_faces = r.count(1 + 1 + 4 + 4)?;
        let mut faces = Vec::with_capacity(n_faces);
        for _ in 0..n_faces {
            let planar = r.u8()? != 0;
            let color = r.color()?;
            let n_pos = r.count(24)?;
            let mut positions = Vec::with_capacity(n_pos);
            for _ in 0..n_pos {
                positions.push(DVec3::new(r.f64()?, r.f64()?, r.f64()?));
            }
            let n_tri = r.count(12)?;
            let mut triangles = Vec::with_capacity(n_tri);
            for _ in 0..n_tri {
                let t = [r.u32()?, r.u32()?, r.u32()?];
                if t.iter().any(|&i| i as usize >= n_pos) {
                    return Err(DecodeError("triangle index out of range".into()));
                }
                triangles.push(t);
            }
            faces.push(RawFace {
                positions,
                triangles,
                planar,
                color,
            });
        }
        let children = r.instances()?;
        parts.push(RawPart {
            name,
            color,
            faces,
            children,
            closed,
            failed_faces,
        });
    }
    let roots = r.instances()?;
    if r.pos != bytes.len() {
        return Err(DecodeError("trailing bytes".into()));
    }
    let out_of_range = parts
        .iter()
        .flat_map(|p| &p.children)
        .chain(&roots)
        .any(|i| i.part >= parts.len());
    if out_of_range {
        return Err(DecodeError("part index out of range".into()));
    }
    Ok(RawStep { parts, roots })
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| DecodeError("truncated".into()))?;
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f64(&mut self) -> Result<f64, DecodeError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        let v = f64::from_le_bytes(a);
        if !v.is_finite() {
            return Err(DecodeError("non-finite number".into()));
        }
        Ok(v)
    }

    /// A count of items at least `min_size` bytes each, checked against
    /// the bytes left so a bogus count cannot allocate.
    fn count(&mut self, min_size: usize) -> Result<usize, DecodeError> {
        let n = self.u32()? as usize;
        let left = self.bytes.len() - self.pos;
        if n.saturating_mul(min_size) > left {
            return Err(DecodeError("count larger than the stream".into()));
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String, DecodeError> {
        let n = self.count(1)?;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| DecodeError("name is not UTF-8".into()))
    }

    fn color(&mut self) -> Result<Option<Rgb>, DecodeError> {
        match self.u8()? {
            0 => Ok(None),
            _ => Ok(Some([self.f64()?, self.f64()?, self.f64()?])),
        }
    }

    fn instances(&mut self) -> Result<Vec<RawInstance>, DecodeError> {
        let n = self.count(4 + 4 + 12 * 8)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let part = self.u32()? as usize;
            let name = self.string()?;
            let mut rows = [0.0; 16];
            for v in rows.iter_mut().take(12) {
                *v = self.f64()?;
            }
            rows[15] = 1.0;
            out.push(RawInstance {
                part,
                name,
                xform: DMat4::from_cols_array(&rows).transpose(),
            });
        }
        Ok(out)
    }
}
