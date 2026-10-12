// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! File ▸ Open Recent is ordered, deduplicated and capped; imports are not
//! offered, nor files deleted meanwhile.

use crate::common;

use common::{window, TmpDir};
use xtrude_app::window::files::{recent_menu, recent_paths, remember_recent};

const RECENT_MAX: usize = 10;

#[test]
fn test_recent_list_is_ordered_deduplicated_and_capped() {
    let _w = window(); // the memory settings store
    let dir = TmpDir::new();
    let mut docs = Vec::new();
    for i in 0..12 {
        let p = dir.path().join(format!("doc{i}.igz"));
        std::fs::write(&p, "{}").unwrap();
        remember_recent(&p);
        docs.push(p);
    }
    let canon = |p: &std::path::Path| p.canonicalize().unwrap().to_string_lossy().into_owned();
    let recent = recent_paths();
    assert_eq!(recent.len(), RECENT_MAX);
    assert_eq!(recent[0], canon(&docs[11])); // newest first
    remember_recent(&docs[9]); // re-open an old one
    assert_eq!(recent_paths()[0], canon(&docs[9]));
    assert_eq!(recent_paths().len(), RECENT_MAX);
    remember_recent(&dir.path().join("model.skp")); // an import
    assert!(!recent_paths().iter().any(|x| x.ends_with(".skp")));
    // A file deleted meanwhile is not offered.
    std::fs::remove_file(&docs[11]).unwrap();
    let labels: Vec<String> = recent_menu().iter().map(|i| i.text().to_string()).collect();
    assert!(!labels.iter().any(|t| t.contains("doc11.igz")));
    assert!(labels.iter().any(|t| t.contains("doc9.igz")));
    assert!(labels.iter().any(|t| t.contains("Clear")));
}
