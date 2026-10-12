# openskp — relationship to OpenSKP

A Rust SketchUp reader derived from the **read path** of [OpenSKP](https://github.com/iamahsanmehmood/openskp)
(MIT, © 2024–2026 Ahsan Mehmood), from `packages/python/openskp` at
commit `291700bc213655a00461838de71e5f206311e619`, the version Xtrude
CAD's original Python code used. Xtrude uses exactly one entry point,
`SkpFile.open(path).parse()`, and this crate implements what that call
reaches, plus the one fix the Python code applied to it at run time
(see Deviations).

The crate keeps OpenSKP's MIT licence (see `LICENSE`) and has no
`xtrude-*` dependencies, so it can move to its own repository or to
upstream's `packages/` later (`git subtree split -P crates/openskp`).

## Module map

`SkpFile.parse()` calls `_core.full_parse`, which routes classic files to
`legacy.full_parse_legacy`, and turns the resulting dict into the model
(`model.py`). Nothing else in the package is reached.

| Python (`openskp/`) | Rust | State |
|---|---|---|
| `errors.py` | `errors` | done |
| `_core.py` TLV parsing (`read_u32`, `read_f64`, `parse_var_int`, `parse_tlv_recursive`, `_flat_headers`, `_unwrap_definitions_container`, `iter_top_level_lazy`, `CONTAINER_TAGS`) | `tlv` | done |
| `_core.py full_parse` steps 1–2, `_validate_zip_entry_size`, `_read_meta_units`; `vff.py validate_header` | `container` | done |
| `_core.py full_parse` (the VFF walk: `collect_layers`, `collect_material_ids`, `collect_defs`), `_extract_geometry_from_nodes`, `extract_entity_id`, `_extract_uv_transforms`, `extract_attribute_dictionaries`, `extract_dynamic_properties`, `_scan_vertex_positions`, `_scan_instance_transforms`, `_parse_dimensions`, `_parse_pages`, `transform_point`, `multiply_matrices` | `vff` | done |
| `_core.py full_parse` material/style XML, `_extract_texture` | `xml` | done |
| the `full_parse` result dict, `_GeometryBuilder`, `legacy._Builder` | `parsed` | done |
| `legacy.py` (`is_legacy`, `_R`, `_Archive`, every record reader, `_walk`, `_scan_pages`, `_fill_builder`, `full_parse_legacy`) | `legacy` | done |
| `model.py` (`SkpFile.open`, `SkpFile.parse`, `SkpModel` and the data classes, `Texture.save`) | `lib`, `model` | done |
| `parser.py` | — | an unused older copy of the `_core` TLV functions; its tests run against `tlv` |
| `materials.py`, `metadata.py`, `transforms.py`, `triangulator.py`, `_curves.py` (and `model.loose_edge_runs`, which uses it) | — | left out: `parse()` never calls them (standalone helpers; triangulation belongs to scene building) |
| `create.py`, `edit.py`, `codegen.py`, `export/*`, `scene.py`, `instanced_scene.py`, `_face_groups.py`, `_fragments_fb/`, `_core.triangulate_face_3d`, `vff.extract_skp_contents` | — | not needed: writer, editor, exporters, scene building, dead code |

Python's built-in behaviour that the output depends on (`str(float)`,
`int()`, `float()`, `bytes.decode` with an error handler, `min`/`max`
with NaN) is in `py`.

## Deviations

- **Classic dimensions anchored inside groups** (iamahsanmehmood/openskp#384).
  The pinned `_read_dimlinear` skips a fixed 42/82 bytes after each
  connection reference, which is right only while the instance-path lists
  that follow are empty; on a dimension anchored inside nested groups the
  read slides off the record and the root list stops early (a 2018 house
  opened with 2 of its 72 root entities). The Python code patched the
  reader at run time (`formats/openskp_compat.py`); here
  `legacy::read_dimlinear` reads the paths (`connection_paths`,
  `path_ref`) as its own behaviour, so the golden dumps are made with the
  patch applied. Provenance: the fix comes from the Python code (GPL, by
  the IngeTrazo contributors); it reads a byte layout upstream tracks in
  #384, and is kept here under the crate's MIT licence so the crate stays
  usable on its own.
- **`SkpModel.layer_names`**, an addition: `full_parse`'s
  `layer_id_to_name`, which Python keeps internal, so a consumer can name
  the raw layer ids of `Face.layer` and `Edge.layer`. The golden dumps do
  not cover it (Python has no such field).
- **Errors.** `SkpFile::open` returns `Error::NotFound` / `Error::NotSkp`
  where Python raises `FileNotFoundError` / `ValueError`. Every other
  failure is an `SkpParseError`, also where Python lets a bare exception
  escape `parse()` (`KeyError` for a missing `model.dat`, `TypeError` on a
  scene record without its name or camera record, a record of the wrong
  kind behind a classic reference). Where Python fails, this fails; only
  the error type differs.
- **Exception classes in the classic walker.** `legacy::LegacyError` keeps
  Python's classes apart because the walk treats them differently: an
  entity list recovers only from a `LegacyParseError`, the slot-base retry
  also from `struct.error`, `IndexError` and `UnicodeDecodeError`, and the
  scene scan from the first two.
- **Nesting depth.** Python stopped a hostile, deeply nested file with
  `RecursionError` at about 1000 frames. `parse_tlv_recursive` stops at
  `MAX_TLV_DEPTH` (960) with an `SkpParseError` (`stage=tlv_walk`); the
  classic walker stops at 250 nested records (`legacy::MAX_DEPTH`, about
  where Python's frames ran out), without trying another slot base, as a
  `RecursionError` did not.
- **Out-of-range reads.** The `tlv` primitives (`read_u32`, `read_f64`,
  `parse_var_int`) return `None` where Python raises `struct.error`. Inside
  the walks a read past the end fails the parse, as Python's exception
  did. A TLV range past the end of the buffer is clamped.
- **Integers.** Python integers are unbounded; entity ids here are `i64`.
  A var-int longer than 8 bytes keeps its low 64 bits (exact for the zero
  padding longer fields carry; `tlv::parse_var_int`, the primitive, still
  refuses it). An XML colour value outside `i64` is a `ValueError` here
  (the material is skipped) where Python would accept it.
- **XML.** Python parses with `defusedxml`, which refuses any entity
  declaration; here a document that declares one is refused before
  `roxmltree` sees it. Encodings: UTF-8 (with or without BOM), UTF-16 with
  a BOM, ISO-8859-1 and ASCII; a document declaring another encoding is
  skipped, where expat would have decoded it.
- **A truncated classic layer record.** `legacy._read_layer` collects its
  flag bytes until the next string marker; at the end of the data
  `raw(1)` returns nothing and Python loops forever. The reader stops at
  the end of the data and fails on the missing string instead (an upstream
  bug).
- **Payloads.** `TlvNode::payload` borrows from the buffer, so the lazy
  slicing of `_TlvNode` (openskp#244) is not needed.
- **ZIP offset.** Python searches the first 512 bytes for `PK\x03\x04`,
  then the first 4096. One search of 4096 bytes gives the same first match.
- **Model shapes.** Python dicts become `IndexMap`s (insertion order is
  part of the result). `SkpModel.definitions` is keyed by
  `Option<i64>`: a VFF definition without an entity id lands under
  Python's `None` key. `materials_by_id` holds indices into `materials`
  (Python shares the `Material` objects); `SkpModel::material(id)`
  resolves one. `Instance.guid` and `Definition.guid` are `Option`s
  because the VFF reader leaves them `None` when the record has none.
- **Logging.** Python logs progress to the `openskp` logger; the crate
  does not log.

### Upstream bugs found

- `legacy._read_layer` spins forever on a file truncated inside a layer
  record (above).
- `_core._parse_pages` raises `TypeError` (`len(None)`) on a scene record
  that lacks its `6F54` name or `714A` camera record, which fails the
  whole parse instead of dropping the scene. Kept here as an error.
- The pinned `_read_dimlinear` (#384, above), reported upstream by the
  Python code's authors.

## Tests

Upstream tests live in `packages/python/tests/` and their fixtures in
`packages/python/tests/fixtures/`. A Rust test file says which upstream
file it derives from in its doc comment (`Derived from OpenSKP
packages/python/tests/…`).

| Upstream | Rust | Notes |
|---|---|---|
| `test_parser.py` `TestReadU32`, `TestReadF64`, `TestParseVarInt`, `TestParseTlvRecursive`, `TestCoreTlvBoundary`, `TestVff`, `TestZipEntrySizeCap`, `TestSkpFile` | `tests/it/test_parser.rs` | included |
| `test_parser.py` `TestTlvNodePerformance` | `tests/it/test_parser.rs` | Included: the contract tests, and the lazy-payload test as a zero-copy check. `test_node_supports_dict_style_get_with_default` is not included because it tests Python `dict`-style access, which a Rust struct does not have. |
| `test_parser.py` `TestDataModel`, `TestUseTrans`, `TestUtf8EntityNames`, `TestMaterialIdJoin`, `TestLayerHidden`, `TestFaceInstanceHidden`, `TestMetaUnits`, `TestInstanceMaterial`, `TestStyles`, `TestFaceUvTransform`, `TestBackMaterial`, `TestFaceCameraBehavior`, `TestSectionPlaneTextDimension`, `TestImageEntities`, `TestTextureExtraction`, `TestLegacyDetection`, `TestLegacyStrings`, `TestLegacyRealFile`, `TestLegacyDynamicProperties`, `TestVffAttributeDictionaries`, `TestLegacyClassRef` | `tests/it/test_parser.rs` (one module per class) | Included. The tests that stub `_core.full_parse` build a `Parsed` by hand and call `SkpModel::from_parsed`. `TestDataModel.test_instance_has_no_children_field` names every field of `Instance` in a struct literal. `TestLegacyDynamicProperties.test_real_legacy_fixture_wires_through_without_crashing` checks the parsed instances (Python walked `build_scene()`'s tree). |
| `test_parser.py` `TestJsonExport.test_real_file_matches_ground_truth` | `tests/it/test_parser.rs` | included as the same counts on the parsed model; the JSON exporter and the other `TestJsonExport` tests are not included (exporter) |
| `test_parser.py` `TestVffLayerHidden` | `tests/it/test_parser.rs` | `test_hidden_and_visible_layers_read_correctly` included; `test_build_scene_threads_the_real_value_through` not (scene building) |
| `test_parser.py` `TestModernRealFile` | `tests/it/test_parser.rs` | `test_untitled_skp_matches_ground_truth`, `test_coedge_orientations_are_normalized_and_connected` included; `test_su_file_skp_matches_ground_truth` included without its `build_scene()` half; `test_untitled_skp_attribute_dictionaries_reach_glb_and_json_export` not (exporters) |
| `test_parser.py` `TestXmlEntityExpansion` | `tests/it/test_parser.rs` | `test_full_parse_skips_malicious_material_and_style_xml` included; `test_parse_material_xml_rejects_entity_declarations` not (`materials.py` is not reached by `parse()`; the full-parse test covers the same refusal) |
| `test_parser.py` `TestObservability` | `tests/it/test_parser.rs` | the three error tests included; `test_silent_by_default`, `test_progress_logs_at_debug`, `test_scene_build_logs` not (Python `logging`; the crate does not log) |
| `test_parser.py` `TestTransforms`, `TestTriangulator`, `TestMaterials` | — | not included: `transforms.py`, `triangulator.py`, `materials.py` are not reached by `parse()` |
| `test_parser.py` `TestTriangulateFace3dRobustness`, `TestIsConvex2d`, `TestTriangulateFace3dConvexFastPath`, `TestTriangulateFace3dMatchesTrueArea`, `TestBuildScene*`, `TestGlbExport`, `TestObjExporter`, `TestStlExporter`, `TestPlyExporter`, `TestDxfExporter` | — | not included: scene building and exporters |
| `test_legacy_single_material.py` | `tests/it/test_legacy_single_material.rs` | included |
| `test_legacy_v20_filler.py` | `tests/it/test_legacy_v20_filler.rs` | included |
| `test_legacy_v20.py` | — | not included: its fixture, `gondola_v20.skp`, is not committed (Fixtures, below) |
| `test_legacy_pre2014_instance_guid.py` | `tests/it/test_legacy_pre2014_instance_guid.rs` | included |
| `test_pages_dimensions.py` | `tests/it/test_pages_dimensions.rs` | included |
| `test_edge_surfacing.py` | `tests/it/test_edge_surfacing.rs` | included, except the four tests on `gondola_v20.skp` (classic edge layers, face layers on two layers, independent curve tables, elliptical arc frames; Fixtures, below) |
| `test_loose_edge_runs_public_api.py` | `tests/it/test_loose_edge_runs_public_api.rs` | the two tests on the parsed model included, on `capilla_quiroz_v17.skp` instead of `gondola_v20.skp`, so without the `Edge::layer` check; `test_matches_build_instanced_scenes_curve_resources` not (`loose_edge_runs` and the instanced scene are not included) |
| `test_loose_edge_curve_sets.py`, `test_overlapping_face_holes.py`, `test_instanced_scene.py` | — | not included: curve sets, triangulation and scene building |
| `test_codegen`, `test_create`, `test_edit`, `test_fragments`, `test_ifc*` | — | not included: writer/exporter features |

Also here:

- `tests/it/test_golden.rs`: golden parses (below).
- `tests/it/test_container.rs`: the container layer on synthetic files.
- `tests/it/test_robustness.rs`: every fixture truncated, with bytes flipped
  and spans zeroed (and VFF `model.dat` mutated inside a valid ZIP) gives
  an error, never a panic.
- `tests/it/test_openskp_compat.rs`: the Xtrude test of the #384 fix.

## Golden parses

`tests/fixtures/` holds the upstream fixtures (`SU_File.skp`,
`Untitled.skp`, `blank_v17.skp`, `capilla_quiroz_v17.skp`,
`legacy_v2014_synthetic.skp`, `legacy_v7_synthetic.skp`,
`single_material_v17.skp`; MIT, annotated in the workspace `REUSE.toml`;
see Fixtures, below)
and, next to each, `<name>.expected.json`: the Python OpenSKP parse
(with the #384 fix applied), dumped as canonical JSON. The dump
covers every field of `SkpModel`, mappings as `[key, value]` pairs in
order, attribute values with their Python type, texture bytes as length
and FNV-1a hash. `tests/it/test_golden.rs` dumps the Rust parse the same way
(`tests/it/common/dump.rs`) and reports the first differing path.

All seven fixtures are identical (floats within 1e-9). The expected
dumps are frozen references: do not regenerate them.

To dump the Rust parse of any file, or compare it with a dump in the same
format:

```bash
cargo run -p openskp --example openskp_dump -- FILE.skp [FILE.expected.json]
```

### Fixtures

The fixtures carry only content the project may redistribute, so two
upstream ones were changed:

- `gondola_v20.skp` is not committed. Besides its own model it holds
  the application's sample scale figure (geometry, materials,
  description and preview image), an IFC classification schema with its
  logo, and a user's local path. The tests that only it backs are left
  out (table above): classic edge layers, elliptical arc frames and the
  SketchUp 2020 file as a whole (the v20 filler probe keeps its
  synthetic tests, `test_legacy_v20_filler.rs`).
- `capilla_quiroz_v17.skp` embedded four textures from the application's
  material library. Each JPEG was replaced in place by a plain image of
  its average colour, at the same pixel size and padded with zeros to
  the same byte length, so every offset in the file is unchanged. In
  `capilla_quiroz_v17.expected.json` only those four textures' `fnv`
  hashes changed; the rest is the Python parse.

## Performance

`cargo bench -p openskp --bench parse` (release, criterion) against
Python OpenSKP's `SkpFile.parse()` on the same machine (median of 15
runs after a warm-up):

| Fixture | Rust | Python |
|---|---|---|
| `legacy_v2014_synthetic.skp` (1.6 MB, the largest) | 0.51 ms | 1.8 ms |
| `Untitled.skp` (VFF, 1588 faces) | 19.0 ms | 542 ms |

The bench also times `capilla_quiroz_v17.skp`, the classic fixture with
the most geometry since `gondola_v20.skp` left (Fixtures, above); it has
no Python figure here.

The VFF walk keeps Python's memory bound: `model.dat` is read whole, but
only one top-level record's (or one definition's) TLV subtree is alive at
a time.
