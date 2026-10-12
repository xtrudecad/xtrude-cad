// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The tray's panels on screen, drawn from the models in [`crate::tray`].

use xtrude_model::i18n::tr;
use xtrude_model::ids::GroupId;

use crate::tray::model_tree::Row;
use crate::tray::parts::{AllVisible, EXPLODE_MODES};
use crate::tray::styles::{Styles, MODES, SWATCHES};
use crate::tray::{drop_index, Tray, TrayAction, SECTIONS};
use crate::window::MainWindow;

/// Per-frame UI state the models do not keep (text being typed).
#[derive(Default)]
pub struct TrayUi {
    renaming: Option<(GroupId, String)>,
    layer_renaming: Option<(String, String)>,
    view_renaming: Option<(u64, String)>,
    name_focus: bool,
    /// The model tree's group and component icons.
    icons: super::IconCache,
}

fn rgb32(c: [f64; 3]) -> egui::Color32 {
    let q = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// The width of a section header's grip.
const GRIP_WIDTH: f32 = 14.0;
/// Where a section's title starts, from the tray's left edge (after the
/// grip and the fold arrow); its body lines up with it.
const TITLE_INSET: f32 = GRIP_WIDTH + 18.0;

/// The faint rule between two sections, inset from the tray's edges.
fn separator(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 7.0), egui::Sense::hover());
    // the theme's separator stroke is near the panel fill: a faint weak-text tone
    let stroke = egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.35));
    let x = egui::Rangef::new(rect.left() + 6.0, rect.right() - 6.0);
    ui.painter().hline(x, rect.center().y, stroke);
}

/// What the user did on a section header this frame.
enum HeaderEvent {
    None,
    /// The title was clicked: fold or unfold.
    Toggle,
    /// The close button was clicked.
    Close,
    /// The grip is being dragged.
    Drag,
    /// The grip was let go.
    Drop,
}

/// A section's header row: a grip of dots on the left, the fold arrow and
/// the title, and a close cross on the right.
fn section_header(ui: &mut egui::Ui, key: &str, title: &str, open: bool) -> HeaderEvent {
    let (row, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    let grip = egui::Rect::from_min_size(row.min, egui::vec2(GRIP_WIDTH, row.height()));
    let close = egui::Rect::from_center_size(
        egui::pos2(row.right() - 10.0, row.center().y),
        egui::vec2(16.0, 16.0),
    );
    let head = egui::Rect::from_min_max(
        egui::pos2(grip.right(), row.top()),
        egui::pos2(close.left() - 2.0, row.bottom()),
    );
    let id = egui::Id::new(("tray-section", key));
    let grip_r = ui.interact(grip, id.with("grip"), egui::Sense::drag());
    let head_r = ui.interact(head, id.with("head"), egui::Sense::click());
    let close_r = ui.interact(close, id.with("close"), egui::Sense::click());
    let visuals = ui.visuals().clone();
    let painter = ui.painter();

    let ink = if grip_r.hovered() || grip_r.dragged() {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    for dy in [-4.0, 0.0, 4.0] {
        for dx in [-2.0, 2.0] {
            let c = grip.center() + egui::vec2(dx, dy);
            painter.rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(2.0, 2.0)),
                0.0,
                ink,
            );
        }
    }

    if head_r.hovered() {
        painter.rect_filled(head, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let text = visuals.strong_text_color();
    let a = egui::pos2(head.left() + 9.0, head.center().y);
    let arrow = if open {
        vec![
            a + egui::vec2(-4.0, -2.0),
            a + egui::vec2(4.0, -2.0),
            a + egui::vec2(0.0, 3.0),
        ]
    } else {
        vec![
            a + egui::vec2(-2.0, -4.0),
            a + egui::vec2(3.0, 0.0),
            a + egui::vec2(-2.0, 4.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(arrow, text, egui::Stroke::NONE));
    painter.text(
        egui::pos2(row.left() + TITLE_INSET, head.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::TextStyle::Body.resolve(ui.style()),
        text,
    );

    let cross = if close_r.hovered() {
        painter.rect_filled(close, 3.0, visuals.widgets.hovered.weak_bg_fill);
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    let (c, r) = (close.center(), 3.0);
    let stroke = egui::Stroke::new(1.2, cross);
    painter.line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
    painter.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], stroke);

    let event = if close_r.clicked() {
        HeaderEvent::Close
    } else if head_r.clicked() {
        HeaderEvent::Toggle
    } else if grip_r.drag_stopped() {
        HeaderEvent::Drop
    } else if grip_r.dragged() {
        HeaderEvent::Drag
    } else {
        HeaderEvent::None
    };
    let carrying = grip_r.dragged();
    grip_r
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text(tr("Drag to move this panel up or down.", &[]));
    if carrying {
        // after the grip's own hover cursor; the pointer may have left it
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    head_r.on_hover_cursor(egui::CursorIcon::PointingHand);
    close_r.on_hover_text(tr("Close this panel. Window > Panels brings it back.", &[]));
    event
}

/// `<b>…</b>` and `<br>` as egui text.
fn rich(ui: &mut egui::Ui, markup: &str) {
    for line in markup.split("<br>") {
        if let Some(b) = line
            .strip_prefix("<b>")
            .and_then(|l| l.strip_suffix("</b>"))
        {
            ui.label(egui::RichText::new(b).strong());
        } else {
            ui.label(line.replace("<b>", "").replace("</b>", ""));
        }
    }
}

fn swatch(ui: &mut egui::Ui, color: egui::Color32, tip: &str, size: f32) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    ui.painter().rect_filled(rect, 3.0, color);
    ui.painter().rect_stroke(
        rect,
        3.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    if tip.is_empty() {
        r
    } else {
        r.on_hover_text(tip)
    }
}

impl TrayUi {
    /// The Properties tray: its shown sections in the user's order, each
    /// folded as the user left it, under a header with a grip to drag it
    /// elsewhere and a button to close it.
    pub fn show(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let shown: Vec<(&str, &str)> = Tray::section_order()
            .into_iter()
            .filter(|key| Tray::section_shown(key))
            .filter_map(|key| SECTIONS.iter().find(|(k, _)| *k == key).copied())
            .collect();
        if shown.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(tr(
                        "All panels are hidden. Window > Panels shows them again.",
                        &[],
                    ))
                    .weak(),
                );
            });
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut spans: Vec<(&str, f32, f32)> = Vec::new();
                let mut dragged = None;
                let mut dropped = None;
                for (key, title) in shown {
                    let top = ui.cursor().top();
                    if !spans.is_empty() {
                        separator(ui);
                    }
                    let open = Tray::section_open(key);
                    match section_header(ui, key, &tr(title, &[]), open) {
                        HeaderEvent::Toggle => Tray::set_section_open(key, !open),
                        HeaderEvent::Close => w.set_tray_section_shown(key, false),
                        HeaderEvent::Drag => dragged = Some(key),
                        HeaderEvent::Drop => dropped = Some(key),
                        HeaderEvent::None => {}
                    }
                    if open {
                        // the body starts under the title, with no
                        // indent line (the header and the rules mark it);
                        // nested indents inside it keep the theme's own
                        let (indent, vline) =
                            (ui.spacing().indent, ui.visuals().indent_has_left_vline);
                        ui.scope(|ui| {
                            ui.visuals_mut().indent_has_left_vline = false;
                            ui.spacing_mut().indent = TITLE_INSET;
                            ui.indent(key, |ui| {
                                ui.spacing_mut().indent = indent;
                                ui.visuals_mut().indent_has_left_vline = vline;
                                match key {
                                    "properties" => self.properties(ui, tray, w),
                                    "model_tree" => self.model_tree(ui, tray, w),
                                    "layers" => self.layers(ui, tray, w),
                                    "saved_views" => self.saved_views(ui, tray, w),
                                    "colors" => self.colors(ui, tray, w),
                                    "components" => self.components(ui, tray, w),
                                    _ => self.parts(ui, tray, w),
                                }
                            });
                        });
                    }
                    ui.add_space(4.0);
                    spans.push((key, top, ui.cursor().top()));
                }
                let pointer = ui.ctx().pointer_latest_pos();
                let bounds: Vec<(f32, f32)> = spans.iter().map(|(_, t, b)| (*t, *b)).collect();
                if let (Some(_), Some(p)) = (dragged, pointer) {
                    let i = drop_index(&bounds, p.y);
                    let y = spans
                        .get(i)
                        .map_or_else(|| spans.last().map_or(p.y, |s| s.2), |s| s.1);
                    let x = ui.clip_rect().x_range();
                    let accent = egui::Stroke::new(2.0, ui.visuals().selection.bg_fill);
                    ui.painter().hline(x, y, accent);
                }
                if let (Some(key), Some(p)) = (dropped, pointer) {
                    let before = spans.get(drop_index(&bounds, p.y)).map(|s| s.0);
                    Tray::move_section(key, before);
                }
            });
    }

    fn properties(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.properties;
        ui.set_min_height(64.0);
        rich(ui, &p.text.clone());
        if p.named.is_some() {
            ui.horizontal(|ui| {
                ui.label(tr("Name:", &[]));
                let r =
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
                self.name_focus = r.has_focus();
                if r.lost_focus() {
                    let text = p.name.clone();
                    p.name_edited(w, &text);
                }
            });
        }
        if p.layer.visible {
            ui.horizontal(|ui| {
                ui.label(tr("Layer:", &[]));
                let current = p.layer.current;
                let label = p
                    .layer
                    .items
                    .get(current)
                    .map(|i| i.0.clone())
                    .unwrap_or_default();
                let mut picked = None;
                egui::ComboBox::from_id_salt("ei-layer")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (i, (text, _)) in p.layer.items.iter().enumerate() {
                            if ui.selectable_label(i == current, text).clicked() {
                                picked = Some(i);
                            }
                        }
                    });
                if let Some(i) = picked {
                    p.layer_picked(w, i);
                }
            });
        }
    }

    fn model_tree(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let ol = &mut tray.model_tree;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut ol.filter)
                    .hint_text(tr("Filter", &[]))
                    .desired_width(ui.available_width() - 36.0),
            );
            ui.menu_button("…", |ui| {
                if ui.button(tr("Expand all", &[])).clicked() {
                    ol.expand_all();
                    ui.close();
                }
                if ui.button(tr("Collapse all", &[])).clicked() {
                    ol.collapse_all();
                    ui.close();
                }
            });
        });
        let mut action: Option<ModelTreeAction> = None;
        egui::ScrollArea::vertical()
            .max_height(24.0 * 20.0)
            .id_salt("ol-scroll")
            .show(ui, |ui| {
                let root = ui.selectable_label(
                    ol.root_selected,
                    egui::RichText::new(&ol.root_name).strong(),
                );
                if root.clicked() {
                    action = Some(ModelTreeAction::Root);
                }
                if let Some(g) = root.dnd_release_payload::<GroupId>() {
                    action = Some(ModelTreeAction::Drop(*g, None));
                }
                let rows = ol.rows.clone();
                for r in &rows {
                    self.model_tree_row(ui, ol, r, 1, &mut action);
                }
            });
        if let Some(a) = action {
            match a {
                ModelTreeAction::Root => ol.tree_selection(w, &[], None, true),
                ModelTreeAction::Click(g, add) => {
                    let mut rows = if add { ol.selected.clone() } else { Vec::new() };
                    if !rows.contains(&g) {
                        rows.push(g);
                    }
                    ol.tree_selection(w, &rows, Some(g), false);
                }
                ModelTreeAction::Edit(g) => ol.edit_group(w, g),
                ModelTreeAction::Rename(g, name) => ol.rename(w, g, &name),
                ModelTreeAction::Hide(gs, h) => ol.set_hidden(w, &gs, h),
                ModelTreeAction::Drop(dragged, parent) => {
                    if !ol.selected.contains(&dragged) {
                        ol.selected = vec![dragged];
                    }
                    if ol.can_drop(w, parent) {
                        ol.drop(w, parent, None);
                    }
                }
            }
        }
    }

    fn model_tree_row(
        &mut self,
        ui: &mut egui::Ui,
        ol: &mut crate::tray::model_tree::ModelTree,
        r: &Row,
        depth: usize,
        action: &mut Option<ModelTreeAction>,
    ) {
        if !ol.shown(r.group) {
            return;
        }
        let open = ol.expanded.contains(&r.group);
        let kind = if r.component {
            tr("Component", &[])
        } else {
            tr("Group", &[])
        };
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 12.0);
            if r.children.is_empty() {
                // the triangle's place, with the spacing after it, so the
                // icons line up
                ui.add_space(14.0 + ui.spacing().item_spacing.x);
            } else {
                // egui's own collapsing triangle, painted: no font has one
                let (rect, toggle) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                let inner = ui.spacing().icon_width_inner;
                let icon_rect =
                    egui::Rect::from_center_size(rect.center(), egui::vec2(inner, inner));
                let openness = if open { 1.0 } else { 0.0 };
                egui::collapsing_header::paint_default_icon(
                    ui,
                    openness,
                    &toggle.clone().with_new_rect(icon_rect),
                );
                if toggle.clicked() {
                    if open {
                        ol.expanded.remove(&r.group);
                    } else {
                        ol.expanded.insert(r.group);
                    }
                }
            }
            let px = ui.text_style_height(&egui::TextStyle::Body).round();
            let (rect, icon) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
            let key = if r.component {
                "tree_component"
            } else {
                "tree_group"
            };
            let ink = ui.visuals().text_color();
            if let Some((tex, _)) = self.icons.get(ui.ctx(), key, ink, px) {
                ui.painter().image(
                    tex,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            icon.on_hover_text(&kind);
            if let Some((g, text)) = self.renaming.as_mut().filter(|(g, _)| *g == r.group) {
                let te = ui.text_edit_singleline(text);
                te.request_focus();
                if te.lost_focus() {
                    *action = Some(ModelTreeAction::Rename(*g, text.clone()));
                    self.renaming = None;
                }
                return;
            }
            let mut label = egui::RichText::new(&r.name);
            if r.hidden {
                label = label.italics().weak();
            }
            if ol.open.contains(&r.group) {
                label = label.strong();
            }
            let resp = ui.selectable_label(ol.selected.contains(&r.group), label);
            let resp = resp.on_hover_text(format!("{} — {kind}", r.name));
            if resp.drag_started() {
                resp.dnd_set_drag_payload(r.group);
            }
            if let Some(g) = resp.dnd_release_payload::<GroupId>() {
                *action = Some(ModelTreeAction::Drop(*g, Some(r.group)));
            }
            if resp.double_clicked() {
                *action = Some(ModelTreeAction::Edit(r.group));
            } else if resp.clicked() {
                let add = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                *action = Some(ModelTreeAction::Click(r.group, add));
            }
            resp.context_menu(|ui| {
                let groups = if ol.selected.contains(&r.group) {
                    ol.selected.clone()
                } else {
                    vec![r.group]
                };
                if ui.button(tr("Edit", &[])).clicked() {
                    *action = Some(ModelTreeAction::Edit(r.group));
                    ui.close();
                }
                if ui.button(tr("Rename", &[])).clicked() {
                    self.renaming = Some((r.group, r.name.clone()));
                    ui.close();
                }
                if ui.button(tr("Hide", &[])).clicked() {
                    *action = Some(ModelTreeAction::Hide(groups.clone(), true));
                    ui.close();
                }
                if ui.button(tr("Unhide", &[])).clicked() {
                    *action = Some(ModelTreeAction::Hide(groups, false));
                    ui.close();
                }
                ui.separator();
                if ui.button(tr("Expand all", &[])).clicked() {
                    ol.expand_all();
                    ui.close();
                }
                if ui.button(tr("Collapse all", &[])).clicked() {
                    ol.collapse_all();
                    ui.close();
                }
            });
        });
        if open {
            for c in &r.children {
                self.model_tree_row(ui, ol, c, depth + 1, action);
            }
        }
    }

    fn layers(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.layers;
        let mut change: Option<(String, bool, bool)> = None;
        let mut rename: Option<(String, String)> = None;
        egui::Grid::new("layers-grid")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(tr("Name", &[])).weak());
                ui.label(egui::RichText::new(tr("Visible", &[])).weak());
                ui.label(egui::RichText::new(tr("Lock", &[])).weak());
                ui.end_row();
                for row in p.rows.clone() {
                    let current = p.current.as_deref() == Some(row.name.as_str());
                    match self.layer_renaming.as_mut().filter(|(n, _)| *n == row.name) {
                        Some((old, text)) => {
                            let te = ui.text_edit_singleline(text);
                            te.request_focus();
                            if te.lost_focus() {
                                rename = Some((old.clone(), text.clone()));
                            }
                        }
                        None => {
                            let r = ui.selectable_label(current, &row.name);
                            if r.clicked() {
                                p.current = Some(row.name.clone());
                            }
                            if r.double_clicked() && row.editable {
                                self.layer_renaming = Some((row.name.clone(), row.name.clone()));
                            }
                        }
                    }
                    let (mut v, mut l) = (row.visible, row.locked);
                    let cv = ui.checkbox(&mut v, "").changed();
                    let cl = ui.checkbox(&mut l, "").changed();
                    if cv || cl {
                        change = Some((row.name.clone(), v, l));
                    }
                    ui.end_row();
                }
            });
        if let Some((n, v, l)) = change {
            p.set_state(w, &n, v, l);
        }
        if let Some((old, new)) = rename {
            self.layer_renaming = None;
            p.rename(w, &old, &new);
        }
        let mut ask = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("+ Layer", &[])).clicked() {
                p.add(w);
            }
            if ui
                .button(tr("−", &[]))
                .on_hover_text(tr("Delete layer (its entities go to the default)", &[]))
                .clicked()
            {
                p.delete(w);
            }
            if ui
                .button(tr("Purge", &[]))
                .on_hover_text(tr(
                    "Delete every layer no entity carries (a layer a saved view hides is kept)",
                    &[],
                ))
                .clicked()
            {
                ask = Some(TrayAction::PurgeLayers);
            }
            if ui.button(tr("Assign selection", &[])).clicked() {
                p.assign(w);
            }
        });
        if let Some(a) = ask {
            tray.run(w, a);
        }
    }

    fn saved_views(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.saved_views;
        ui.label(egui::RichText::new(tr("Double-click a view to show it", &[])).weak());
        let mut activate = None;
        let mut rename = None;
        for (id, name) in p.rows.clone() {
            match self.view_renaming.as_mut().filter(|(i, _)| *i == id) {
                Some((_, text)) => {
                    let te = ui.text_edit_singleline(text);
                    te.request_focus();
                    if te.lost_focus() {
                        rename = Some((id, text.clone()));
                    }
                }
                None => {
                    let r = ui.selectable_label(p.current == Some(id), &name);
                    if r.clicked() {
                        p.current = Some(id);
                    }
                    if r.double_clicked() {
                        activate = Some(id);
                    }
                    r.context_menu(|ui| {
                        if ui.button(tr("Rename", &[])).clicked() {
                            self.view_renaming = Some((id, name.clone()));
                            ui.close();
                        }
                    });
                }
            }
        }
        if let Some(id) = activate {
            p.activate(w, id);
        }
        if let Some((id, text)) = rename {
            self.view_renaming = None;
            p.rename(w, id, &text);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(tr("+ View", &[]))
                .on_hover_text(tr("Save the current view and layer visibility", &[]))
                .clicked()
            {
                p.add(w);
            }
            if ui
                .button(tr("Update", &[]))
                .on_hover_text(tr("Update the selected view from the current one", &[]))
                .clicked()
            {
                p.update(w);
            }
            if ui
                .button(tr("−", &[]))
                .on_hover_text(tr("Delete the selected view", &[]))
                .clicked()
            {
                p.delete(w);
            }
        });
    }

    fn colors(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.colors;
        let st = crate::tools::paint::paint_state();
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Active:", &[]));
            if st.is_default {
                swatch(
                    ui,
                    egui::Color32::from_gray(200),
                    &tr("Default (no color)", &[]),
                    30.0,
                );
            } else {
                swatch(ui, rgb32(st.color), "", 30.0);
            }
            let r = swatch(
                ui,
                egui::Color32::from_gray(235),
                &tr(
                    "Default (no color) — paint with it to remove a face's or an object's color",
                    &[],
                ),
                30.0,
            );
            if r.clicked() {
                p.apply_default(w);
            }
        });
        ui.label(egui::RichText::new(tr("In model", &[])).weak().small());
        let mut apply: Option<([f64; 3], Option<String>)> = None;
        let mut edit: Option<(String, [f64; 3])> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for s in p.in_model.clone() {
                let r = swatch(ui, rgb32(s.rgb), &s.tip, 30.0);
                if r.clicked() {
                    apply = Some((s.rgb, s.name.clone()));
                }
                if let Some(n) = s.name.clone() {
                    r.context_menu(|ui| {
                        if ui.button(tr("Edit color…", &[])).clicked() {
                            edit = Some((n.clone(), s.rgb));
                            ui.close();
                        }
                    });
                }
            }
        });
        ui.label(egui::RichText::new(tr("Library", &[])).weak().small());
        let open = p.library_open;
        let r = egui::CollapsingHeader::new(p.library_title())
            .id_salt("ral")
            .open(Some(open))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                    for s in &p.library {
                        if swatch(ui, rgb32(s.rgb), &s.tip, 30.0).clicked() {
                            apply = Some((s.rgb, s.name.clone()));
                        }
                    }
                });
            });
        if r.header_response.clicked() {
            p.set_library_open(!open);
        }
        if let Some((rgb, name)) = apply {
            p.apply_color(w, rgb, name.as_deref());
        }
        let mut ask = edit.map(|(name, current)| TrayAction::EditNamedColor { name, current });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("+ Color…", &[])).clicked() {
                ask = Some(TrayAction::AddColor);
            }
            if ui
                .button(tr("Purge", &[]))
                .on_hover_text(tr("Delete every color no face wears", &[]))
                .clicked()
            {
                ask = Some(TrayAction::PurgeColors);
            }
        });
        if let Some(a) = ask {
            tray.run(w, a);
        }
    }

    fn components(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.components;
        ui.label(egui::RichText::new(tr("In model", &[])).strong());
        if p.rows.is_empty() {
            ui.label(
                egui::RichText::new(crate::tray::components::Components::empty_label()).weak(),
            );
        }
        let mut pick = None;
        for (i, r) in p.rows.iter().enumerate() {
            if ui
                .selectable_label(false, &r.label)
                .on_hover_text(tr(
                    "Components placed in this drawing — click one to select its copies",
                    &[],
                ))
                .clicked()
            {
                pick = Some(i);
            }
        }
        if let Some(i) = pick {
            p.select(w, i);
        }
    }

    fn parts(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.parts;
        rich(ui, &p.title.clone());
        if !p.lines.is_empty() {
            let mut all = p.all_visible == AllVisible::Checked;
            let r = ui.add(
                egui::Checkbox::new(&mut all, tr("All parts visible", &[]))
                    .indeterminate(p.all_visible == AllVisible::Partial),
            );
            if r.clicked() {
                p.toggle_all_visible(w);
            }
            let mut click: Option<GroupId> = None;
            let mut hide: Option<(GroupId, bool)> = None;
            let mut commit = false;
            egui::Grid::new("parts-grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(tr("Name", &[])).weak());
                    ui.label(egui::RichText::new(tr("Size", &[])).weak());
                    ui.label(egui::RichText::new(tr("Color", &[])).weak());
                    ui.end_row();
                    for l in p.lines.clone() {
                        ui.horizontal(|ui| {
                            let mut shown = !l.hidden;
                            if ui.checkbox(&mut shown, "").changed() {
                                hide = Some((l.part, !shown));
                            }
                            match p.editing.as_mut().filter(|(g, _)| *g == l.part) {
                                Some((_, text)) => {
                                    let te = ui.text_edit_singleline(text);
                                    te.request_focus();
                                    if te.lost_focus() {
                                        commit = true;
                                    }
                                }
                                None => {
                                    let r = ui
                                        .selectable_label(l.selected, &l.name)
                                        .on_hover_text(tr("{n} faces", &[("n", &l.faces)]));
                                    if p.scroll_to == Some(l.part) {
                                        p.scroll_to = None;
                                        r.scroll_to_me(None);
                                    }
                                    if r.double_clicked() {
                                        p.begin_rename(l.part);
                                    } else if r.clicked() {
                                        click = Some(l.part);
                                    }
                                }
                            }
                        });
                        let s = ui.label(&l.size);
                        if !l.size_tip.is_empty() {
                            s.on_hover_text(&l.size_tip);
                        }
                        ui.label(&l.color);
                        ui.end_row();
                    }
                });
            if let Some((g, h)) = hide {
                p.set_hidden(w, g, h);
            }
            if commit {
                p.commit_rename(w);
            }
            if let Some(g) = click {
                let add = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                let mut parts: Vec<GroupId> = if add {
                    p.lines
                        .iter()
                        .filter(|l| l.selected)
                        .map(|l| l.part)
                        .collect()
                } else {
                    Vec::new()
                };
                parts.push(g);
                p.click(w, &parts);
            }
        }
        ui.add_enabled_ui(p.split_enabled, |ui| {
            if ui
                .button(tr("Split into Pieces", &[]))
                .on_hover_text(tr(
                    "Regroup the component by the solids that do not touch",
                    &[],
                ))
                .clicked()
            {
                p.split(w);
            }
        });
        if p.lines.len() > 1 {
            ui.label(egui::RichText::new(tr("Exploded view", &[])).strong());
            ui.horizontal(|ui| {
                let label = EXPLODE_MODES
                    .iter()
                    .find(|(k, _)| *k == p.explode_mode)
                    .map(|(_, l)| tr(l, &[]))
                    .unwrap_or_default();
                let mut picked = None;
                egui::ComboBox::from_id_salt("explode-mode")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (k, l) in EXPLODE_MODES {
                            if ui
                                .selectable_label(p.explode_mode == k, tr(l, &[]))
                                .clicked()
                            {
                                picked = Some(k);
                            }
                        }
                    });
                if let Some(k) = picked {
                    p.explode_mode_changed(w, k);
                }
                if ui
                    .add_enabled(
                        p.reassemble_enabled,
                        egui::Button::new(tr("Reassemble", &[])),
                    )
                    .clicked()
                {
                    p.reassemble(w);
                }
            });
            let mut v = p.explode_percent;
            let r = ui.add(egui::Slider::new(&mut v, 0..=300).suffix("%"));
            if r.drag_started() {
                p.explode_press(w);
            }
            if r.changed() {
                p.explode_value(w, v);
            }
            if r.drag_stopped() {
                p.explode_release(w);
            }
        }
    }

    /// The Styles drop-down panel.
    pub fn styles(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        let p = &mut tray.styles;
        let current = p
            .current
            .and_then(|i| p.combo.get(i).cloned().flatten())
            .map(|(l, _)| l)
            .unwrap_or_default();
        let mut picked = None;
        egui::ComboBox::from_id_salt("style-combo")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (i, e) in p.combo.iter().enumerate() {
                    match e {
                        Some((label, _)) => {
                            if ui.selectable_label(p.current == Some(i), label).clicked() {
                                picked = Some(i);
                            }
                        }
                        None => {
                            ui.separator();
                        }
                    }
                }
            });
        if let Some(i) = picked {
            p.pick(w, i);
        }
        let s = w.viewport.scene.display_style.clone();
        egui::Grid::new("style-grid").num_columns(2).show(ui, |ui| {
            ui.label(tr("Face mode:", &[]));
            let label = MODES
                .iter()
                .find(|(k, _)| *k == s.face_mode)
                .map(|(_, l)| tr(l, &[]))
                .unwrap_or_default();
            let mut mode = None;
            egui::ComboBox::from_id_salt("style-mode")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (k, l) in MODES {
                        if ui.selectable_label(s.face_mode == k, tr(l, &[])).clicked() {
                            mode = Some(k);
                        }
                    }
                });
            if let Some(m) = mode {
                p.edit(w, |st| st.face_mode = m.into());
            }
            ui.end_row();
            let mut flags = [s.edges, s.profiles, s.back_edges, s.sky, s.section_fill];
            let names = [
                "Edges",
                "Silhouette Edges",
                "Hidden Lines",
                "Sky",
                "Section Fill",
            ];
            for (i, n) in names.iter().enumerate() {
                if ui.checkbox(&mut flags[i], tr(n, &[])).changed() {
                    let f = flags;
                    p.edit(w, |st| {
                        st.edges = f[0];
                        st.profiles = f[1];
                        st.back_edges = f[2];
                        st.sky = f[3];
                        st.section_fill = f[4];
                    });
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
            ui.end_row();
        });
        let mut ask = None;
        ui.horizontal_wrapped(|ui| {
            for (attr, tip) in SWATCHES {
                let c = Styles::shown_color(w, attr);
                if swatch(ui, rgb32(c), &tr(tip, &[]), 24.0).clicked() {
                    ask = Some(TrayAction::StyleColor {
                        title: tr(tip, &[]),
                        attr: attr.to_string(),
                    });
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button(tr("Save style…", &[])).clicked() {
                ask = Some(TrayAction::SaveStyle);
            }
            if ui
                .add_enabled(p.delete_enabled, egui::Button::new(tr("Delete", &[])))
                .clicked()
            {
                ask = Some(TrayAction::DeleteStyle);
            }
        });
        if let Some(a) = ask {
            tray.run(w, a);
        }
    }

    /// The Dimension style drop-down panel.
    pub fn dimstyle(&mut self, ui: &mut egui::Ui, tray: &mut Tray, w: &mut MainWindow) {
        use crate::tray::dimstyle::{DimensionStyle, ENDS};
        let mut f = DimensionStyle::fields(w);
        let before = f.clone();
        egui::Grid::new("dim-grid").num_columns(2).show(ui, |ui| {
            ui.label(tr("Decimals:", &[]));
            ui.add(egui::DragValue::new(&mut f.decimals).range(0..=4));
            ui.end_row();
            ui.label(tr("Unit:", &[]));
            egui::ComboBox::from_id_salt("dim-units")
                .selected_text(f.units.clone())
                .show_ui(ui, |ui| {
                    for u in xtrude_model::units::UNIT_CHOICES {
                        ui.selectable_value(&mut f.units, u.to_string(), u);
                    }
                });
            ui.end_row();
            ui.label(tr("Font:", &[]));
            ui.add(egui::DragValue::new(&mut f.font_size).range(6..=28));
            ui.end_row();
            ui.label(tr("Color:", &[]));
            let c = f.color.map(|x| x as f64 / 255.0);
            if swatch(ui, rgb32(c), "", 22.0).clicked() {
                tray.run(w, TrayAction::DimensionColor);
            }
            ui.end_row();
            ui.label(tr("Ends:", &[]));
            let label = ENDS
                .iter()
                .find(|(k, _)| *k == f.ends)
                .map(|(_, l)| tr(l, &[]))
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("dim-ends")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (k, l) in ENDS {
                        ui.selectable_value(&mut f.ends, k.to_string(), tr(l, &[]));
                    }
                });
            ui.end_row();
            ui.label(tr("Baseline step:", &[]));
            ui.add(
                egui::DragValue::new(&mut f.base_step_mm)
                    .range(1.0..=60.0)
                    .speed(0.5)
                    .suffix(" mm"),
            );
            ui.end_row();
        });
        if f != before {
            DimensionStyle::apply(w, &f);
        }
    }
}

enum ModelTreeAction {
    Root,
    Click(GroupId, bool),
    Edit(GroupId),
    Rename(GroupId, String),
    Hide(Vec<GroupId>, bool),
    Drop(GroupId, Option<GroupId>),
}
