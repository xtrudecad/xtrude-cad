// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP `packages/python/tests/test_parser.py`: every class
//! that tests what `SkpFile.parse()` reaches. UPSTREAM.md lists the classes
//! that are left out (scene building, exporters, modules `parse()` never
//! calls) and why. Classes after the TLV primitives each get a
//! module named after them.

use std::io::{Cursor, Write};

use openskp::container::{validate_header, validate_zip_entry_size, MAX_COMPRESSION_RATIO};
use openskp::tlv::{
    flat_headers, parse_tlv_recursive, parse_var_int, read_f64, read_u32, Header, Tag,
    CONTAINER_TAGS,
};
use openskp::{Error, SkpFile, Stage};

fn tlv(tag: [u8; 2], payload: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

// ── TestReadU32 ──

#[test]
fn test_zero() {
    assert_eq!(read_u32(&0u32.to_le_bytes(), 0), Some(0));
}

#[test]
fn test_known_value() {
    assert_eq!(read_u32(&305419896u32.to_le_bytes(), 0), Some(305419896));
}

#[test]
fn test_offset() {
    let mut data = vec![0, 0];
    data.extend_from_slice(&42u32.to_le_bytes());
    assert_eq!(read_u32(&data, 2), Some(42));
}

// ── TestReadF64 ──

#[test]
fn test_pi() {
    let v = read_f64(&std::f64::consts::PI.to_le_bytes(), 0).unwrap();
    assert!((v - std::f64::consts::PI).abs() < 1e-15);
}

// ── TestParseVarInt ──

#[test]
fn test_single_byte() {
    assert_eq!(parse_var_int(&[0x42], 0, 1), Some(0x42));
}

#[test]
fn test_two_bytes() {
    assert_eq!(parse_var_int(&[0x01, 0x02], 0, 2), Some(0x0201));
}

#[test]
fn test_four_bytes() {
    assert_eq!(
        parse_var_int(&[0x78, 0x56, 0x34, 0x12], 0, 4),
        Some(0x12345678)
    );
}

// ── TestParseTlvRecursive ──

#[test]
fn test_single_leaf() {
    let data = tlv([0x01, 0x00], &[0xAA, 0xBB]);
    let nodes = parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag.to_string(), "0100");
    assert_eq!(nodes[0].payload, [0xAA, 0xBB]);
    assert!(nodes[0].children.is_empty());
}

#[test]
fn test_two_elements() {
    let mut data = tlv([0x01, 0x00], &[0x01]);
    data.extend(tlv([0x02, 0x00], &[0x02, 0x03]));
    let nodes = parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].tag.to_string(), "0100");
    assert_eq!(nodes[1].tag.to_string(), "0200");
}

#[test]
fn test_empty_payload() {
    let mut data = tlv([0x03, 0x00], &[]);
    data.extend(tlv([0x01, 0x00], &[0x01]));
    let nodes = parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].size, 0);
    assert!(nodes[0].payload.is_empty());
}

#[test]
fn test_single_element_whose_header_exactly_fills_the_range() {
    // A zero-payload record is exactly 6 bytes; the `pos + 6 <= end` guard
    // must keep it (openskp's boundary fix).
    let data = tlv([0x03, 0x00], &[]);
    assert_eq!(data.len(), 6);
    let nodes = parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag.to_string(), "0300");
    assert_eq!(nodes[0].size, 0);
}

// ── TestCoreTlvBoundary ──

#[test]
fn test_parse_tlv_recursive_keeps_a_header_that_exactly_fills_the_range() {
    let data = tlv([0x03, 0x00], &[]);
    let nodes = parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag, Tag::new(0x03, 0x00));
}

#[test]
fn test_flat_headers_keeps_a_header_that_exactly_fills_the_range() {
    let data = tlv([0x03, 0x00], &[]);
    assert_eq!(
        flat_headers(&data, 0, data.len()),
        [Header {
            tag: Tag::new(0x03, 0x00),
            offset: 0,
            size: 0
        }]
    );
}

// ── TestTlvNodePerformance (the contract half; laziness is Python-only) ──

#[test]
fn test_node_supports_dict_style_bracket_access() {
    let data = tlv([0x01, 0x00], &[0xAA, 0xBB]);
    let node = &parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap()[0];
    assert_eq!(node.offset, 0);
    assert_eq!(node.tag.to_string(), "0100");
    assert_eq!(node.size, 2);
    assert!(node.children.is_empty());
    assert_eq!(node.payload, [0xAA, 0xBB]);
}

#[test]
fn test_container_payload_is_empty_without_touching_the_buffer() {
    let inner = tlv([0x03, 0x00], &[0x01]);
    let data = tlv([0x7C, 0x15], &inner); // 7C15 is a real container tag
    let node = &parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap()[0];
    assert_eq!(node.children.len(), 1);
    assert!(node.payload.is_empty());
}

#[test]
fn test_leaf_payload_is_lazy_but_correct_and_cached() {
    let data = tlv([0x01, 0x00], &[0xDE, 0xAD, 0xBE, 0xEF]);
    let node = &parse_tlv_recursive(&data, 0, data.len(), &CONTAINER_TAGS).unwrap()[0];
    assert_eq!(node.payload, [0xDE, 0xAD, 0xBE, 0xEF]);
    // The payload borrows the buffer: no copy at all.
    assert!(std::ptr::eq(node.payload.as_ptr(), data[6..].as_ptr()));
}

// ── TestVff ──

#[test]
fn test_validate_header_valid() {
    let mut data = b"\xFF\xFE\xFF\x0E".to_vec();
    data.extend([0u8; 100]);
    assert!(validate_header(&data));
}

#[test]
fn test_validate_header_invalid() {
    assert!(!validate_header(b"\x00\x00\x00\x00"));
}

#[test]
fn test_validate_header_too_short() {
    assert!(!validate_header(b"\xFF\xFE"));
}

// ── TestZipEntrySizeCap ──

/// A one-entry ZIP, returning the entry's (declared, compressed) sizes.
fn zip_sizes(content: &[u8], compress: bool) -> (u64, u64) {
    let method = if compress {
        zip::CompressionMethod::Deflated
    } else {
        zip::CompressionMethod::Stored
    };
    let mut buf = Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut buf);
    let opts = zip::write::SimpleFileOptions::default().compression_method(method);
    w.start_file("payload.dat", opts).unwrap();
    w.write_all(content).unwrap();
    w.finish().unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(buf.into_inner())).unwrap();
    let e = z.by_name("payload.dat").unwrap();
    (e.size(), e.compressed_size())
}

/// Deterministic, poorly compressible bytes (the test used `os.urandom`).
fn noise(n: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[test]
fn test_rejects_an_implausible_compression_ratio() {
    let (declared, compressed) = zip_sizes(&vec![0u8; 8 * 1024 * 1024], true);
    assert!(compressed > 0);
    assert!(declared as f64 / compressed as f64 > MAX_COMPRESSION_RATIO);
    let err = validate_zip_entry_size("payload.dat", declared, compressed).unwrap_err();
    assert!(err.to_string().contains("compression ratio"));
    assert_eq!(err.stage, Some(Stage::ZipExtract));
}

#[test]
fn test_allows_a_realistic_compression_ratio() {
    let (declared, compressed) = zip_sizes(&noise(64 * 1024), true);
    validate_zip_entry_size("payload.dat", declared, compressed).unwrap();
}

#[test]
fn test_allows_a_tiny_entry_regardless_of_ratio() {
    let (declared, compressed) = zip_sizes(&[0u8; 1024], true);
    validate_zip_entry_size("payload.dat", declared, compressed).unwrap();
}

#[test]
fn test_allows_an_empty_entry() {
    let (declared, compressed) = zip_sizes(b"", false);
    validate_zip_entry_size("payload.dat", declared, compressed).unwrap();
}

// ── TestSkpFile ──

#[test]
fn test_open_missing_file() {
    assert!(matches!(
        SkpFile::open("/nonexistent/path/model.skp"),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn test_open_wrong_extension() {
    let dir = std::env::temp_dir().join(format!("openskp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("test.txt");
    std::fs::write(&fake, "hello").unwrap();
    let err = SkpFile::open(&fake).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(matches!(err, Error::NotSkp(_)));
    assert!(err.to_string().contains("Expected a .skp file"));
}

// ── TestDataModel ──

mod test_data_model {
    use openskp::{Edge, Face, Instance, Layer, Material, SkpModel, Vertex};

    #[test]
    fn test_vertex_creation() {
        let v = Vertex {
            id: 0,
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        assert_eq!(v.x, 1.0);
    }

    #[test]
    fn test_edge_creation() {
        let e = Edge {
            id: 0,
            v1_id: 1,
            v2_id: 2,
            ..Default::default()
        };
        assert_eq!(e.v1_id, 1);
    }

    #[test]
    fn test_face_defaults() {
        let f = Face::default();
        assert!(f.loops.is_empty());
        assert_eq!(f.normal, None);
    }

    #[test]
    fn test_layer_defaults() {
        assert_eq!(Layer::new("Test").color_r, 200);
    }

    #[test]
    fn test_material_defaults() {
        let mat = Material::new("Wood");
        assert_eq!(mat.transparency, 1.0);
        assert_eq!(mat.color, [200, 200, 200, 255]);
    }

    #[test]
    fn test_instance_identity_matrix() {
        let inst = Instance::default();
        assert_eq!(inst.matrix.len(), 16);
        assert_eq!(inst.matrix[0], 1.0);
        assert_eq!(inst.matrix[5], 1.0);
    }

    /// Python lists the dataclass fields; a struct literal naming every
    /// field stops compiling the day a `children` field appears.
    #[test]
    fn test_instance_has_no_children_field() {
        let inst = Instance {
            name: String::new(),
            ref_idx: None,
            guid: None,
            matrix: Vec::new(),
            layer: "L".into(),
            properties: Default::default(),
            attribute_dictionaries: Default::default(),
            material_id: None,
            hidden: false,
        };
        assert_eq!(inst.layer, "L");
        assert!(inst.properties.is_empty());
    }

    #[test]
    fn test_skp_model_defaults() {
        let model = SkpModel::default();
        assert_eq!(model.version, "unknown");
        assert!(model.definitions.is_empty());
        assert!(model.layers.is_empty());
    }
}

// ── TestJsonExport (the parsed-model ground truth only) ──

mod test_json_export {
    use crate::common::fixture;

    /// The counts `to_dict` reported for this fixture, checked on the
    /// parsed model (the JSON exporter itself is left out).
    #[test]
    fn test_real_file_matches_ground_truth() {
        let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(model.definitions.len(), 2);
        assert_eq!(model.layers.len(), 1);
        assert_eq!(model.root.vertices.len(), 251);
        assert_eq!(model.root.edges.len(), 390);
        assert_eq!(model.root.faces.len(), 146);
        assert_eq!(model.root.instances.len(), 3);
        let puerta = model
            .definitions
            .values()
            .find(|d| d.name == "puerta")
            .unwrap();
        assert_eq!(puerta.id, 40);
        assert_eq!(puerta.vertices.len(), 64);
        assert_eq!(puerta.edges.len(), 95);
        assert_eq!(puerta.faces.len(), 24);
    }
}

// ── TestUseTrans ──

mod test_use_trans {
    use crate::common::{synthetic_skp, temp_dir};

    fn xml(trans: &str, use_trans: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0"?>
<materialDocument xmlns="http://sketchup.google.com/schemas/sketchup/1.0/material"
                  xmlns:mat="http://sketchup.google.com/schemas/sketchup/1.0/material">
  <mat:material name="M" colorRed="1" colorGreen="2" colorBlue="3"
                trans="{trans}" useTrans="{use_trans}"/>
</materialDocument>
"#
        )
        .into_bytes()
    }

    fn parse(tag: &str, trans: &str, use_trans: &str) -> openskp::SkpModel {
        let dir = temp_dir(tag);
        let x = xml(trans, use_trans);
        let path = synthetic_skp(&dir, "m.skp", &[("materials/M/material.xml", &x)]);
        openskp::SkpFile::open(path).unwrap().parse().unwrap()
    }

    #[test]
    fn test_use_trans_1_applies() {
        // trans stores a TRANSPARENCY: trans="0.27" reads back as 0.73.
        let model = parse("usetrans1", "0.27", "1");
        assert!((model.materials[0].transparency - 0.73).abs() < 1e-9);
    }

    #[test]
    fn test_use_trans_0_means_opaque() {
        let model = parse("usetrans0", "0", "0");
        assert_eq!(model.materials[0].transparency, 1.0);
    }
}

// ── TestUtf8EntityNames ──

mod test_utf8_entity_names {
    use crate::common::tlv;
    use openskp::parsed::GeometryBuilder;
    use openskp::tlv::{parse_tlv_recursive, CONTAINER_TAGS};
    use openskp::vff::extract_geometry_from_nodes;

    #[test]
    fn test_instance_name_keeps_accents() {
        let name = "Diseño de árbol".as_bytes();
        let mut node = tlv("6419", &[tlv("6519", name), tlv("6719", b"\x05")].concat());
        let len = node.len() + 7;
        node.extend(tlv("0100", b"\x00"));
        let elements = parse_tlv_recursive(&node, 0, len, &CONTAINER_TAGS).unwrap();
        let mut builder = GeometryBuilder::default();
        extract_geometry_from_nodes(&elements, &mut builder).unwrap();
        assert_eq!(
            builder.instances[0].name.as_deref(),
            Some("Diseño de árbol")
        );
    }
}

// ── TestMaterialIdJoin, TestLayerHidden, TestFaceInstanceHidden (a
// stubbed `full_parse` becomes a hand-built `Parsed`) ──

mod test_material_id_join {
    use openskp::parsed::RawMaterial;
    use openskp::{Material, Parsed, SkpModel};

    fn raw(name: &str, rgb: [i64; 3]) -> RawMaterial {
        RawMaterial {
            name: name.into(),
            color: rgb,
            alpha: None,
            transparency: 1.0,
            colorized: false,
            colorize_type: 0,
            texture: None,
        }
    }

    #[test]
    fn test_material_id_defaults_to_none() {
        assert_eq!(Material::new("Wood").id, None);
    }

    #[test]
    fn test_face_material_resolves_through_materials_by_id() {
        let mut parsed = Parsed {
            version: "test".into(),
            ..Default::default()
        };
        parsed.material_data.push(raw("Wood", [10, 20, 30]));
        parsed.materials.insert("Wood".into(), 0);
        parsed.material_id_to_name.insert(29491, "Wood".into());
        let model = SkpModel::from_parsed(parsed);
        assert_eq!(model.materials[0].id, Some(29491));
        assert_eq!(model.materials_by_id[&29491], 0);
        assert_eq!(model.material(29491).unwrap().color, [10, 20, 30, 255]);
    }

    #[test]
    fn test_folder_alias_resolves_to_same_material() {
        // The TLV name may match the ZIP folder rather than the XML name.
        let mut parsed = Parsed::default();
        parsed.material_data.push(raw("Wood", [1, 2, 3]));
        parsed.materials.insert("Wood".into(), 0);
        parsed.materials_by_folder.insert("m0".into(), 0);
        parsed.material_id_to_name.insert(7, "Wood".into());
        parsed.material_id_to_name.insert(8, "m0".into());
        let model = SkpModel::from_parsed(parsed);
        assert_eq!(model.materials.len(), 1);
        assert_eq!(model.materials_by_id[&7], model.materials_by_id[&8]);
        assert!(matches!(model.materials[0].id, Some(7 | 8)));
    }

    #[test]
    fn test_unresolvable_id_is_skipped() {
        let mut parsed = Parsed::default();
        parsed.material_id_to_name.insert(99, "Ghost".into());
        assert!(SkpModel::from_parsed(parsed).materials_by_id.is_empty());
    }
}

mod test_layer_hidden {
    use crate::common::fixture;
    use openskp::{Parsed, SkpModel};

    #[test]
    fn test_hidden_layer_is_reported_hidden() {
        let mut parsed = Parsed::default();
        parsed.layer_colors.insert("Layer0".into(), (136, 136, 136));
        parsed
            .layer_colors
            .insert("Furniture".into(), (200, 50, 50));
        parsed.layer_hidden.insert("Layer0".into(), false);
        parsed.layer_hidden.insert("Furniture".into(), true);
        let model = SkpModel::from_parsed(parsed);
        let hidden = |n: &str| model.layers.iter().find(|l| l.name == n).unwrap().hidden;
        assert!(!hidden("Layer0"));
        assert!(hidden("Furniture"));
    }

    #[test]
    fn test_missing_layer_hidden_dict_defaults_to_visible() {
        let mut parsed = Parsed::default();
        parsed.layer_colors.insert("Layer0".into(), (136, 136, 136));
        assert!(!SkpModel::from_parsed(parsed).layers[0].hidden);
    }

    #[test]
    fn test_real_legacy_fixture_layer_is_not_hidden() {
        let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(model.layers.len(), 1);
        assert!(!model.layers[0].hidden);
    }
}

// ── TestVffLayerHidden ──

mod test_vff_layer_hidden {
    use crate::common::{temp_dir, tlv};

    fn layer_node(id: u8, name: &str, hidden: bool) -> Vec<u8> {
        [
            tlv("DC05", &[id]),
            tlv("8D3C", name.as_bytes()),
            tlv("8E3C", &[u8::from(hidden)]),
        ]
        .concat()
    }

    #[test]
    fn test_hidden_and_visible_layers_read_correctly() {
        use std::io::Write;
        let layers = [
            tlv("8C3C", &layer_node(5, "wall_external_cladding_1", true)),
            tlv("8C3C", &layer_node(6, "wall", false)),
        ]
        .concat();
        let model_dat = tlv("993A", &layers);
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("model.dat", opts).unwrap();
        zw.write_all(&model_dat).unwrap();
        zw.finish().unwrap();
        // The ZIP starts right after the magic (is_legacy looks there).
        let mut data = vec![0xFF, 0xFE, 0xFF, 0x0E];
        data.extend_from_slice(buf.get_ref());
        let path = temp_dir("vfflayers").join("vff_layers.skp");
        std::fs::write(&path, data).unwrap();

        let parsed = openskp::full_parse(&path).unwrap();
        assert!(parsed.layer_hidden["wall_external_cladding_1"]);
        assert!(!parsed.layer_hidden["wall"]);
    }
}

mod test_face_instance_hidden {
    use crate::common::fixture;
    use openskp::parsed::{root_definition as root, GeometryBuilder, RawFace, RawInstance};
    use openskp::{Parsed, SkpModel};

    fn face(hidden: bool) -> RawFace {
        RawFace {
            normal: Some([0.0, 0.0, 1.0]),
            hidden,
            ..Default::default()
        }
    }

    fn instance(name: &str, hidden: bool) -> RawInstance {
        RawInstance {
            name: Some(name.into()),
            ref_guid: Some(String::new()),
            hidden,
            ..Default::default()
        }
    }

    #[test]
    fn test_hidden_face_and_instance_are_reported_hidden() {
        let mut builder = GeometryBuilder::default();
        builder.faces.insert(1, face(true));
        builder.faces.insert(2, face(false));
        builder.instances.push(instance("hidden_one", true));
        builder.instances.push(instance("visible_one", false));
        let model = SkpModel::from_parsed(Parsed {
            root: Some(root(builder)),
            ..Default::default()
        });
        assert!(model.root.faces[&1].hidden);
        assert!(!model.root.faces[&2].hidden);
        let by_name = |n: &str| {
            model
                .root
                .instances
                .iter()
                .find(|i| i.name == n)
                .unwrap()
                .hidden
        };
        assert!(by_name("hidden_one"));
        assert!(!by_name("visible_one"));
    }

    #[test]
    fn test_missing_hidden_key_defaults_to_visible() {
        let mut builder = GeometryBuilder::default();
        builder.faces.insert(1, RawFace::default());
        builder.instances.push(RawInstance::default());
        let model = SkpModel::from_parsed(Parsed {
            root: Some(root(builder)),
            ..Default::default()
        });
        assert!(!model.root.faces[&1].hidden);
        assert!(!model.root.instances[0].hidden);
    }

    #[test]
    fn test_real_legacy_fixture_faces_and_instances_are_not_hidden() {
        let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(!model.definitions.is_empty());
        for d in model.definitions.values() {
            assert!(d.faces.values().all(|f| !f.hidden));
        }
        assert!(model.root.instances.iter().all(|i| !i.hidden));
    }
}

// ── TestMetaUnits ──

mod test_meta_units {
    use crate::common::fixture;
    use openskp::container::read_meta_units;
    use openskp::{Parsed, SkpModel};

    fn tlv(tag: &[u8; 2], payload: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn test_extracts_units_from_real_fixture_bytes() {
        // The 388-byte meta/meta.dat of Untitled.skp (SketchUp 25.0.575).
        let real: &[u8] = &[
            b"d\x00~\x01\x00\x00u\x00\x08\x00\x00\x0025.0.575v\x00\x02\x00\x00\x00\x18\x00"
                .as_slice(),
            b"w\x00\x02\x00\x00\x00\x02\x00s\x00\x02\x00\x00\x00\x01\x00t\x00\x02\x00\x00",
            b"\x00\x11\x00f\x00\x10\x00\x00\x00\xdc\xd4u*8=rG\x83\x02/\xa2\x9c\xda2$",
            b"g\x00.\x00\x00\x00(#(\x00\x00\x00)#\x04\x00\x00\x00\x04\x00\x00\x00",
            b"*#\x18\x00\x00\x00meta/model_thumbnail.png",
            b"h\x000\x00\x00\x00(#*\x00\x00\x00)#\x04\x00\x00\x00\x04\x00\x00\x00",
            b"*#\x1a\x00\x00\x00meta/preview_thumbnail.png",
            b"i\x00\x01\x00\x00\x00\x01j\x00\x00\x00\x00\x00k\x00\x00\x00\x00\x00",
            b"l\x00\x00\x00\x00\x00n\x00\x00\x00\x00\x00q\x00\x01\x00\x00\x00\x00",
            b"y\x00\x01\x00\x00\x00\x00r\x00\x01\x00\x00\x00\x00",
            b"m\x00\n\x00\x00\x00Millimeter",
            b"p\x00\x01\x00\x00\x00\x01",
            b"o\x00'\x00\x00\x00E:/Devs/TEst/Skp Test/ref2/Untitled.skp",
            b"x\x00R\x00\x00\x00\xc8\x00L\x00\x00\x00\xc9\x00F\x00\x00\x00\xca\x00",
            b"@\x00\x00\x00\xcb\x00\"\x00\x00\x00SketchUp Client (Windows) 25.0.575",
            b"\xcc\x00\x04\x00\x00\x00#\xc52j",
            b"\xcd\x00\x08\x00\x00\x00\xecD=\xc9\xb4\xdb\x98w",
        ]
        .concat();
        assert_eq!(real.len(), 388);
        assert_eq!(read_meta_units(real).as_deref(), Some("Millimeter"));
    }

    #[test]
    fn test_extracts_units_from_minimal_synthetic_record() {
        let outer = tlv(b"\x64\x00", &tlv(b"\x6D\x00", b"Inches"));
        assert_eq!(read_meta_units(&outer).as_deref(), Some("Inches"));
    }

    #[test]
    fn test_returns_none_when_units_tag_absent() {
        let outer = tlv(b"\x64\x00", &tlv(b"\x75\x00", b"25.0.575"));
        assert_eq!(read_meta_units(&outer), None);
    }

    #[test]
    fn test_returns_none_for_empty_or_truncated_bytes() {
        assert_eq!(read_meta_units(b""), None);
        assert_eq!(read_meta_units(b"\x01\x02\x03"), None);
    }

    #[test]
    fn test_model_units_wired_from_parsed_dict() {
        let model = SkpModel::from_parsed(Parsed {
            units: Some("Millimeter".into()),
            ..Default::default()
        });
        assert_eq!(model.units.as_deref(), Some("Millimeter"));
    }

    #[test]
    fn test_model_units_defaults_to_none_when_absent() {
        assert_eq!(SkpModel::from_parsed(Parsed::default()).units, None);
    }

    #[test]
    fn test_real_legacy_fixture_has_no_units() {
        let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(model.units, None);
    }
}

// ── TestInstanceMaterial ──

mod test_instance_material {
    use crate::common::tlv;
    use openskp::parsed::GeometryBuilder;
    use openskp::tlv::{parse_tlv_recursive, CONTAINER_TAGS};
    use openskp::vff::extract_geometry_from_nodes;
    use openskp::Instance;

    fn extract(node: &[u8]) -> GeometryBuilder {
        let mut data = node.to_vec();
        data.extend(tlv("0100", b"\x00"));
        let elements = parse_tlv_recursive(&data, 0, node.len() + 7, &CONTAINER_TAGS).unwrap();
        let mut builder = GeometryBuilder::default();
        extract_geometry_from_nodes(&elements, &mut builder).unwrap();
        builder
    }

    #[test]
    fn test_instance_material_defaults_to_none() {
        assert_eq!(Instance::default().material_id, None);
    }

    #[test]
    fn test_extractor_reads_instance_d007_material() {
        let d007 = tlv("D007", &tlv("D107", &[0x33, 0x73])); // id 0x7333
        let reference = tlv("6719", &[0x05]);
        let ones: Vec<u8> = std::iter::repeat_n(1.0f64.to_le_bytes(), 13)
            .flatten()
            .collect();
        let matrix = tlv("6619", &ones);
        let node = tlv("6419", &[reference, matrix, d007].concat());
        let builder = extract(&node);
        assert_eq!(builder.instances.len(), 1);
        assert_eq!(builder.instances[0].ref_idx, Some(5));
        assert_eq!(builder.instances[0].material_id, Some(0x7333));
    }

    #[test]
    fn test_instance_without_material_stays_none() {
        let node = tlv("6419", &tlv("6719", &[0x05]));
        assert_eq!(extract(&node).instances[0].material_id, None);
    }
}

// ── TestStyles ──

mod test_styles {
    use crate::common::{synthetic_skp, temp_dir};

    #[test]
    fn test_style_colors_via_synthetic_skp() {
        let style_xml = br#"<?xml version="1.0"?>
<styleDocument xmlns="http://sketchup.google.com/schemas/sketchup/1.0/style"
               xmlns:sty="http://sketchup.google.com/schemas/sketchup/1.0/style">
  <sty:style xmlns:t="http://sketchup.google.com/schemas/1.0/types" name="Verde">
    <sty:item id="4000"><t:variant type="4">-3552052</t:variant></sty:item>
    <sty:item id="4001"><t:variant type="4">-3093050</t:variant></sty:item>
  </sty:style>
</styleDocument>
"#;
        let path = synthetic_skp(
            &temp_dir("styles"),
            "s.skp",
            &[("styles/Verde/style.xml", style_xml)],
        );
        let model = openskp::SkpFile::open(path).unwrap().parse().unwrap();
        assert_eq!(model.styles.len(), 1);
        let st = &model.styles[0];
        assert_eq!(st.name, "Verde");
        let rgb = |v: i64| {
            let v = (v & 0xFFFF_FFFF) as u32;
            [(v >> 16) as u8, (v >> 8) as u8, v as u8]
        };
        assert_eq!(st.front_color, Some(rgb(-3552052)));
        assert_eq!(st.back_color, Some(rgb(-3093050)));
    }
}

// ── TestXmlEntityExpansion ──

mod test_xml_entity_expansion {
    use crate::common::{synthetic_skp, temp_dir};

    const ENTITY_BOMB: &[u8] = br#"<?xml version="1.0"?>
<!DOCTYPE r [ <!ENTITY x "hi"> ]>
<r>&x;</r>
"#;

    #[test]
    fn test_full_parse_skips_malicious_material_and_style_xml() {
        let path = synthetic_skp(
            &temp_dir("evil"),
            "evil.skp",
            &[
                ("materials/Evil/material.xml", ENTITY_BOMB),
                ("styles/Evil/style.xml", ENTITY_BOMB),
            ],
        );
        let model = openskp::SkpFile::open(path).unwrap().parse().unwrap();
        assert!(model.materials.is_empty());
        assert!(model.styles.is_empty());
    }
}

// ── TestFaceUvTransform ──

mod test_face_uv_transform {
    use crate::common::tlv;
    use openskp::vff::extract_uv_transforms;
    use openskp::Face;

    const ROT90: [f64; 9] = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 96.0, -96.0, 1.0];

    fn mat_bytes(m: &[f64; 9]) -> Vec<u8> {
        m.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn dc05(front: Option<[f64; 9]>, back: Option<[f64; 9]>) -> Vec<u8> {
        let side = |tag: &str, m: &[f64; 9]| {
            tlv(
                tag,
                &tlv(
                    "1327",
                    &[tlv("1427", b"\x01"), tlv("1527", &mat_bytes(m))].concat(),
                ),
            )
        };
        let mut inner = Vec::new();
        if let Some(f) = front {
            inner.extend(side("1127", &f));
        }
        if let Some(b) = back {
            inner.extend(side("1227", &b));
        }
        let t1027 = tlv("1027", &inner);
        [
            tlv("DE05", b"\x2A"),
            tlv("DD05", &tlv("B136", &tlv("B236", &t1027))),
        ]
        .concat()
    }

    #[test]
    fn test_extracts_front_matrix() {
        let (front, back) = extract_uv_transforms(&dc05(Some(ROT90), None));
        assert_eq!(front, Some(ROT90));
        assert_eq!(back, None);
    }

    #[test]
    fn test_extracts_both_sides() {
        let other = ROT90.map(|v| v * 2.0);
        let (front, back) = extract_uv_transforms(&dc05(Some(ROT90), Some(other)));
        assert_eq!(front, Some(ROT90));
        assert_eq!(back, Some(other));
    }

    #[test]
    fn test_untouched_texture_has_no_transform() {
        assert_eq!(extract_uv_transforms(&tlv("DE05", b"\x2A")), (None, None));
        assert_eq!(Face::default().uv_transform, None);
        assert_eq!(Face::default().uv_transform_back, None);
    }

    /// Ground truth from a controlled SketchUp file: UV = [x, y, 1] @
    /// inv(M) / tile.
    #[test]
    fn test_recipe_reproduces_known_uvs() {
        let m = ROT90;
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        #[rustfmt::skip]
        let inv = [
            (m[4] * m[8] - m[5] * m[7]) / det, (m[2] * m[7] - m[1] * m[8]) / det, (m[1] * m[5] - m[2] * m[4]) / det,
            (m[5] * m[6] - m[3] * m[8]) / det, (m[0] * m[8] - m[2] * m[6]) / det, (m[2] * m[3] - m[0] * m[5]) / det,
            (m[3] * m[7] - m[4] * m[6]) / det, (m[1] * m[6] - m[0] * m[7]) / det, (m[0] * m[4] - m[1] * m[3]) / det,
        ];
        let tile = 48.0;
        for ((x, y), (ut, vt)) in [
            ((82.64, 0.0), (2.0, 0.2784)),
            ((122.01, 0.0), (2.0, -0.5418)),
            ((82.64, 39.37), (2.8202, 0.2784)),
        ] {
            let row = [x, y, 1.0];
            let uvq: Vec<f64> = (0..3)
                .map(|c| (0..3).map(|r| row[r] * inv[r * 3 + c]).sum())
                .collect();
            let u = uvq[0] / uvq[2] / tile;
            let v = uvq[1] / uvq[2] / tile;
            assert!((u - ut).abs() <= 2e-3, "{u} vs {ut}");
            assert!((v - vt).abs() <= 2e-3, "{v} vs {vt}");
        }
    }
}

// ── TestBackMaterial ──

mod test_back_material {
    use crate::common::tlv;
    use openskp::parsed::GeometryBuilder;
    use openskp::tlv::{parse_tlv_recursive, CONTAINER_TAGS};
    use openskp::vff::extract_geometry_from_nodes;
    use openskp::Face;

    #[test]
    fn test_back_material_extracted() {
        let node = tlv(
            "AC0D",
            &[
                tlv("DC05", &tlv("DE05", b"\x2A")),
                tlv("AF0D", &[0x85, 0x8B, 0x06]),
            ]
            .concat(),
        );
        let mut data = node.clone();
        data.extend(tlv("0100", b"\x00"));
        let elements = parse_tlv_recursive(&data, 0, node.len() + 7, &CONTAINER_TAGS).unwrap();
        let mut builder = GeometryBuilder::default();
        extract_geometry_from_nodes(&elements, &mut builder).unwrap();
        let f = &builder.faces[&0x2A];
        assert_eq!(f.material_id, None); // front unpainted
        assert_eq!(f.back_material_id, Some(0x68B85)); // back painted
    }

    #[test]
    fn test_face_defaults() {
        assert_eq!(Face::default().back_material_id, None);
    }
}

// ── TestFaceCameraBehavior ──

mod test_face_camera_behavior {
    use crate::common::{synthetic_skp, temp_dir, tlv};
    use openskp::tlv::{parse_tlv_recursive, CONTAINER_TAGS};

    #[test]
    fn test_flag_set() {
        let flags = [
            tlv("5B1B", b"\x00"),
            tlv("5D1B", b"\x01"),
            tlv("5E1B", b"\x01"),
        ]
        .concat();
        let node = tlv(
            "7C15",
            &[tlv("7E15", b"Susan"), tlv("581B", &flags)].concat(),
        );
        let mut data = node.clone();
        data.extend(tlv("0100", b"\x00"));
        let elements = parse_tlv_recursive(&data, 0, node.len() + 7, &CONTAINER_TAGS).unwrap();
        assert_eq!(elements[0].tag.to_string(), "7C15");
    }

    #[test]
    fn test_flag_via_synthetic_skp() {
        let def = |guid: u8, name: &[u8], on: u8, id: u8| {
            tlv(
                "7C15",
                &[
                    tlv("7D15", &[guid; 16]),
                    tlv("7E15", name),
                    tlv("581B", &[tlv("5D1B", &[on]), tlv("5E1B", &[on])].concat()),
                    tlv("DC05", &tlv("DE05", &[id])),
                ]
                .concat(),
            )
        };
        let defs = [def(0x11, b"Susan", 1, 5), def(0x22, b"Silla", 0, 6)].concat();
        let model_dat = tlv("F901", &tlv("7017", &tlv("7117", &defs)));
        let path = synthetic_skp(&temp_dir("facecam"), "s.skp", &[("model.dat", &model_dat)]);
        let model = openskp::SkpFile::open(path).unwrap().parse().unwrap();
        let by_name = |n: &str| model.definitions.values().find(|d| d.name == n).unwrap();
        assert!(by_name("Susan").always_faces_camera);
        assert!(by_name("Susan").shadows_face_sun);
        assert!(!by_name("Silla").always_faces_camera);
        assert!(!by_name("Silla").shadows_face_sun);
    }
}

// ── TestSectionPlaneTextDimension ──

mod test_section_plane_text_dimension {
    use openskp::{Definition, Dimension, SectionPlane, TextEntity};

    #[test]
    fn test_default_lists_empty() {
        let d = Definition::default();
        assert!(d.section_planes.is_empty());
        assert!(d.texts.is_empty());
        assert!(d.dimensions.is_empty());

        let sp = SectionPlane {
            plane: vec![0.0, 0.0, 1.0, 0.0],
            name: "Cut 1".into(),
            label: "A".into(),
            ..Default::default()
        };
        assert_eq!(sp.name, "Cut 1");
        assert!(!sp.hidden);

        let txt = TextEntity {
            text: "Note".into(),
            ..Default::default()
        };
        assert_eq!(txt.text, "Note");

        let dim = Dimension {
            text: "100mm".into(),
            ..Default::default()
        };
        assert_eq!(dim.text, "100mm");
    }
}

// ── TestImageEntities ──

mod test_image_entities {
    use crate::common::tlv;
    use openskp::parsed::{GeometryBuilder, RawDefinition};
    use openskp::tlv::{parse_tlv_recursive, CONTAINER_TAGS};
    use openskp::vff::extract_geometry_from_nodes;
    use openskp::{Definition, Parsed, SkpModel};

    #[test]
    fn test_image_placement_instance_is_extracted() {
        let inner = tlv("6419", &tlv("6719", b"\x07"));
        let node = tlv("9013", &tlv("401F", &inner));
        let mut data = node.clone();
        data.extend(tlv("0100", b"\x00"));
        let elements = parse_tlv_recursive(&data, 0, node.len() + 7, &CONTAINER_TAGS).unwrap();
        let mut builder = GeometryBuilder::default();
        extract_geometry_from_nodes(&elements, &mut builder).unwrap();
        assert_eq!(builder.instances.len(), 1);
        assert_eq!(builder.instances[0].ref_idx, Some(7));
    }

    #[test]
    fn test_definition_kind_2_marks_is_image() {
        let mut parsed = Parsed {
            version: "test".into(),
            ..Default::default()
        };
        for (id, name, is_image) in [(1, "imagen#1", true), (2, "Grupo", false)] {
            parsed.defs.insert(
                Some(id),
                RawDefinition {
                    guid: Some(String::new()),
                    name: Some(name.into()),
                    is_image,
                    ..Default::default()
                },
            );
        }
        let model = SkpModel::from_parsed(parsed);
        assert!(model.definitions[&Some(1)].is_image);
        assert!(!model.definitions[&Some(2)].is_image);
        assert!(!Definition::default().is_image);
    }
}

// ── TestTextureExtraction ──

mod test_texture_extraction {
    use crate::common::{synthetic_skp, temp_dir};
    use openskp::Texture;

    fn textured(name: &str, file: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<materialDocument xmlns="http://sketchup.google.com/schemas/sketchup/1.0/material"
                  xmlns:mat="http://sketchup.google.com/schemas/sketchup/1.0/material">
  <mat:material name="{name}" type="1" colorRed="10" colorGreen="20"
                colorBlue="30" trans="1" hasTexture="1">
    <mat:texture textureFilename="{file}" xScale="24" yScale="12"/>
  </mat:material>
</materialDocument>
"#
        )
        .into_bytes()
    }

    const PLAIN: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<materialDocument xmlns="http://sketchup.google.com/schemas/sketchup/1.0/material"
                  xmlns:mat="http://sketchup.google.com/schemas/sketchup/1.0/material">
  <mat:material name="Plain" type="0" colorRed="1" colorGreen="2"
                colorBlue="3" trans="1" hasTexture="0"/>
</materialDocument>
"#;

    #[test]
    fn test_texture_dataclass_save() {
        let tex = Texture {
            filename: "wood.jpg".into(),
            width: 24.0,
            height: 12.0,
            data: Some(b"\xff\xd8fakejpeg".to_vec()),
        };
        let out = tex.save(temp_dir("texsave").join("out.jpg")).unwrap();
        assert_eq!(std::fs::read(out).unwrap(), b"\xff\xd8fakejpeg");
    }

    #[test]
    fn test_texture_save_without_data_raises() {
        let tex = Texture {
            filename: "missing.jpg".into(),
            ..Default::default()
        };
        let path = temp_dir("texnodata").join("never-written.jpg");
        let err = tex.save(&path).unwrap_err();
        assert!(err.to_string().contains("no image data"));
        assert!(!path.exists());
    }

    #[test]
    fn test_textured_material_roundtrip() {
        let jpeg = b"\xff\xd8syntheticjpegbytes";
        let wood = textured("Wood", "wood.jpg");
        let path = synthetic_skp(
            &temp_dir("texround"),
            "synthetic.skp",
            &[
                ("materials/Wood/material.xml", &wood),
                ("materials/Wood/wood.jpg", jpeg),
                ("materials/Plain/material.xml", PLAIN),
            ],
        );
        let model = openskp::SkpFile::open(path).unwrap().parse().unwrap();
        let by_name = |n: &str| model.materials.iter().find(|m| m.name == n).unwrap();
        let tex = by_name("Wood").texture.as_ref().unwrap();
        assert_eq!(tex.filename, "wood.jpg");
        assert_eq!(tex.width, 24.0);
        assert_eq!(tex.height, 12.0);
        assert_eq!(tex.data.as_deref(), Some(&jpeg[..]));
        assert!(by_name("Plain").texture.is_none());
    }

    #[test]
    fn test_image_name_mismatch_falls_back_to_sibling() {
        let jpeg = b"\xff\xd8siblingbytes";
        let glass = textured("Glass", "glass_safety.jpg");
        let path = synthetic_skp(
            &temp_dir("texsibling"),
            "synthetic.skp",
            &[
                ("materials/Glass/material.xml", &glass),
                ("materials/Glass/glass_saftey.jpg", jpeg),
            ],
        );
        let model = openskp::SkpFile::open(path).unwrap().parse().unwrap();
        let glass = model.materials.iter().find(|m| m.name == "Glass").unwrap();
        assert_eq!(
            glass.texture.as_ref().unwrap().data.as_deref(),
            Some(&jpeg[..])
        );
    }

    #[test]
    fn test_colorized_copy_shares_source_image() {
        let png = b"\x89PNGsharedchainlink";
        let colorized = br#"<?xml version="1.0" encoding="UTF-8"?>
<materialDocument xmlns="http://sketchup.google.com/schemas/sketchup/1.0/material"
                  xmlns:mat="http://sketchup.google.com/schemas/sketchup/1.0/material">
  <mat:material name="[Fence]1" type="2" colorRed="27" colorGreen="135"
                colorBlue="59" colorizeType="0" trans="1" hasTexture="1">
    <mat:texture textureFilename="fence.png" xScale="2.75" yScale="2.75">
      <mat:images>
        <mat:image id="1" path="materials/Fence/fence.png"
                   file_name="fence.png"/>
      </mat:images>
    </mat:texture>
  </mat:material>
</materialDocument>
"#;
        let fence = textured("Fence", "fence.png");
        let path = synthetic_skp(
            &temp_dir("texcolorized"),
            "synthetic.skp",
            &[
                ("materials/Fence/material.xml", &fence),
                ("materials/Fence/fence.png", png),
                ("materials/[Fence]1/material.xml", colorized),
            ],
        );
        let model = openskp::SkpFile::open(path).unwrap().parse().unwrap();
        let by_name = |n: &str| model.materials.iter().find(|m| m.name == n).unwrap();
        let copy = by_name("[Fence]1");
        assert_eq!(
            copy.texture.as_ref().unwrap().data.as_deref(),
            Some(&png[..])
        );
        assert!(copy.colorized);
        assert_eq!(copy.colorize_type, 0);
        assert_eq!(copy.color[..3], [27, 135, 59]);
        assert!(!by_name("Fence").colorized);
    }
}

// ── TestLegacyDetection ──

mod test_legacy_detection {
    use openskp::legacy::is_legacy;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn test_classic_header_detected() {
        let data = [
            b"\xFF\xFE\xFF\x0E".to_vec(),
            utf16("SketchUp Model"),
            b"\xFF\xFE\xFF\x0C".to_vec(),
            utf16("{16.0.19912}"),
            vec![0; 0x20],
            b"\xFF\xFF\x00\x00\x0B\x00CVersionMap".to_vec(),
        ]
        .concat();
        assert!(is_legacy(&data));
    }

    #[test]
    fn test_vff_zip_not_legacy() {
        let data = [
            b"\xFF\xFE\xFF\x0E".to_vec(),
            utf16("SketchUp Model"),
            b"\xFF\xFE\xFF\x08".to_vec(),
            utf16("{26.2.0}"),
            b"VFF\x08PK\x03\x04".to_vec(),
            vec![0; 64],
        ]
        .concat();
        assert!(!is_legacy(&data));
    }

    #[test]
    fn test_random_bytes_not_legacy() {
        assert!(!is_legacy(&[0u8; 64]));
    }
}

// ── TestLegacyStrings ──

mod test_legacy_strings {
    use openskp::legacy::{Cursor, LegacyError};

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn test_short_string() {
        let data = [b"\xFF\xFE\xFF\x04".to_vec(), utf16("Casa")].concat();
        assert_eq!(Cursor::new(&data, 0).utf16().unwrap(), "Casa");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(Cursor::new(b"\xFF\xFE\xFF\x00", 0).utf16().unwrap(), "");
    }

    #[test]
    fn test_escalated_length() {
        let text = "x".repeat(300);
        let data = [
            b"\xFF\xFE\xFF\xFF".to_vec(),
            300u16.to_le_bytes().to_vec(),
            utf16(&text),
        ]
        .concat();
        assert_eq!(Cursor::new(&data, 0).utf16().unwrap(), text);
    }

    #[test]
    fn test_not_a_string_raises() {
        assert!(matches!(
            Cursor::new(b"\x00\x00\x00\x00", 0).utf16(),
            Err(LegacyError::Parse(_))
        ));
    }
}

// ── TestLegacyRealFile, TestModernRealFile ──

/// `_assert_connected_normalized_loops`: every loop's coedges carry ±1 and
/// connect head to tail.
fn assert_connected_normalized_loops<'a>(
    definitions: impl Iterator<Item = &'a openskp::Definition>,
) {
    for d in definitions {
        for face in d.faces.values() {
            for lp in &face.loops {
                assert!(!lp.is_empty(), "empty loop");
                let n = lp.len();
                for i in 0..n {
                    let (edge_id, orientation) = lp[i];
                    let (next_id, next_orientation) = lp[(i + 1) % n];
                    assert!(orientation == 1 || orientation == -1);
                    let edge = &d.edges[&edge_id];
                    let next = &d.edges[&next_id];
                    let end = if orientation == 1 {
                        edge.v2_id
                    } else {
                        edge.v1_id
                    };
                    let next_start = if next_orientation == 1 {
                        next.v1_id
                    } else {
                        next.v2_id
                    };
                    assert_eq!(end, next_start);
                }
            }
        }
    }
}

mod test_legacy_real_file {
    use crate::common::fixture;
    use openskp::SkpModel;

    fn model() -> SkpModel {
        openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn test_version_and_counts() {
        let model = model();
        assert_eq!(model.version, "{17.0.18899}");
        let all = || model.definitions.values().chain([&model.root]);
        assert_eq!(all().map(|d| d.faces.len()).sum::<usize>(), 181);
        assert_eq!(all().map(|d| d.edges.len()).sum::<usize>(), 515);
        assert_eq!(all().map(|d| d.vertices.len()).sum::<usize>(), 335);
        assert_eq!(model.materials.len(), 16);
    }

    #[test]
    fn test_bounding_box() {
        let model = model();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for d in model.definitions.values().chain([&model.root]) {
            for v in d.vertices.values() {
                for (i, c) in [v.x, v.y, v.z].into_iter().enumerate() {
                    lo[i] = lo[i].min(c);
                    hi[i] = hi[i].max(c);
                }
            }
        }
        for (got, want) in lo.iter().zip([-326.719, 0.0, -23.419]) {
            assert!((got - want).abs() <= 1e-2, "{lo:?}");
        }
        for (got, want) in hi.iter().zip([784.443, 222.211, 145.976]) {
            assert!((got - want).abs() <= 1e-2, "{hi:?}");
        }
    }

    #[test]
    fn test_has_instances_and_geometry() {
        let model = model();
        assert_eq!(model.definitions.len(), 2);
        let all = || model.definitions.values().chain([&model.root]);
        assert!(all().map(|d| d.instances.len()).sum::<usize>() >= 2);
        let a_face = all()
            .flat_map(|d| d.faces.values())
            .find(|f| !f.loops.is_empty())
            .unwrap();
        assert!(a_face.loops[0].len() >= 3);
    }

    #[test]
    fn test_root_is_a_separate_field_not_a_definitions_entry() {
        let model = model();
        assert_eq!(model.root.name, "ROOT_MODEL");
        assert_eq!(model.root.guid.as_deref(), Some("ROOT"));
        assert!(model.root.instances.len() >= 2);
        assert!(model.definitions.keys().all(|k| k.is_some()));
    }

    #[test]
    fn test_coedge_orientations_are_normalized_and_connected() {
        let model = model();
        super::assert_connected_normalized_loops(model.definitions.values().chain([&model.root]));
    }
}

mod test_modern_real_file {
    use crate::common::fixture;
    use openskp::{AttrValue, SkpModel};

    fn model(name: &str) -> SkpModel {
        openskp::SkpFile::open(fixture(name))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn test_untitled_skp_matches_ground_truth() {
        let model = model("Untitled.skp");
        assert_eq!(model.version, "{25.0.575}");
        assert_eq!(model.units.as_deref(), Some("Millimeter"));

        assert_eq!(model.layers.len(), 14);
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        for want in [
            "Layer0",
            "BottomPlate",
            "TopPlate",
            "Stud",
            "Nog",
            "KingStud",
            "HeaderJackStud",
            "HeaderPlate1",
            "HeaderPlate2",
            "SillPlate1",
            "VerticalHeaderStud",
            "generic_frame",
            "dimension",
            "Hat Sections",
        ] {
            assert!(names.contains(&want), "{want}");
        }
        let layer0 = model.layers.iter().find(|l| l.name == "Layer0").unwrap();
        assert_eq!(
            (layer0.color_r, layer0.color_g, layer0.color_b),
            (136, 136, 136)
        );
        assert!(model.layers.iter().all(|l| !l.hidden));

        assert_eq!(model.materials.len(), 15);
        let mat0 = model
            .materials
            .iter()
            .find(|m| m.name == "Layer_Layer0")
            .unwrap();
        assert_eq!(mat0.transparency, 1.0);
        assert_eq!(mat0.id, None);
        assert!(mat0.texture.is_none());
        assert!(!mat0.colorized);
        assert_eq!(mat0.colorize_type, 0);

        // TLV id 26180 resolves to the default "*" material, and the join
        // points at the material in `materials`.
        let joined = model.material(26180).unwrap();
        assert_eq!(joined.name, "*");
        assert_eq!(joined.id, Some(26180));
        assert!(model.materials.contains(joined));

        let batten = model
            .root
            .instances
            .iter()
            .find(|i| i.name == "BattenHatSection_1")
            .unwrap();
        assert_eq!(batten.layer, "Hat Sections");
        let w1 = model
            .root
            .instances
            .iter()
            .find(|i| i.name == "W1")
            .unwrap();
        assert!(w1.properties.is_empty());
        let steelframer = &w1.attribute_dictionaries["steelframer-dict"];
        assert_eq!(
            steelframer["generator"],
            AttrValue::Str("SteelFramer::Engine::PanelGenerator".into())
        );
        assert_eq!(steelframer["profile"], AttrValue::Str("362S200-43".into()));

        assert_eq!(model.definitions.len(), 46);
        let def66 = &model.definitions[&Some(66)];
        assert_eq!(def66.name, "Group200#2");
        assert_eq!(def66.guid.as_deref().map(str::len), Some(32));
        assert!(!def66.always_faces_camera);
        assert!(!def66.is_image);

        assert_eq!(def66.vertices.len(), 136);
        assert_eq!(def66.edges.len(), 158);
        let first_edge = def66.edges.values().next().unwrap();
        assert!(!first_edge.soft && !first_edge.smooth && !first_edge.hidden);
        assert_eq!(def66.faces.len(), 26);
        let first_face = def66.faces.values().next().unwrap();
        assert!(!first_face.loops.is_empty());
        assert!(first_face.normal.is_some());
        assert_eq!(first_face.back_material_id, None);
        assert_eq!(first_face.uv_transform, None);
        assert_eq!(first_face.uv_transform_back, None);
        assert!(def66.faces.values().all(|f| !f.hidden));

        assert_eq!(model.styles.len(), 2);
        assert_eq!(model.styles[0].name, "[Construction Documentation Style]");
        assert_eq!(model.styles[0].front_color, Some([255, 255, 255]));
        assert_eq!(model.styles[0].back_color, Some([208, 209, 189]));
    }

    /// The parse half; the `build_scene` half is left out.
    #[test]
    fn test_su_file_skp_matches_ground_truth() {
        let model = model("SU_File.skp");
        assert_eq!(model.version, "{25.0.575}");
        assert_eq!(model.layers.len(), 1);
        assert_eq!(model.layers[0].name, "Layer0");
        assert_eq!(model.materials.len(), 1);
        assert_eq!(model.materials[0].name, "Layer_Layer0");
        assert!(model.definitions.is_empty());
    }

    #[test]
    fn test_coedge_orientations_are_normalized_and_connected() {
        let model = model("Untitled.skp");
        super::assert_connected_normalized_loops(model.definitions.values().chain([&model.root]));
    }
}

// ── TestLegacyDynamicProperties ──

mod test_legacy_dynamic_properties {
    use std::rc::Rc;

    use crate::common::fixture;
    use openskp::legacy::{extract_legacy_attribute_dictionaries, stringify_attr_value, Obj};
    use openskp::AttrValue;

    #[test]
    fn test_stringify_scalar_values() {
        assert_eq!(stringify_attr_value(&AttrValue::Null), "");
        assert_eq!(stringify_attr_value(&AttrValue::Int(42)), "42");
        assert_eq!(stringify_attr_value(&AttrValue::Float(3.5)), "3.5");
        assert_eq!(
            stringify_attr_value(&AttrValue::Str("width".into())),
            "width"
        );
    }

    #[test]
    fn test_stringify_list_and_tuple_values() {
        let list = AttrValue::List(vec![
            AttrValue::Int(1),
            AttrValue::Int(2),
            AttrValue::Int(3),
        ]);
        assert_eq!(stringify_attr_value(&list), "1,2,3");
        assert_eq!(
            stringify_attr_value(&AttrValue::Triple([1.0, 2.0, 3.0])),
            "1.0,2.0,3.0"
        );
    }

    fn dict(name: &str, entries: &[(&str, AttrValue)]) -> (Option<Rc<str>>, Option<Rc<Obj>>) {
        (
            Some(Rc::from("CAttributeNamed")),
            Some(Rc::new(Obj::Dict {
                name: name.into(),
                entries: entries
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            })),
        )
    }

    #[test]
    fn test_extracts_every_attribute_dictionary_by_name() {
        let attrs = Obj::Attrs {
            children: vec![
                dict("SU_DefinitionSet", &[("unrelated", AttrValue::Int(1))]),
                dict(
                    "dynamic_attributes",
                    &[
                        ("width", AttrValue::Float(10.0)),
                        ("_width_label", AttrValue::Str("Width".into())),
                        ("count", AttrValue::Int(4)),
                    ],
                ),
                dict(
                    "fbd-profile",
                    &[("guide", AttrValue::Triple([1.0, 2.0, 3.0]))],
                ),
            ],
        };
        let dicts = extract_legacy_attribute_dictionaries(Some(&attrs));
        let names: Vec<&str> = dicts.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            ["SU_DefinitionSet", "dynamic_attributes", "fbd-profile"]
        );
        assert_eq!(dicts["SU_DefinitionSet"]["unrelated"], AttrValue::Int(1));
        assert_eq!(dicts["dynamic_attributes"]["width"], AttrValue::Float(10.0));
        assert_eq!(
            dicts["dynamic_attributes"]["_width_label"],
            AttrValue::Str("Width".into())
        );
        assert_eq!(dicts["dynamic_attributes"]["count"], AttrValue::Int(4));
        assert_eq!(
            dicts["fbd-profile"]["guide"],
            AttrValue::Triple([1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn test_returns_empty_dict_for_no_children() {
        let attrs = Obj::Attrs { children: vec![] };
        assert!(extract_legacy_attribute_dictionaries(Some(&attrs)).is_empty());
    }

    #[test]
    fn test_returns_empty_dict_for_no_attribute_container() {
        assert!(extract_legacy_attribute_dictionaries(None).is_empty());
    }

    /// Python walks `build_scene()`'s tree; the parse carries the same
    /// per-instance properties, so this checks every parsed instance.
    #[test]
    fn test_real_legacy_fixture_wires_through_without_crashing() {
        let model = openskp::SkpFile::open(fixture("capilla_quiroz_v17.skp"))
            .unwrap()
            .parse()
            .unwrap();
        for d in model.definitions.values().chain([&model.root]) {
            assert!(d.instances.iter().all(|i| i.properties.is_empty()));
        }
    }
}

// ── TestVffAttributeDictionaries ──

mod test_vff_attribute_dictionaries {
    use crate::common::tlv;
    use openskp::tlv::{parse_tlv_recursive, CONTAINER_TAGS};
    use openskp::vff::{extract_attribute_dictionaries, extract_dynamic_properties};
    use openskp::{AttrDicts, AttrValue, IndexMap};

    fn value(inner: &[u8]) -> Vec<u8> {
        tlv("A438", inner)
    }

    fn entry(key: &str, value: Vec<u8>) -> Vec<u8> {
        [tlv("B636", key.as_bytes()), value].concat()
    }

    fn dict(name: &str, entries: &[u8]) -> Vec<u8> {
        [tlv("B436", name.as_bytes()), tlv("B536", entries)].concat()
    }

    fn f64s(vs: &[f64]) -> Vec<u8> {
        vs.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn dicts_of(dc05_payload: &[u8]) -> AttrDicts {
        let d007 = tlv("D007", &tlv("DC05", dc05_payload));
        let elements = parse_tlv_recursive(&d007, 0, d007.len(), &CONTAINER_TAGS).unwrap();
        extract_attribute_dictionaries(&elements[0]).unwrap()
    }

    fn one(name: &str, entries: &[(&str, AttrValue)]) -> AttrDicts {
        let mut d = IndexMap::new();
        d.insert(
            name.to_string(),
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        );
        d
    }

    #[test]
    fn test_string_value_round_trips() {
        let entries = entry("code", value(&tlv("AD38", b"Ks")));
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one("fbd-einfo", &[("code", AttrValue::Str("Ks".into()))])
        );
    }

    #[test]
    fn test_length_and_float_are_distinct_tags_both_f64() {
        let entries = [
            entry("depth", value(&tlv("AF38", &f64s(&[15.5])))),
            entry("price", value(&tlv("A938", &f64s(&[120.0])))),
        ]
        .concat();
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one(
                "fbd-einfo",
                &[
                    ("depth", AttrValue::Float(15.5)),
                    ("price", AttrValue::Float(120.0))
                ]
            )
        );
    }

    #[test]
    fn test_integer_value_round_trips() {
        let entries = entry("angle", value(&tlv("A738", &(-7i32).to_le_bytes())));
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one("fbd-einfo", &[("angle", AttrValue::Int(-7))])
        );
    }

    #[test]
    fn test_null_value_round_trips() {
        let entries = entry("child_thickness", value(b""));
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one("fbd-einfo", &[("child_thickness", AttrValue::Null)])
        );
    }

    #[test]
    fn test_point3d_and_vector3d_round_trip() {
        let entries = [
            entry(
                "end_pos",
                value(&tlv("B438", &f64s(&[0.0, 0.807085, 14.6551]))),
            ),
            entry("vector_new", value(&tlv("B538", &f64s(&[1.0, 0.0, 0.0])))),
        ]
        .concat();
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one(
                "fbd-einfo",
                &[
                    ("end_pos", AttrValue::Triple([0.0, 0.807085, 14.6551])),
                    ("vector_new", AttrValue::Triple([1.0, 0.0, 0.0]))
                ]
            )
        );
    }

    #[test]
    fn test_empty_array_round_trips() {
        let entries = entry("added_bolt_holes", value(&tlv("AE38", b"")));
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one(
                "fbd-einfo",
                &[("added_bolt_holes", AttrValue::List(vec![]))]
            )
        );
    }

    #[test]
    fn test_array_of_floats_round_trips() {
        let elems: Vec<u8> = [0.728, 11.358, 14.655]
            .iter()
            .flat_map(|&v| value(&tlv("A938", &f64s(&[v]))))
            .collect();
        let entries = entry("flangeholes", value(&tlv("AE38", &elems)));
        let floats = AttrValue::List(vec![
            AttrValue::Float(0.728),
            AttrValue::Float(11.358),
            AttrValue::Float(14.655),
        ]);
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one("fbd-einfo", &[("flangeholes", floats)])
        );
    }

    #[test]
    fn test_nested_array_round_trips() {
        let arr = |vs: &[f64]| -> Vec<u8> {
            vs.iter()
                .flat_map(|&v| value(&tlv("A938", &f64s(&[v]))))
                .collect()
        };
        let outer = [
            value(&tlv("AE38", &arr(&[25.17, 0.07]))),
            value(&tlv("AE38", &arr(&[25.17, 15.35]))),
        ]
        .concat();
        let entries = entry("lip_side1_cords", value(&tlv("AE38", &outer)));
        let pair = |a: f64, b: f64| AttrValue::List(vec![AttrValue::Float(a), AttrValue::Float(b)]);
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one(
                "fbd-einfo",
                &[(
                    "lip_side1_cords",
                    AttrValue::List(vec![pair(25.17, 0.07), pair(25.17, 15.35)])
                )]
            )
        );
    }

    #[test]
    fn test_multiple_dictionaries_kept_separate_by_name() {
        let d1 = dict("fbd-einfo", &entry("code", value(&tlv("AD38", b"Ks"))));
        let d2 = dict(
            "fbd-profile",
            &entry("guide", value(&tlv("A738", &1i32.to_le_bytes()))),
        );
        let mut want = one("fbd-einfo", &[("code", AttrValue::Str("Ks".into()))]);
        want.extend(one("fbd-profile", &[("guide", AttrValue::Int(1))]));
        assert_eq!(dicts_of(&[d1, d2].concat()), want);
    }

    #[test]
    fn test_unrecognized_value_tag_is_skipped_not_guessed() {
        let entries = entry("mystery", value(&tlv("EE99", b"\x01\x02")));
        assert_eq!(
            dicts_of(&dict("fbd-einfo", &entries)),
            one("fbd-einfo", &[("mystery", AttrValue::Null)])
        );
    }

    #[test]
    fn test_backward_compatible_properties_view() {
        let d1 = dict(
            "dynamic_attributes",
            &entry("width", value(&tlv("AF38", &f64s(&[10.0])))),
        );
        let d2 = dict(
            "fbd-profile",
            &entry("guide", value(&tlv("A738", &1i32.to_le_bytes()))),
        );
        let d007 = tlv("D007", &tlv("DC05", &[d1, d2].concat()));
        let elements = parse_tlv_recursive(&d007, 0, d007.len(), &CONTAINER_TAGS).unwrap();
        let props = extract_dynamic_properties(&elements[0]).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props["width"], "10.0");
    }

    #[test]
    fn test_no_dc05_returns_empty() {
        let d007 = tlv("D007", b"");
        let elements = parse_tlv_recursive(&d007, 0, d007.len(), &CONTAINER_TAGS).unwrap();
        assert!(extract_attribute_dictionaries(&elements[0])
            .unwrap()
            .is_empty());
        assert!(extract_dynamic_properties(&elements[0]).unwrap().is_empty());
    }
}

// ── TestObservability (the error half; Python's logging is left out) ──

mod test_observability {
    use crate::common::temp_dir;
    use openskp::tlv::Tag;
    use openskp::{Error, SkpParseError, Stage};

    #[test]
    fn test_bad_header_raises_parse_error_with_stage() {
        let path = temp_dir("badheader").join("not_a_skp.skp");
        std::fs::write(&path, b"not a real skp file".repeat(10)).unwrap();
        match openskp::SkpFile::open(&path).unwrap().parse() {
            Err(Error::Parse(e)) => assert_eq!(e.stage, Some(Stage::Header)),
            other => panic!("expected a header error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_error_str_includes_context() {
        let err = SkpParseError::new("boom", Stage::TlvWalk)
            .at_record(3, 10)
            .with_tag(Tag::new(0xF6, 0x01));
        let text = err.to_string();
        assert!(text.contains("stage=tlv_walk"));
        assert!(text.contains("record=3/10"));
        assert!(text.contains("tag=F601"));
    }

    #[test]
    fn test_parse_error_preserves_cause() {
        use std::error::Error as _;
        let original = std::io::Error::other("inner failure");
        let wrapped = SkpParseError::new("wrapped", Stage::TlvWalk).caused_by(original);
        let cause = wrapped.source().unwrap();
        assert_eq!(cause.to_string(), "inner failure");
        assert!(cause.downcast_ref::<std::io::Error>().is_some());
    }
}

// ── TestLegacyClassRef ──

mod test_legacy_class_ref {
    use openskp::legacy::is_class_ref;

    #[test]
    fn test_short_form() {
        let data = (0x8000u16 | 278).to_le_bytes();
        assert!(is_class_ref(&data, 0, 278));
        assert!(!is_class_ref(&data, 0, 279));
    }

    #[test]
    fn test_big_tag_escape() {
        // Slot 65712 does not fit in 0x8000|slot: MFC writes 0x7FFF + u32.
        let data = [
            0x7FFFu16.to_le_bytes().to_vec(),
            (0x8000_0000u32 | 65712).to_le_bytes().to_vec(),
        ]
        .concat();
        assert!(is_class_ref(&data, 0, 65712));
        assert!(!is_class_ref(&data, 0, 65713));
    }

    #[test]
    fn test_big_slot_never_matches_short_form() {
        let data = ((0x8000u32 | 65712) as u16).to_le_bytes();
        assert!(!is_class_ref(&data, 0, 65712));
    }

    #[test]
    fn test_truncated_data() {
        assert!(!is_class_ref(b"\xff\x7f\xb0", 0, 65712));
    }
}
