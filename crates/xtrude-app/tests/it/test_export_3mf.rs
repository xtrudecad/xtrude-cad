// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File ▸ Export ▸ 3MF writes the package where the user chose.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{add_box, v3, TmpDir};
use xtrude_app::window::ScriptedDialogs;

#[test]
fn test_export_3mf_writes_the_package_with_its_extension() {
    let tmp = TmpDir::new();
    let mut w = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    add_box(&mut w.viewport, v3(0., 0., 0.), v3(0.01, 0.01, 0.01));
    dialogs.borrow_mut().save_paths.push(Some(tmp.join("part")));
    assert!(w.trigger("export:3mf"));
    let asked = dialogs.borrow().asked.clone();
    assert_eq!(asked, [("save".to_string(), "Export 3MF".to_string())]);
    let bytes = std::fs::read(tmp.join("part.3mf")).expect("the 3MF was written");
    assert!(bytes.starts_with(b"PK\x03\x04"), "a zip package");
    let has = |name: &[u8]| bytes.windows(name.len()).any(|w| w == name);
    assert!(has(b"3D/3dmodel.model") && has(b"[Content_Types].xml") && has(b"_rels/.rels"));
    let msg = w.status_message.as_ref().map(|m| m.text.clone());
    assert_eq!(msg.as_deref(), Some("Exported 3MF to part.3mf"));
}

#[test]
fn test_export_3mf_cancelled_writes_nothing() {
    let tmp = TmpDir::new();
    let mut w = common::window();
    let dialogs = Rc::new(RefCell::new(ScriptedDialogs::default()));
    w.dialogs = Box::new(dialogs.clone());
    add_box(&mut w.viewport, v3(0., 0., 0.), v3(0.01, 0.01, 0.01));
    assert!(w.trigger("export:3mf"));
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}
