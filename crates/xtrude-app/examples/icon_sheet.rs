// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Render every toolbar icon onto one sheet: `cargo run --example icon_sheet -- out.png`.
use xtrude_app::shell::icons::{render, Rgba};

const KEYS: &[&str] = &[
    "save",
    "select",
    "undo",
    "redo",
    "eraser",
    "paint",
    "eyedropper",
    "line",
    "freehand",
    "rectangle",
    "rotated_rect",
    "circle",
    "polygon",
    "arc",
    "arc3",
    "center_arc",
    "sector",
    "pushpull",
    "move",
    "rotate",
    "scale",
    "flip",
    "sweep",
    "offset",
    "fillet",
    "measure",
    "protractor",
    "dimension",
    "text",
    "text3d",
    "section",
    "section_planes",
    "section_cuts",
    "section_fill",
    "merge_solids",
    "solid_union",
    "solid_subtract",
    "solid_trim",
    "solid_intersect",
    "solid_split",
    "orbit",
    "pan",
    "zoom",
    "zoom_window",
    "zoom_extents",
    "zoom_selection",
    "view_iso",
    "view_top",
    "view_front",
    "view_right",
    "view_left",
    "view_back",
    "view_bottom",
    "styles",
    "dimension_style",
];

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "icons.png".into());
    let cols = 10u32;
    let cell = 64u32;
    let rows = (KEYS.len() as u32).div_ceil(cols);
    let mut sheet = tiny_skia::Pixmap::new(cols * cell, rows * cell).unwrap();
    sheet.fill(tiny_skia::Color::from_rgba8(48, 50, 56, 255));
    for (i, k) in KEYS.iter().enumerate() {
        let Some(r) = render(k, 1.0, Rgba(220, 224, 230, 255)) else {
            eprintln!("no icon: {k}");
            continue;
        };
        let img = tiny_skia::Pixmap::from_vec(
            r.rgba,
            tiny_skia::IntSize::from_wh(r.size, r.size).unwrap(),
        )
        .unwrap();
        let (x, y) = ((i as u32 % cols) * cell + 8, (i as u32 / cols) * cell + 8);
        sheet.draw_pixmap(
            x as i32,
            y as i32,
            img.as_ref(),
            &Default::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }
    sheet.save_png(out).unwrap();
}
