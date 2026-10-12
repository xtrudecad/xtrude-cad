# STEP test files

Inputs of the STEP import tests (`crates/xtrude-io/tests/it/test_step_import.rs`).
They are frozen: never regenerate or edit them.

| File | What it holds |
|---|---|
| `cube.step` | a 10 mm cube |
| `plate_hole.step` | a 40 × 30 × 5 mm plate with a Ø10 through-hole at (20, 15) |
| `cylinder_fillet.step` | a Ø20 × 30 mm cylinder with a 2 mm fillet on its top edge (a torus) |
| `bspline_loft.step` | a loft through three periodic B-spline sections (B-spline surfaces) |
| `assembly.step` | `Assembly`: the part `Bracket` placed twice (`BracketLeft`, and `BracketRight` turned 90° about Z and moved 50 mm along X) and `Base` |
| `assembly_colours.step` | the same, with the bracket red (0.8, 0.1, 0.1), its hole green (0.1, 0.7, 0.2) and the base blue (0.1, 0.2, 0.8) |
| `plate_inch.step` | a 2 × 1.5 × 0.25 inch plate with a Ø0.5 inch hole: the file's length unit is a conversion-based `INCH` |
| `nist_ftc_11_asme1_rb.stp` | NIST FTC-11, AP203 geometry only (millimetres) |
| `nist_ftc_09_asme1_rd.stp` | NIST FTC-09, AP203 geometry only, in inches |

The first seven were written by FreeCAD 1.1 (OpenCascade 7.9) with
`generate.py`, which records how; their licence is the project's.

The `nist_*` files come from the NIST MBE PMI Validation and Conformance
Testing Project, <https://www.nist.gov/document/nist-pmi-step-files>,
exported there by commercial CAD systems. They are works of the US
government, in the public domain (`NIST-PD`, annotated in `REUSE.toml`).
