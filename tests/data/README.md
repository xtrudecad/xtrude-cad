# IngeTrazo test documents

Documents written by IngeTrazo, inputs of the `.igz` import tests
(`crates/xtrude-io/tests/it/test_upstream_documents_open.rs` and others).
They are frozen: never regenerate or edit them. The `dxf/` and `step/`
folders hold the inputs of the other importers.

| File | `igz_format` | `app_version` | What it holds |
|---|---|---|---|
| `banca-pergola-yanque.igz` | 2 | 0.3.12 | an example model: nine groups, saved views, dimensions, section planes, sheets |
| `upstream_removed_features.igz` | 2 | 0.5.7 | a small part, with the blocks of features Xtrude CAD does not have (georeferencing, base map, photo survey, sheets, shadows…) |
| `ingetrazo-0.5.8-new-keys.igz` | 1 | 0.5.8 | the keys 0.5.8 added, each set: a box; the group `Locked base` (locked, on the layer `Parts`, one face painted (0.8, 0.2, 0.1)); the component `Bracket` placed once; two definitions kept with no copy of their own (`Bracket`, sharing the placed one's prototype, and `Spare peg`); units in mm, precision 1, with area `cm2` (precision 3), volume `cm3`, angle `rad` (precision 2) and the decimal comma; the section plane `Cut A` with its own hatch fill |
| `ingetrazo-0.5.8-format3.igz` | 3 | 0.5.8 | a large document (20,445 faces + edges), every mesh a binary block: the loose mesh, a 60 × 60 grid of 1 mm squares (the first 60 painted (0.2, 0.6, 0.3), the next one (0.9, 0.9, 0.1) on the layer `Guides`) and a 20 mm square at z = 10 mm with two holes; the group `Plate`, a 50 × 50 grid of 2 mm squares from x = 100 mm, painted (0.3, 0.3, 0.8), among whose first 200 edges every second is soft, every third hidden, the first 50 are on curve 41 and the 100th to 119th on `Guides`; the component `Peg`, a 25 × 25 grid of 0.4 mm squares with its first face red, placed twice (`Peg 0`, `Peg 20`, 20 mm apart, at y = −20 mm) |

The two 0.5.8 documents were written by IngeTrazo 0.5.8 itself, its own
scene and document writer driven by a script; lengths in them are in
metres.
