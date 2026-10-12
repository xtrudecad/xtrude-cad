// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Extensions menu: extension commands as actions, Run Lua Script…,
//! Reload, and the extensions that failed to load. The folder is a
//! temporary one, never the real configuration.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use xtrude_app::window::extensions::{self, action_id};
use xtrude_app::window::{MainWindow, MenuItem, ScriptedDialogs};
use xtrude_ext::ParamValue;
use xtrude_model::settings::{self, keys, JsonText};
use xtrude_model::units::Units;

use crate::common::{self, TmpDir};

/// A manifest for the folder `id`: each command draws a triangle of side
/// `size` mm.
fn manifest(id: &str, name: &str, commands: &[(&str, &str, f64)]) -> String {
    let cmds: Vec<String> = commands
        .iter()
        .map(|(c, label, size)| {
            format!(
                "{{ id = '{c}', label = '{label}', run = function(doc) \
                 doc:add_face({{ xtrude.vec(0, 0), xtrude.vec({size}, 0), xtrude.vec(0, {size}) }}) \
                 end }}"
            )
        })
        .collect();
    format!(
        "return {{ id = '{id}', name = '{name}', version = '1.0', api = 1, \
         commands = {{ {} }} }}",
        cmds.join(", ")
    )
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("temp subdir");
    }
    std::fs::write(path, text).expect("temp file");
}

/// A window with the extensions of `dir`, its failures logged in `dir`,
/// and scripted dialogs.
fn window_with(dir: &TmpDir) -> (MainWindow, Rc<RefCell<ScriptedDialogs>>) {
    let mut w = common::window();
    w.viewport.history.error_log = Some(dir.join("errors.log"));
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    w.load_extensions(Some(dir.join("extensions")));
    (w, dialogs)
}

fn ext_menu(w: &MainWindow) -> Vec<MenuItem> {
    w.menus
        .iter()
        .find(|(t, _)| *t == extensions::menu_title())
        .map(|(_, items)| items.clone())
        .expect("an Extensions menu")
}

fn act(id: &str) -> MenuItem {
    MenuItem::Action(id.to_string())
}

fn fixed() -> Vec<MenuItem> {
    vec![
        act(extensions::RUN_SCRIPT),
        act(extensions::RELOAD),
        act(extensions::OPEN_FOLDER),
    ]
}

#[test]
fn without_a_folder_the_menu_has_the_fixed_entries() {
    let w = common::window();
    assert!(w.extensions.dir.is_none());
    assert_eq!(ext_menu(&w), fixed());
    let titles: Vec<&str> = w.menus.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        titles,
        [
            "File",
            "Edit",
            "Camera",
            "Draw",
            "Tools",
            "Extensions",
            "Window",
            "Help"
        ]
    );
}

#[test]
fn extensions_give_actions_and_menu_entries_in_folder_order() {
    let dir = TmpDir::new();
    let exts = dir.join("extensions");
    write(
        &exts,
        "zeta/init.lua",
        &manifest("zeta", "Zeta", &[("one", "Only one", 10.0)]),
    );
    write(
        &exts,
        "alpha/init.lua",
        &manifest(
            "alpha",
            "Alpha tools",
            &[("a", "First…", 10.0), ("b", "Second", 20.0)],
        ),
    );
    let (w, _) = window_with(&dir);
    for (id, text) in [
        ("ext.alpha.a", "First…"),
        ("ext.alpha.b", "Second"),
        ("ext.zeta.one", "Only one"),
    ] {
        let a = w.actions.get(id).expect("an action per command");
        assert_eq!(a.text, text);
        assert!(a.shortcuts.is_empty());
    }
    let mut expected = vec![
        MenuItem::Menu(
            "Alpha tools".into(),
            vec![act("ext.alpha.a"), act("ext.alpha.b")],
        ),
        act("ext.zeta.one"),
        MenuItem::Separator,
    ];
    expected.extend(fixed());
    assert_eq!(ext_menu(&w), expected);
}

#[test]
fn an_extension_command_is_one_undo_step_named_after_it() {
    let dir = TmpDir::new();
    write(
        &dir.join("extensions"),
        "tri/init.lua",
        &manifest("tri", "Triangles", &[("big", "Big triangle…", 40.0)]),
    );
    let (mut w, dialogs) = window_with(&dir);
    assert!(w.trigger(&action_id("tri", "big")));
    assert_eq!(w.viewport.history.undo_len(), 1);
    assert_eq!(w.viewport.history.undo_name(), Some("Big triangle"));
    let (lo, hi) = w.viewport.scene.compute_bounds().expect("a face");
    assert!(((hi - lo).x - 0.04).abs() < 1e-9, "{lo} {hi}");
    assert!(dialogs.borrow().asked.is_empty());
}

#[test]
fn a_failing_command_warns_and_leaves_no_step() {
    let dir = TmpDir::new();
    write(
        &dir.join("extensions"),
        "bad/init.lua",
        "return { id = 'bad', name = 'Bad', api = 1, commands = { \
         { id = 'boom', label = 'Boom', run = function(doc) \
           doc:add_face({ xtrude.vec(0, 0), xtrude.vec(1, 0), xtrude.vec(0, 1) }) \
           error('it broke') end } } }",
    );
    let (mut w, dialogs) = window_with(&dir);
    assert!(w.trigger("ext.bad.boom"));
    assert_eq!(w.viewport.history.undo_len(), 0);
    assert!(w.viewport.scene.compute_bounds().is_none());
    let asked = &dialogs.borrow().asked;
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, "report");
    assert!(asked[0].1.starts_with("Extension Bad: "), "{asked:?}");
    assert!(asked[0].1.contains("bad/init.lua:1: it broke"), "{asked:?}");
}

#[test]
fn broken_extensions_are_skipped_and_listed() {
    let dir = TmpDir::new();
    let exts = dir.join("extensions");
    write(&exts, "broken/init.lua", "return {");
    write(
        &exts,
        "good/init.lua",
        &manifest("good", "Good", &[("c", "C", 5.0)]),
    );
    write(
        &exts,
        "other/init.lua",
        &manifest("renamed", "R", &[("c", "C", 5.0)]),
    );
    write(
        &exts,
        "future/init.lua",
        "return { id = 'future', name = 'Future', api = 99 }",
    );
    let (mut w, dialogs) = window_with(&dir);
    assert!(w.actions.get("ext.good.c").is_some());
    assert!(w.actions.get("ext.renamed.c").is_none());
    let failed = w
        .actions
        .get(extensions::FAILED)
        .expect("the failure entry");
    assert_eq!(failed.text, "3 extensions failed to load");
    assert!(!failed.enabled);
    let mut expected = vec![act("ext.good.c"), MenuItem::Separator];
    expected.extend(fixed());
    expected.extend([
        MenuItem::Separator,
        act(extensions::FAILED),
        act(extensions::SHOW_FAILURES),
    ]);
    assert_eq!(ext_menu(&w), expected);

    assert!(w.trigger(extensions::SHOW_FAILURES));
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, "report");
    let text = &asked[0].1;
    assert!(
        text.starts_with("Extensions that failed to load: "),
        "{text}"
    );
    for folder in ["broken", "future", "other"] {
        assert!(
            text.contains(&exts.join(folder).display().to_string()),
            "{folder} in {text}"
        );
    }
    assert!(text.contains("does not match"), "{text}");
    assert!(text.contains("API version 99"), "{text}");
    assert!(text.contains("broken/init.lua:1:"), "{text}");
}

#[test]
fn run_lua_script_runs_the_picked_file_once() {
    let dir = TmpDir::new();
    let script = dir.join("square.lua");
    write(
        dir.path(),
        "square.lua",
        "local doc = ...\n\
         doc:add_face({ xtrude.vec(0, 0), xtrude.vec(25, 0), xtrude.vec(25, 25), xtrude.vec(0, 25) })\n",
    );
    let (mut w, dialogs) = window_with(&dir);
    dialogs.borrow_mut().open_paths.push(Some(script));
    assert!(w.trigger(extensions::RUN_SCRIPT));
    assert_eq!(
        dialogs.borrow().asked[0],
        ("open".into(), "Run Lua Script".into())
    );
    assert_eq!(w.viewport.history.undo_len(), 1);
    assert_eq!(w.viewport.history.undo_name(), Some("Run square.lua"));
    let (lo, hi) = w.viewport.scene.compute_bounds().expect("a face");
    assert!(((hi - lo).y - 0.025).abs() < 1e-9, "{lo} {hi}");

    // cancelled: nothing runs
    assert!(w.trigger(extensions::RUN_SCRIPT));
    assert_eq!(w.viewport.history.undo_len(), 1);
}

#[test]
fn run_lua_script_reports_a_syntax_error() {
    let dir = TmpDir::new();
    write(dir.path(), "bad.lua", "x = = 1");
    let (mut w, dialogs) = window_with(&dir);
    dialogs
        .borrow_mut()
        .open_paths
        .push(Some(dir.join("bad.lua")));
    assert!(w.trigger(extensions::RUN_SCRIPT));
    assert_eq!(w.viewport.history.undo_len(), 0);
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked[1].0, "report");
    assert!(asked[1].1.contains("bad.lua:1:"), "{asked:?}");
}

#[test]
fn reload_picks_up_a_new_extension_and_drops_a_removed_one() {
    let dir = TmpDir::new();
    let exts = dir.join("extensions");
    write(
        &exts,
        "old/init.lua",
        &manifest("old", "Old", &[("c", "Old one", 5.0)]),
    );
    let (mut w, _) = window_with(&dir);
    assert!(w.actions.get("ext.old.c").is_some());

    std::fs::remove_dir_all(exts.join("old")).expect("remove");
    write(
        &exts,
        "new/init.lua",
        &manifest("new", "New", &[("c", "New one", 5.0)]),
    );
    assert!(w.trigger(extensions::RELOAD));
    assert!(w.actions.get("ext.old.c").is_none());
    w.trigger("ext.old.c");
    assert_eq!(
        w.viewport.history.undo_len(),
        0,
        "a dropped command runs nothing"
    );
    assert_eq!(
        w.actions.get("ext.new.c").map(|a| a.text.as_str()),
        Some("New one")
    );
    let mut expected = vec![act("ext.new.c"), MenuItem::Separator];
    expected.extend(fixed());
    assert_eq!(ext_menu(&w), expected);
}

#[test]
fn open_extensions_folder_creates_it_and_asks_the_shell() {
    let dir = TmpDir::new();
    let (mut w, _) = window_with(&dir);
    let exts = dir.join("extensions");
    assert!(!exts.exists());
    assert!(w.trigger(extensions::OPEN_FOLDER));
    assert!(exts.is_dir());
    assert_eq!(w.open_folder_requested.as_deref(), Some(exts.as_path()));
}

#[test]
fn the_shell_queues_reports_apart_from_warnings() {
    use xtrude_app::shell::dialogs::ShellDialogs;
    use xtrude_app::window::{Dialogs, NoDialogs};
    let mut d = ShellDialogs::default();
    d.report("First", "line 1\nline 2");
    d.report("Second", "text");
    d.warn("Note", "a status line");
    let titles: Vec<&str> = d.reports.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(titles, ["First", "Second"]);
    assert_eq!(d.reports[0].1, "line 1\nline 2");
    assert_eq!(
        d.warnings,
        [("Note".to_string(), "a status line".to_string())]
    );
    // an answerer without a report dialog warns instead
    NoDialogs.report("Title", "text");
}

// ---- command parameters ------------------------------------------------------------

const BLOCK: &str = r#"
return { id = "block", name = "Blocks", api = 1, commands = {
  { id = "make", label = "Block…",
    params = {
      { key = "count", label = "Count", type = "int", default = 2, min = 1, max = 10 },
      { key = "side", label = "Side", type = "length", default = 10 },
      { key = "named", label = "Named", type = "bool", default = false },
      { key = "shape", label = "Shape", type = "choice", choices = { "flat", "tall" } },
    },
    run = function(doc, p)
      assert(math.type(p.count) == "integer")
      local s = p.side
      doc:add_face({ xtrude.vec(0, 0), xtrude.vec(s, 0), xtrude.vec(s, s), xtrude.vec(0, s) })
      if p.named then
        doc:make_group(doc:faces(), p.shape .. " x" .. p.count)
      end
    end },
} }
"#;

fn block_window(dir: &TmpDir) -> (MainWindow, Rc<RefCell<ScriptedDialogs>>) {
    write(&dir.join("extensions"), "block/init.lua", BLOCK);
    window_with(dir)
}

fn side_of(w: &MainWindow) -> f64 {
    let (lo, hi) = w.viewport.scene.compute_bounds().expect("a face");
    (hi - lo).x
}

#[test]
fn a_command_asks_for_its_params_and_runs_with_them() {
    let dir = TmpDir::new();
    let (mut w, dialogs) = block_window(&dir);
    dialogs.borrow_mut().forms.push(Some(vec![
        ParamValue::Int(3),
        ParamValue::Length(25.0),
        ParamValue::Bool(true),
        ParamValue::Choice(1),
    ]));
    assert!(w.trigger("ext.block.make"));
    let d = dialogs.borrow();
    assert_eq!(d.asked[0], ("form".into(), "Block".into()));
    assert_eq!(
        d.form_values[0],
        [
            ParamValue::Int(2),
            ParamValue::Length(10.0),
            ParamValue::Bool(false),
            ParamValue::Choice(0),
        ],
        "the defaults the first time"
    );
    assert_eq!(w.viewport.history.undo_len(), 1);
    assert!((side_of(&w) - 0.025).abs() < 1e-9);
    let names: Vec<&str> = w
        .viewport
        .scene
        .doc
        .groups
        .iter()
        .map(|g| g.name.as_str())
        .collect();
    assert_eq!(names, ["tall x3"]);
}

#[test]
fn an_out_of_range_value_is_refused() {
    let dir = TmpDir::new();
    let (mut w, dialogs) = block_window(&dir);
    dialogs.borrow_mut().forms.push(Some(vec![
        ParamValue::Int(11),
        ParamValue::Length(25.0),
        ParamValue::Bool(true),
        ParamValue::Choice(1),
    ]));
    assert!(w.trigger("ext.block.make"));
    assert_eq!(w.viewport.history.undo_len(), 0);
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked[1].0, "report");
    assert!(
        asked[1].1.contains("Count must be between 1 and 10"),
        "{asked:?}"
    );
    // a refused value is not remembered
    assert!(w.trigger("ext.block.make"));
    assert_eq!(dialogs.borrow().form_values[1][0], ParamValue::Int(2));
}

#[test]
fn cancel_runs_nothing_and_adds_no_step() {
    let dir = TmpDir::new();
    let (mut w, dialogs) = block_window(&dir);
    dialogs.borrow_mut().forms.push(None);
    assert!(w.trigger("ext.block.make"));
    assert_eq!(w.viewport.history.undo_len(), 0);
    assert!(w.viewport.scene.compute_bounds().is_none());
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked, [("form".to_string(), "Block".to_string())]);
}

#[test]
fn the_last_values_are_offered_next_time() {
    let dir = TmpDir::new();
    let (mut w, dialogs) = block_window(&dir);
    let given = vec![
        ParamValue::Int(4),
        ParamValue::Length(12.5),
        ParamValue::Bool(true),
        ParamValue::Choice(1),
    ];
    dialogs.borrow_mut().forms.push(Some(given.clone()));
    assert!(w.trigger("ext.block.make"));
    let stored = settings::read(&keys::ext_params("block", "make")).0;
    assert_eq!(
        stored,
        serde_json::json!({ "count": 4, "side": 0.0125, "named": true, "shape": "tall" })
    );
    assert!(w.trigger("ext.block.make"));
    assert_eq!(dialogs.borrow().form_values[1], given);

    // lengths are kept in metres: in a cm document 12.5 mm is 1.25 cm
    w.viewport.scene.apply_units(&Units::new("cm", 2));
    assert!(w.trigger("ext.block.make"));
    assert_eq!(dialogs.borrow().form_values[2][1], ParamValue::Length(1.25));

    // a stored value no longer valid falls back to the default, which is
    // in model units as the manifest wrote it
    settings::write(
        &keys::ext_params("block", "make"),
        &JsonText(serde_json::json!({ "count": 99, "shape": "round", "named": true })),
    );
    assert!(w.trigger("ext.block.make"));
    assert_eq!(
        dialogs.borrow().form_values[3],
        [
            ParamValue::Int(2),
            ParamValue::Length(10.0),
            ParamValue::Bool(true),
            ParamValue::Choice(0),
        ]
    );
}

#[test]
fn the_shell_asks_the_form_and_replays_the_command() {
    use xtrude_app::shell::dialogs::{answer, Answer, Question, ShellDialogs};
    use xtrude_app::tray::Tray;
    let dir = TmpDir::new();
    write(&dir.join("extensions"), "block/init.lua", BLOCK);
    let mut w = common::window();
    w.viewport.history.error_log = Some(dir.join("errors.log"));
    let dialogs = Rc::new(RefCell::new(ShellDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    w.load_extensions(Some(dir.join("extensions")));
    assert!(w.trigger("ext.block.make"));
    assert_eq!(
        w.viewport.history.undo_len(),
        0,
        "nothing runs before the answer"
    );
    let pending = dialogs.borrow().pending.clone();
    match pending {
        Some(Question::Form {
            title,
            fields,
            values,
        }) => {
            assert_eq!(title, "Block");
            assert_eq!(fields.len(), 4);
            assert_eq!(values[1], ParamValue::Length(10.0));
        }
        other => panic!("a form, not {other:?}"),
    }
    let mut tray = Tray::default();
    let given = vec![
        ParamValue::Int(1),
        ParamValue::Length(30.0),
        ParamValue::Bool(false),
        ParamValue::Choice(0),
    ];
    answer(&dialogs, Answer::Form(Some(given)), &mut w, &mut tray);
    assert_eq!(w.viewport.history.undo_len(), 1);
    assert_eq!(w.viewport.history.undo_name(), Some("Block"));
    assert!((side_of(&w) - 0.03).abs() < 1e-9);
    assert!(dialogs.borrow().pending.is_none());
}

#[test]
fn form_entries_read_lengths_like_the_measurements_box() {
    use xtrude_app::window::form::{field_text, parse_field};
    let dir = TmpDir::new();
    let (w, _) = block_window(&dir);
    let ext = &w.extensions.loaded[0];
    let specs = &ext.commands[0].params;
    let (count, side) = (&specs[0], &specs[1]);
    let none = ParamValue::Bool(false);
    // a bare number is in the document's unit (mm), a typed unit converts
    assert_eq!(parse_field(side, "12", &none), Ok(ParamValue::Length(12.0)));
    assert_eq!(
        parse_field(side, "1.5cm", &none),
        Ok(ParamValue::Length(15.0))
    );
    assert_eq!(parse_field(side, "2,5", &none), Ok(ParamValue::Length(2.5)));
    assert!(parse_field(side, "wide", &none).is_err());
    assert_eq!(parse_field(count, " 7 ", &none), Ok(ParamValue::Int(7)));
    let err = parse_field(count, "0", &none).expect_err("below the minimum");
    assert!(err.contains("Count must be between 1 and 10"), "{err}");
    assert!(parse_field(count, "2.5", &none).is_err());
    assert_eq!(field_text(&ParamValue::Length(12.5)), "12.5");
    assert_eq!(field_text(&ParamValue::Length(10.0)), "10");
    assert_eq!(field_text(&ParamValue::Int(3)), "3");
    // a check box or a choice keeps its value
    assert_eq!(
        parse_field(&specs[2], "", &ParamValue::Bool(true)),
        Ok(ParamValue::Bool(true))
    );
}

// ---- exports ----------------------------------------------------------------------------

#[test]
fn a_script_exports_where_the_save_dialog_answers() {
    let dir = TmpDir::new();
    write(
        dir.path(),
        "export.lua",
        "local doc = ...\n\
         doc:add_face({ xtrude.vec(0, 0), xtrude.vec(10, 0), xtrude.vec(0, 10) })\n\
         doc:export_stl(xtrude.ask_save_path('Export the part', 'STL mesh (*.stl)'))\n",
    );
    let (mut w, dialogs) = window_with(&dir);
    let out = dir.join("part.stl");
    {
        let mut d = dialogs.borrow_mut();
        d.open_paths.push(Some(dir.join("export.lua")));
        d.save_paths.push(Some(out.clone()));
    }
    assert!(w.trigger(extensions::RUN_SCRIPT));
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked[1], ("save".into(), "Export the part".into()));
    assert_eq!(w.viewport.history.undo_len(), 1);
    let bytes = std::fs::read(&out).expect("the STL was written");
    assert_eq!(bytes.len(), 84 + 50);

    // cancelled: the script gets nil, and export_stl refuses it
    dialogs
        .borrow_mut()
        .open_paths
        .push(Some(dir.join("export.lua")));
    assert!(w.trigger(extensions::RUN_SCRIPT));
    assert_eq!(w.viewport.history.undo_len(), 1);
    let asked = dialogs.borrow().asked.clone();
    let last = asked.last().expect("a report");
    assert_eq!(last.0, "report");
    assert!(last.1.contains("the save dialog was cancelled"), "{last:?}");
}
