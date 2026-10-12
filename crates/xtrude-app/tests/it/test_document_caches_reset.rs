// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Opening a document lets the previous one go: the caches are values owned by
//! the viewport and replaced, so what is checked is that nothing piles up
//! across reopenings.

use crate::common;

use std::path::Path;

#[test]
fn test_reopening_a_document_keeps_no_chunk_or_face_of_the_old_one() {
    let doc =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/banca-pergola-yanque.igz");
    let mut win = common::window();
    let mut counts = Vec::new();
    for _ in 0..3 {
        win.open_path(&doc);
        let f = win.viewport.frame_data([400, 300], false);
        counts.push((
            win.viewport.placements().len(),
            f.scene_faces.data.len(),
            win.viewport.scene.doc.meshes.len(),
        ));
    }
    assert!(counts[0].0 > 0 && counts[0].1 > 0);
    assert_eq!(counts[1], counts[2]); // nothing piles up
    assert_eq!(counts[2], counts[0]);
}
