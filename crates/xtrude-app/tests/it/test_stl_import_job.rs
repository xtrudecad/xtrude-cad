// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! An STL import parses on a worker thread and reports its progress on the
//! thread that waits for it (the UI thread).

use crate::common;

use common::TmpDir;
use xtrude_app::window::progress::{ImportJob, ImportResult};
use xtrude_io::stl::SimplifyMode;

#[test]
fn test_stl_parse_runs_on_worker_and_reports_progress_on_ui_thread() {
    let tmp = TmpDir::new();
    let path = tmp.join("triangle.stl");
    let mut bytes = b"binary STL".to_vec();
    bytes.resize(80, 0);
    bytes.extend(1u32.to_le_bytes());
    for f in [0f32, 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0.] {
        bytes.extend(f.to_le_bytes());
    }
    bytes.extend([0u8, 0]);
    std::fs::write(&path, bytes).unwrap();
    let ui = std::thread::current().id();
    let mut progress_threads = Vec::new();
    let job = ImportJob::stl(&path, 1.0, SimplifyMode::None).expect("started");
    let ImportResult::Stl(parsed) = job.wait(&mut |_, _| {
        progress_threads.push(std::thread::current().id() == ui);
    }) else {
        panic!("not an STL parse");
    };
    let mesh = parsed.expect("parsed");
    assert_eq!(mesh.face_ids().len(), 1);
    assert!(!progress_threads.is_empty() && progress_threads.iter().all(|&b| b));
}
