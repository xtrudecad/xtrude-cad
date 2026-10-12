// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The XML entries of a VFF container (`openskp/_core.py full_parse`,
//! "Materials & layer colors" and "Styles"; `_extract_texture`).
//!
//! The XML comes from inside the `.skp`, so it is untrusted. Python parses
//! it with `defusedxml`, which refuses any entity declaration; here a
//! document that declares one is refused the same way, before
//! `roxmltree` sees it.

use roxmltree::{Document, Node, ParsingOptions};

use crate::container::VffArchive;
use crate::errors::SkpParseError;
use crate::model::{Style, Texture};
use crate::parsed::RawMaterial;
use crate::py;

const MAT_NS: &str = "http://sketchup.google.com/schemas/sketchup/1.0/material";
const STY_NS: &str = "http://sketchup.google.com/schemas/sketchup/1.0/style";
const TYP_NS: &str = "http://sketchup.google.com/schemas/1.0/types";

/// `ET.fromstring` on bytes: the text, decoded by its BOM or XML
/// declaration. `None` where Python raises `ParseError` (an undecodable
/// encoding) or `EntitiesForbidden` (an entity declaration).
fn xml_text(bytes: &[u8]) -> Option<String> {
    let text = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        String::from_utf8(rest.to_vec()).ok()?
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        utf16(rest, u16::from_le_bytes)?
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        utf16(rest, u16::from_be_bytes)?
    } else {
        match declared_encoding(bytes).as_deref() {
            None | Some("utf-8" | "utf8" | "us-ascii" | "ascii") => {
                String::from_utf8(bytes.to_vec()).ok()?
            }
            Some("iso-8859-1" | "latin-1" | "latin1") => bytes.iter().map(|&b| b as char).collect(),
            Some(_) => return None,
        }
    };
    // defusedxml: forbid_entities.
    if text.contains("<!ENTITY") {
        return None;
    }
    Some(text)
}

fn utf16(b: &[u8], unit: fn([u8; 2]) -> u16) -> Option<String> {
    if b.len() % 2 != 0 {
        return None;
    }
    char::decode_utf16(b.chunks_exact(2).map(|c| unit([c[0], c[1]])))
        .collect::<Result<String, _>>()
        .ok()
}

/// The `encoding="…"` of an XML declaration, lower-cased.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let head = bytes.strip_prefix(b"<?xml")?;
    let end = head.windows(2).position(|w| w == b"?>")?;
    let decl = std::str::from_utf8(&head[..end]).ok()?;
    let at = decl.find("encoding")?;
    let rest = decl[at + "encoding".len()..]
        .trim_start()
        .strip_prefix('=')?;
    let rest = rest.trim_start();
    let quote = rest.chars().next()?;
    let value = rest.get(1..)?.split(quote).next()?;
    Some(value.to_ascii_lowercase())
}

fn parse(text: &str) -> Option<Document<'_>> {
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .ok()
}

fn is(node: &Node, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == name
}

/// `elem.find('ns:name')`: the first child element with that name.
fn child<'a, 'i>(node: Node<'a, 'i>, ns: &str, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is(c, ns, name))
}

/// One `materials/<folder>/material.xml`, as `full_parse` reads it; `None`
/// where Python's `try` block swallows an exception (the material is then
/// skipped). `xml_name` is the entry's archive name.
pub(crate) fn parse_material(
    zf: &mut VffArchive,
    names: &[String],
    xml_name: &str,
    xml: &[u8],
) -> Option<RawMaterial> {
    let text = xml_text(xml)?;
    let doc = parse(&text)?;
    // `root.find('.//mat:material')`: descendants of the root, not itself.
    let mat_elem = doc
        .root_element()
        .descendants()
        .skip(1)
        .find(|n| is(n, MAT_NS, "material"))?;
    let name = mat_elem.attribute("name").unwrap_or("unknown").to_string();
    let int_attr = |attr: &str| match mat_elem.attribute(attr) {
        Some(v) => py::parse_int(v),
        None => Some(128),
    };
    let r = int_attr("colorRed")?;
    let g = int_attr("colorGreen")?;
    let b = int_attr("colorBlue")?;
    // `trans` only applies when useTrans="1"; it stores a TRANSPARENCY, so
    // the opacity is 1 - trans.
    let transparency = if mat_elem.attribute("useTrans") == Some("1") {
        let trans = match mat_elem.attribute("trans") {
            Some(v) => py::parse_float(v)?,
            None => 0.0,
        };
        py::min(py::max(1.0 - trans, 0.0), 1.0)
    } else {
        1.0
    };
    // type="2" marks a colourized copy ("[Name]1").
    let colorized = mat_elem.attribute("type") == Some("2");
    let colorize_type = match mat_elem.attribute("colorizeType") {
        Some(v) if !v.is_empty() => py::parse_int(v).unwrap_or(0),
        _ => 0,
    };
    let texture = extract_texture(zf, names, xml_name, mat_elem).ok()?;
    Some(RawMaterial {
        name,
        color: [r, g, b],
        alpha: None,
        transparency,
        colorized,
        colorize_type,
        texture,
    })
}

/// `float(attr or 0)` with `ValueError` → 0.0.
fn scale_attr(elem: Node, attr: &str) -> f64 {
    match elem.attribute(attr) {
        Some(v) if !v.is_empty() => py::parse_float(v).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `_extract_texture`: the material's `<mat:texture>`, its tile size and
/// image bytes. `Ok(None)` for a plain colour material; `Err` where a
/// read fails (Python's exception then skips the whole material).
fn extract_texture(
    zf: &mut VffArchive,
    names: &[String],
    xml_name: &str,
    mat_elem: Node,
) -> Result<Option<Texture>, SkpParseError> {
    let Some(tex_elem) = child(mat_elem, MAT_NS, "texture") else {
        return Ok(None);
    };
    let mut filename = tex_elem
        .attribute("textureFilename")
        .unwrap_or_default()
        .to_string();
    let x_scale = scale_attr(tex_elem, "xScale");
    let y_scale = scale_attr(tex_elem, "yScale");

    let folder = xml_name.rsplit_once('/').map_or(xml_name, |(f, _)| f);
    let has = |n: &str| names.iter().any(|x| x == n);
    let mut data = None;
    let candidate = (!filename.is_empty()).then(|| format!("{folder}/{filename}"));
    match candidate {
        Some(c) if has(&c) => data = Some(zf.read(&c)?),
        _ => {
            // The image name may differ from textureFilename: take the
            // folder's non-XML sibling.
            let prefix = format!("{folder}/");
            for entry in names {
                if entry.starts_with(&prefix)
                    && entry != xml_name
                    && !entry.to_lowercase().ends_with(".xml")
                {
                    data = Some(zf.read(entry)?);
                    if filename.is_empty() {
                        filename = last_component(entry);
                    }
                    break;
                }
            }
        }
    }
    if data.is_none() {
        // A colourized copy keeps no image: its <mat:image path> points
        // into the SOURCE material's folder, sometimes prefixed "./".
        let img_path = child(tex_elem, MAT_NS, "images")
            .and_then(|imgs| child(imgs, MAT_NS, "image"))
            .and_then(|img| img.attribute("path"))
            .unwrap_or_default()
            .trim_start_matches(['.', '/'])
            .to_string();
        for cand in [img_path.clone(), format!("{folder}/{img_path}")] {
            if !cand.is_empty() && has(&cand) {
                data = Some(zf.read(&cand)?);
                if filename.is_empty() {
                    filename = last_component(&cand);
                }
                break;
            }
        }
    }
    Ok(Some(Texture {
        filename,
        width: x_scale,
        height: y_scale,
        data,
    }))
}

fn last_component(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// One `styles/*/style.xml`: its name and the front (`4000`) and back
/// (`4001`) face colours, stored as signed-int32 ARGB. `None` when the
/// document does not parse or holds no style (the entry is skipped).
pub(crate) fn parse_style(xml: &[u8]) -> Option<Style> {
    let text = xml_text(xml)?;
    let doc = parse(&text)?;
    let style_el = child(doc.root_element(), STY_NS, "style")?;
    let mut front = None;
    let mut back = None;
    for item in style_el.children().filter(|c| is(c, STY_NS, "item")) {
        let iid = item.attribute("id");
        let Some(var) = child(item, TYP_NS, "variant") else {
            continue;
        };
        let Some(text) = var.text().filter(|t| !t.is_empty()) else {
            continue;
        };
        let slot = match iid {
            Some("4000") => &mut front,
            Some("4001") => &mut back,
            _ => continue,
        };
        let Some(v) = py::parse_int_low32(text) else {
            continue;
        };
        *slot = Some([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    Some(Style {
        name: style_el.attribute("name").unwrap_or_default().to_string(),
        front_color: front,
        back_color: back,
    })
}
