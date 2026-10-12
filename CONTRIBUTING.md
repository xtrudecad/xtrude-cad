# Contributing to Xtrude CAD

Xtrude CAD is push/pull 3D modelling for 3D-printable parts, in Rust.
This page has what you need before changing code: the workspace, the
ground rules, how to build and test, and how to commit. Every change must
pass `scripts/check.sh`.

Before a large change, open an
[issue](https://github.com/xtrudecad/xtrude-cad/issues) or a
[discussion](https://github.com/xtrudecad/xtrude-cad/discussions) so we
can agree on the approach first. Report security problems privately, as
[SECURITY.md](SECURITY.md) says. Everyone taking part follows the
[code of conduct](CODE_OF_CONDUCT.md).

## The workspace

| Crate | What |
|---|---|
| `xtrude-core` | geometry kernel (`f64`/`glam`): meshes with holed faces, triangulation, offset, fillet, solids, snapping, camera, 3D-text outlines. No GUI or GPU. |
| `xtrude-model` | the document: scene, groups and components, commands, snapshot undo/redo, push/pull, selection, layers, units, settings |
| `xtrude-io` | `.igz` documents, STL, 3MF export, OBJ, DAE and GLB, DXF, SKP and STEP import |
| `xtrude-render` | the wgpu viewport renderer and its WGSL shaders |
| `xtrude-ext` | sandboxed Lua extensions and their API ([docs/extensions.md](docs/extensions.md)) |
| `xtrude-app` | the application, binary `xtrude` |
| `openskp` | the `.skp` reader (MIT) |
| `xtrude-step` | the STEP helper program, on OpenCascade (opt-in `occt` feature) |

`xtrude-app` keeps almost everything testable without a window:
`viewport/` (input, picking, snapping, frame building), `tools/` (one
module per tool), `window/` (actions, menus, files, dialogs through a
`Dialogs` trait) and `tray/` (side panels) hold the logic, and `shell/`
is the only egui code: it draws and forwards input. Keep that split.

## Ground rules

- **Behaviour is deliberate.** Tools, snapping, tolerances and shortcuts
  are tuned and pinned by tests. Change behaviour on purpose, with a test
  that shows the new behaviour.
- **`.igz` stays compatible with IngeTrazo** in both directions.
- **Every change comes with tests.** Never drop or weaken a test to make
  a change pass.
- **Licence:** GPL-3.0-or-later. Every source file starts with
  ```rust
  // SPDX-License-Identifier: GPL-3.0-or-later
  // SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
  ```
  (`#` for scripts and TOML, `--` for Lua). `crates/openskp` stays MIT.
  Add only GPL-3.0-compatible dependencies; `deny.toml` checks them.
- **Clean Rust:** `Result` errors, no `unwrap()` on user input or file
  data, `cargo fmt` and `cargo clippy --workspace --all-targets -- -D
  warnings` clean. A local `#[allow(…)]` says why in a comment.
- **Generic names:** name tools and panels with generic CAD terms.
- **English only,** but wrap every user-visible string in `tr("…")`.

## Build and test

```bash
cargo build                                  # debug build
cargo test --workspace                       # all tests; several minutes
cargo test -p xtrude-app --test it test_x::  # one test file
scripts/check.sh                             # what CI runs
```

STEP import runs OpenCascade in a separate helper program, `xtrude-step`.
OpenCascade builds from source (CMake and a C++17 compiler, about 17
minutes the first time), so a plain `cargo build` makes a stub helper
instead. To build the real one, and run the STEP tests that need it:

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release -p xtrude-step --features occt
cargo test -p xtrude-io --test it test_step
```

OpenCascade lands in `target/OCCT`. A build in another target folder
(after a `cargo clean`, or in a worktree) can reuse an earlier one: copy
its `lib` and `include` folders somewhere and point `XTRUDE_OCCT_DIR` at
that folder, and the build links them instead of compiling OpenCascade
again.

Each crate's integration tests are one binary, `tests/it/main.rs`: a new
test file goes in `tests/it/` with a `mod` line in `main.rs`. Tests must
not write real settings: call `xtrude_model::settings::use_memory_store()`
first (the shared test helpers do).

`tests/golden/` holds reference models built from
`tests/golden/recipes.json`; a changed result fails the tests. When a
change is deliberate, regenerate them with
`XTRUDE_UPDATE_GOLDEN=1 cargo test -p xtrude-io --test it test_golden`,
check that the diff touches only the models it should, and say why in
the commit. The other test data (`tests/data/`, the OpenSKP fixtures) is
frozen.

To try the app without touching your desktop settings, run it with
`XDG_CONFIG_HOME` pointing to a scratch folder (VS Code's tasks do, and
can start it on a headless Xvfb display).

## Commits

Make small commits that compile with tests green. The subject is
`<area>: <what>`, e.g. `app: reset toolbar layout action`.

Every commit carries a [Developer Certificate of Origin](https://developercertificate.org)
sign-off from a human (`git commit -s`). The repository ships a commit
message template with the trailers; enable it once per clone:

```bash
git config commit.template .gitmessage
```

## AI-assisted contributions

- AI-assisted contributions are welcome. The human contributor must
  understand, review and test every AI-assisted change, and is responsible
  for it exactly as for code they wrote by hand.
- Disclose AI involvement with a `Co-authored-by:` trailer that names the
  tool, for example `Co-Authored-By: Claude <noreply@anthropic.com>`.
- Every commit needs a human DCO sign-off (`git commit -s`). An AI tool
  cannot certify the DCO and must never appear in a `Signed-off-by:` line.
- Do not submit code you do not have the right to contribute under
  GPL-3.0-or-later, whether it was AI-generated or not.

Copyright stays with the human authors: AI tools are not added to
[AUTHORS](AUTHORS), the SPDX copyright lines or the `Cargo.toml` authors.

## VS Code

The repository ships a VS Code workspace setup in `.vscode/`. Install the
recommended extensions when VS Code offers them (rust-analyzer and CodeLLDB
are the essential ones). Then:

- **Ctrl+Shift+B** builds the workspace. **Tasks: Run Task** lists the
  rest: run the app (optionally with sandboxed settings or on a headless
  Xvfb display), test the current file, run the CI checks and benchmarks.
- **F5** debugs the app, the tests of the current file, or the current
  crate's unit tests. The **Debug** lens above each `#[test]` debugs a
  single test.
- rust-analyzer runs clippy on save and builds into `target/rust-analyzer`,
  so it does not block `cargo` in the terminal.
- Typing `spdx` (or `spdx-mit` in `crates/openskp`) inserts the licence
  header, and `testfile` starts an integration test file (in `tests/it/`,
  with a `mod` line in `tests/it/main.rs`).
