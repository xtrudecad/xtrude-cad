// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The app shows colours, so what the user reads says "color", never
//! "material". The one material it names is what a part is printed in,
//! always called a "print material".

use crate::common;

use common::{v3, window};
use xtrude_app::window::hints::{hint_for, started_texts};
use xtrude_app::window::{tool_keys, CtxItem, MainWindow};
use xtrude_core::attrs::Paint;
use xtrude_core::snap::LinearMode;
use xtrude_model::entity::EntityRef;
use xtrude_model::ids::MeshId;
use xtrude_model::scene::SelectMode;

fn says_material(text: &str) -> bool {
    text.to_lowercase()
        .replace("print material", "")
        .contains("material")
}

#[test]
fn test_only_a_print_material_is_a_material() {
    assert!(says_material("Paint material"));
    assert!(says_material("Print material, or the material it gets"));
    assert!(!says_material("Document print material…"));
}

#[test]
fn test_the_eyedropper_is_sample_color() {
    let w = window();
    let a = w.actions.get("eyedropper").expect("the eyedropper action");
    assert_eq!(a.text, "Sample Color");
    assert!(
        a.status_tip.starts_with("Sample Color — "),
        "{}",
        a.status_tip
    );
}

#[test]
fn test_no_action_label_or_tip_says_material() {
    let w = window();
    let bad: Vec<String> = w
        .actions
        .iter()
        .filter(|a| says_material(&a.text) || says_material(&a.status_tip))
        .map(|a| format!("{}: {} / {}", a.id, a.text, a.status_tip))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn test_no_tool_name_description_or_hint_says_material() {
    let mut w = window();
    let mut bad = Vec::new();
    for key in tool_keys() {
        w.activate_tool(key);
        let Some(tool) = w.viewport.active_tool() else {
            continue;
        };
        let info = tool.info();
        let mut texts = vec![info.name.to_string()];
        texts.extend(info.description.map(String::from));
        for mode in [LinearMode::All, LinearMode::Off, LinearMode::ParallelPerp] {
            texts.push(hint_for(Some(key), Some(tool), None, Some(mode)));
        }
        texts.extend(started_texts(key));
        bad.extend(
            texts
                .into_iter()
                .filter(|t| says_material(t))
                .map(|t| format!("{key}: {t}")),
        );
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Right-click ▸ Select: `(id, text)` per entry.
fn select_submenu(w: &MainWindow) -> Vec<(String, String)> {
    w.context_menu()
        .into_iter()
        .find_map(|i| match i {
            CtxItem::Menu(t, sub) if t == "Select" => Some(
                sub.into_iter()
                    .filter_map(|s| match s {
                        CtxItem::Entry { id, text, .. } => Some((id, text)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn test_select_all_with_same_color_takes_the_faces_of_that_color() {
    let mut w = window();
    let red = Paint {
        color: Some([0.8, 0.1, 0.1]),
        ..Default::default()
    };
    let mut faces = Vec::new();
    for (x, paint) in [(0., Some(&red)), (3., None), (6., Some(&red))] {
        let mesh = w.viewport.scene.mesh_mut();
        let f = mesh.add_face(
            &[
                v3(x, 0., 0.),
                v3(x + 2., 0., 0.),
                v3(x + 2., 2., 0.),
                v3(x, 2., 0.),
            ],
            &[],
        );
        if let Some(p) = paint {
            mesh.face_mut(f).attrs.paint = p.clone();
        }
        faces.push(EntityRef::Face(MeshId::LOOSE, f));
    }
    w.viewport.scene.select([faces[0]], SelectMode::Replace);

    let entries = select_submenu(&w);
    assert!(
        entries.contains(&("select:same_color".into(), "All with Same Color".into())),
        "{entries:?}"
    );
    assert!(entries.iter().all(|(_, text)| !says_material(text)));

    w.trigger("select:same_color");
    let sel = &w.viewport.scene.selection;
    assert!(sel.contains(&faces[0]) && sel.contains(&faces[2]));
    assert!(!sel.contains(&faces[1]));
}
