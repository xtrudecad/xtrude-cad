// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The toolbars on screen: one panel per docked line, the floating bars
//! and the grip drag (four docking areas around the viewport). Where each
//! bar sits is the window's [`crate::window::ToolbarLayout`]; this only draws it,
//! measures it and forwards the drop.

use super::{paint, XtrudeApp};
use crate::window::toolbar_layout::{DropTarget, LayoutGeometry, Rect};
use crate::window::toolbar_stacks::{self, STACK_PREFIX};
use crate::window::{ToolbarArea, ToolbarDef};
use xtrude_model::i18n::tr;

/// The grip's thickness along its bar.
const GRIP: f32 = 8.0;

/// The thickness of a stacked button's arrow strip along its bar.
const ARROW: f32 = 10.0;

/// A toolbar held by its grip.
pub(super) struct ToolbarDrag {
    id: String,
    /// Where the pointer holds the bar, from its top-left.
    grab: egui::Vec2,
    size: egui::Vec2,
}

fn to_rect(r: egui::Rect) -> Rect {
    Rect::new(r.min.x, r.min.y, r.max.x, r.max.y)
}

fn to_egui(r: &Rect) -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(r.min.0, r.min.1), egui::pos2(r.max.0, r.max.1))
}

impl XtrudeApp {
    /// The docked lines of `area`, outermost first, as panels of the
    /// `ui` being laid out; records the area's rectangle (zero thickness
    /// at its edge when empty) for the drop test. Declared in this order —
    /// top, status bar, bottom, left, right, then the trays — so the top
    /// and bottom lines span the window and the side columns sit outside
    /// the trays.
    pub(super) fn toolbar_area(&mut self, ui: &mut egui::Ui, area: ToolbarArea) {
        let before = ui.available_rect_before_wrap();
        let column = self.icon_px() + 14.0;
        for (i, bars) in self.window.toolbar_lines(area).into_iter().enumerate() {
            if bars.is_empty() {
                continue;
            }
            let id = egui::Id::new(("toolbar-line", area, i));
            let panel = match area {
                ToolbarArea::Top => egui::Panel::top(id),
                ToolbarArea::Bottom => egui::Panel::bottom(id),
                ToolbarArea::Left => egui::Panel::left(id).resizable(false).exact_size(column),
                ToolbarArea::Right => egui::Panel::right(id).resizable(false).exact_size(column),
            };
            let r = panel.show(ui, |ui| self.toolbar_line(ui, area, &bars));
            self.toolbar_geometry
                .lines
                .push((area, i, to_rect(r.response.rect)));
        }
        let after = ui.available_rect_before_wrap();
        let (l, t, r, b) = (before.left(), before.top(), before.right(), before.bottom());
        let rect = match area {
            ToolbarArea::Top => Rect::new(l, t, r, after.top()),
            ToolbarArea::Bottom => Rect::new(l, after.bottom(), r, b),
            ToolbarArea::Left => Rect::new(l, t, after.left(), b),
            ToolbarArea::Right => Rect::new(after.right(), t, r, b),
        };
        self.toolbar_geometry.areas.push((area, rect));
    }

    /// One docked line: its bars side by side (a top or bottom line wraps
    /// a bar that does not fit; a column clips it), separated by a rule.
    fn toolbar_line(&mut self, ui: &mut egui::Ui, area: ToolbarArea, bars: &[ToolbarDef]) {
        ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
        let vertical = area.vertical();
        let draw = |ui: &mut egui::Ui| {
            for (i, (id, _, keys)) in bars.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                let r = self.toolbar_bar(ui, id, keys, vertical);
                self.toolbar_geometry.bars.push((id.clone(), to_rect(r)));
            }
        };
        if vertical {
            ui.vertical(draw);
        } else {
            ui.horizontal_wrapped(draw);
        }
    }

    /// One bar: its grip, then its buttons; returns where it was drawn.
    fn toolbar_bar(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        keys: &[String],
        vertical: bool,
    ) -> egui::Rect {
        let px = self.icon_px();
        let n = keys.len() + if id == "panels" { 2 } else { 0 };
        let stacks = keys.iter().filter(|k| k.starts_with(STACK_PREFIX)).count();
        let length = GRIP + n as f32 * (px + 9.0) + stacks as f32 * (ARROW + 1.0);
        let (size, layout) = if vertical {
            (
                egui::vec2(px + 8.0, length),
                egui::Layout::top_down(egui::Align::Center),
            )
        } else {
            (
                egui::vec2(length, px + 8.0),
                egui::Layout::left_to_right(egui::Align::Center),
            )
        };
        let inner = ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
            let grip = self.grip(ui, id, vertical, px);
            if id == "panels" {
                self.panel_buttons(ui, vertical);
            }
            for k in keys {
                match k.strip_prefix(STACK_PREFIX) {
                    Some(stack) => self.stack_button(ui, stack, vertical, px),
                    None => self.tool_button(ui, k, px),
                }
            }
            grip
        });
        let bar = inner.response.rect;
        let grip = inner.inner;
        if grip.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(p) = origin.or(grip.interact_pointer_pos()) {
                self.toolbar_drag = Some(ToolbarDrag {
                    id: id.to_string(),
                    grab: p - bar.min,
                    size: bar.size(),
                });
            }
        }
        bar
    }

    /// The toolbar handle: two columns of dots at the start of a
    /// horizontal bar, two rows on top of a vertical one.
    fn grip(&mut self, ui: &mut egui::Ui, id: &str, vertical: bool, px: f32) -> egui::Response {
        let size = if vertical {
            egui::vec2(px + 8.0, GRIP)
        } else {
            egui::vec2(GRIP, px + 8.0)
        };
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let r = ui.interact(
            rect,
            egui::Id::new(("toolbar-grip", id)),
            egui::Sense::click_and_drag(),
        );
        let ink = ui.visuals().weak_text_color();
        let c = rect.center();
        let span = (if vertical {
            rect.width()
        } else {
            rect.height()
        }) - 12.0;
        let dots = (span / 4.0).floor().max(1.0) as i32;
        for k in 0..dots {
            let along = (k as f32 - (dots - 1) as f32 / 2.0) * 4.0;
            for across in [-2.0, 2.0] {
                let p = if vertical {
                    egui::pos2(c.x + along, c.y + across)
                } else {
                    egui::pos2(c.x + across, c.y + along)
                };
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(p, egui::vec2(2.0, 2.0)),
                    0.0,
                    ink,
                );
            }
        }
        let cursor = if self.toolbar_drag.is_some() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        };
        r.on_hover_cursor(cursor)
    }

    /// The floating bars, over the viewport `central`: horizontal, a grip
    /// and the buttons in a thin frame, no title. A bar
    /// the window has shrunk away from is shown inside it, where it stays
    /// saved until it is dragged.
    pub(super) fn floating_toolbars(&mut self, ctx: &egui::Context, central: egui::Rect) {
        for ((id, _, keys), pos) in self.window.floating_toolbars() {
            let area_id = egui::Id::new(("toolbar-float", &id));
            let size = ctx
                .memory(|m| m.area_rect(area_id))
                .map(|r| r.size())
                .unwrap_or_default();
            let x = pos.0.min(central.right() - size.x).max(central.left());
            let y = pos.1.min(central.bottom() - size.y).max(central.top());
            egui::Area::new(area_id)
                .order(egui::Order::Middle)
                .current_pos(egui::pos2(x, y))
                .movable(false)
                .constrain(false)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().panel_fill)
                        .stroke(ui.visuals().window_stroke)
                        .corner_radius(2)
                        .inner_margin(2)
                        .show(ui, |ui| {
                            self.toolbar_bar(ui, &id, &keys, false);
                        });
                });
        }
    }

    /// While a grip is held: the bar's outline under the pointer and where
    /// it would land; on release the window places it; Esc drops the drag.
    /// Runs before the viewport, so the Esc does not reach the tool.
    pub(super) fn toolbar_drag_frame(&mut self, ctx: &egui::Context) {
        let Some(drag) = &self.toolbar_drag else {
            return;
        };
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.toolbar_drag = None;
            return;
        }
        let Some(p) = ctx.pointer_latest_pos() else {
            return;
        };
        let target = self.window.toolbar_layout.drop_target(
            &self.toolbar_geometry,
            (p.x, p.y),
            (drag.grab.x, drag.grab.y),
        );
        if !ctx.input(|i| i.pointer.primary_down()) {
            let id = drag.id.clone();
            self.toolbar_drag = None;
            self.window.place_toolbar(&id, target);
            return;
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("toolbar-drag"),
        ));
        let accent = ctx.global_style().visuals.selection.bg_fill;
        let stroke = egui::Stroke::new(2.0, accent);
        painter.rect_stroke(
            egui::Rect::from_min_size(p - drag.grab, drag.size),
            2.0,
            stroke,
            egui::StrokeKind::Inside,
        );
        if let Some(marker) = self.drop_marker(&target) {
            painter.rect(
                marker,
                0.0,
                accent.gamma_multiply(0.35),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
    }

    /// Where a drop lands, as the shell shows it: a bar across the line
    /// at the insertion point, or the band of the new line.
    fn drop_marker(&self, target: &DropTarget) -> Option<egui::Rect> {
        let geo: &LayoutGeometry = &self.toolbar_geometry;
        match *target {
            DropTarget::Into(spot) => {
                let line = geo
                    .lines
                    .iter()
                    .find(|(a, l, _)| *a == spot.area && *l == spot.line)
                    .map(|(_, _, r)| to_egui(r))?;
                let ids = self.window.toolbar_layout.lines[spot.area.index()].get(spot.line)?;
                let vertical = spot.area.vertical();
                let mut drawn: Vec<(usize, egui::Rect)> = geo
                    .bars
                    .iter()
                    .filter_map(|(id, r)| ids.iter().position(|b| b == id).map(|i| (i, to_egui(r))))
                    .collect();
                drawn.sort_by_key(|(i, _)| *i);
                let along = match drawn.iter().find(|(i, _)| *i >= spot.index) {
                    Some((_, r)) if vertical => r.top(),
                    Some((_, r)) => r.left(),
                    None => match drawn.last() {
                        Some((_, r)) if vertical => r.bottom(),
                        Some((_, r)) => r.right(),
                        None if vertical => line.top(),
                        None => line.left(),
                    },
                };
                // a wrapped top line: the marker spans the row of the bar
                let row = drawn
                    .iter()
                    .find(|(i, _)| *i >= spot.index)
                    .or(drawn.last())
                    .map(|(_, r)| *r)
                    .unwrap_or(line);
                Some(if vertical {
                    egui::Rect::from_min_max(
                        egui::pos2(line.left() + 2.0, along - 2.0),
                        egui::pos2(line.right() - 2.0, along + 2.0),
                    )
                } else {
                    egui::Rect::from_min_max(
                        egui::pos2(along - 2.0, row.top()),
                        egui::pos2(along + 2.0, row.bottom()),
                    )
                })
            }
            DropTarget::NewLine { area, .. } => {
                let r = geo
                    .areas
                    .iter()
                    .find(|(a, _)| *a == area)
                    .map(|(_, r)| to_egui(r))?;
                let t = self.icon_px() + 14.0;
                Some(match area {
                    ToolbarArea::Top => egui::Rect::from_min_max(
                        egui::pos2(r.left(), r.bottom()),
                        egui::pos2(r.right(), r.bottom() + t),
                    ),
                    ToolbarArea::Bottom => egui::Rect::from_min_max(
                        egui::pos2(r.left(), r.top() - t),
                        egui::pos2(r.right(), r.top()),
                    ),
                    ToolbarArea::Left => egui::Rect::from_min_max(
                        egui::pos2(r.right(), r.top()),
                        egui::pos2(r.right() + t, r.bottom()),
                    ),
                    ToolbarArea::Right => egui::Rect::from_min_max(
                        egui::pos2(r.left() - t, r.top()),
                        egui::pos2(r.left(), r.bottom()),
                    ),
                })
            }
            DropTarget::Float { .. } => None,
        }
    }

    pub(super) fn tool_button(&mut self, ui: &mut egui::Ui, k: &str, px: f32) {
        let Some(a) = self.window.actions.get(k) else {
            return;
        };
        let (text, tip, checked, enabled, keys_txt) = (
            a.text.replace('&', ""),
            a.status_tip.clone(),
            a.checked,
            a.enabled,
            a.shortcut_text(),
        );
        let size = egui::vec2(px + 8.0, px + 8.0);
        let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
        let visuals = ui.style().interact_selectable(&r, checked);
        if checked || r.hovered() {
            ui.painter().rect(
                rect,
                4.0,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let ink = if enabled {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let key = self.icon_key(k);
        let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(px, px));
        match self.icon(ui.ctx(), &key, ink) {
            Some((tex, texts)) => {
                ui.painter().image(
                    tex,
                    inner,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                paint::icon_texts(ui.painter(), inner, &texts);
            }
            None => {
                let short: String = text.chars().take(2).collect();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    short,
                    egui::FontId::proportional(11.0),
                    ink,
                );
            }
        }
        let tip_text = if keys_txt.is_empty() {
            format!("{text}\n{tip}")
        } else {
            format!("{text}  ({keys_txt})\n{tip}")
        };
        let r = r.on_hover_text(tip_text);
        if r.hovered() && !tip.is_empty() {
            self.menu_tip = Some(tip);
        }
        if r.clicked() && enabled {
            self.trigger(k);
        }
    }

    /// A stacked button: its current member's button, then an arrow strip
    /// (below it in a column) that lists every member: icon, name and
    /// shortcut, the current one selected. The list opens below the
    /// button in a row and beside it in a column; a click outside or Esc
    /// closes it.
    fn stack_button(&mut self, ui: &mut egui::Ui, stack: &str, vertical: bool, px: f32) {
        let (Some(current), Some(members)) = (
            self.window.stack_current(stack).map(str::to_string),
            toolbar_stacks::members(stack),
        ) else {
            return;
        };
        self.tool_button(ui, &current, px);
        let size = if vertical {
            egui::vec2(px + 8.0, ARROW)
        } else {
            egui::vec2(ARROW, px + 8.0)
        };
        let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
        let popup_id = egui::Popup::default_response_id(&r);
        let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let visuals = ui.style().interact_selectable(&r, open);
        if open || r.hovered() {
            ui.painter().rect(
                rect,
                2.0,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        // a small triangle pointing where the list opens
        let c = rect.center();
        let tip = if vertical {
            vec![
                egui::pos2(c.x - 2.0, c.y - 3.0),
                egui::pos2(c.x + 2.0, c.y),
                egui::pos2(c.x - 2.0, c.y + 3.0),
            ]
        } else {
            vec![
                egui::pos2(c.x - 3.0, c.y - 2.0),
                egui::pos2(c.x + 3.0, c.y - 2.0),
                egui::pos2(c.x, c.y + 2.0),
            ]
        };
        ui.painter().add(egui::Shape::convex_polygon(
            tip,
            ui.visuals().text_color(),
            egui::Stroke::NONE,
        ));
        let title = tr(toolbar_stacks::title(stack).unwrap_or(stack), &[]);
        let r = r.on_hover_text(title);
        let align = if vertical {
            egui::RectAlign::RIGHT_START
        } else {
            egui::RectAlign::BOTTOM_START
        };
        egui::Popup::menu(&r).align(align).show(|ui| {
            for &m in members {
                self.stack_entry(ui, stack, m, m == current);
            }
        });
    }

    /// One row of a stack's list: the member's icon, name and shortcut.
    fn stack_entry(&mut self, ui: &mut egui::Ui, stack: &str, action: &str, current: bool) {
        let Some(a) = self.window.actions.get(action) else {
            return;
        };
        let (text, tip, enabled, keys_txt) = (
            a.text.replace('&', ""),
            a.status_tip.clone(),
            a.enabled,
            a.shortcut_text(),
        );
        let ink = if enabled {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let key = self.icon_key(action);
        let side = ui.text_style_height(&egui::TextStyle::Button) + 6.0;
        let mut b = match self.icon(ui.ctx(), &key, ink) {
            Some((tex, _)) => {
                egui::Button::image_and_text(egui::Image::new((tex, egui::vec2(side, side))), text)
            }
            None => egui::Button::new(text),
        }
        .selected(current);
        if !keys_txt.is_empty() {
            b = b.shortcut_text(keys_txt);
        }
        let r = ui.add_enabled(enabled, b);
        if r.hovered() && !tip.is_empty() {
            self.menu_tip = Some(tip);
        }
        if r.clicked() {
            ui.close();
            self.window.pick_in_stack(stack, action);
            self.window.pump_events();
        }
    }

    /// The Styles / Dimension style drop-downs (the Panels toolbar); in a
    /// column they open beside the button, not below it.
    fn panel_buttons(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let px = self.icon_px();
        for (key, title) in [("styles", "Styles"), ("dimension_style", "Dimension style")] {
            let size = egui::vec2(px + 8.0, px + 8.0);
            let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
            let ink = ui.visuals().text_color();
            if let Some((tex, _)) = self.icon(ui.ctx(), key, ink) {
                ui.painter().image(
                    tex,
                    egui::Rect::from_center_size(rect.center(), egui::vec2(px, px)),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            let r = r.on_hover_text(tr(title, &[]));
            if r.clicked() {
                self.open_panel = if self.open_panel == Some(key) {
                    None
                } else {
                    Some(key)
                };
                self.tray.styles.refresh(&self.window);
            }
            if self.open_panel == Some(key) {
                let pos = if vertical {
                    r.rect.right_top()
                } else {
                    r.rect.left_bottom()
                };
                let mut open = true;
                egui::Window::new(tr(title, &[]))
                    .id(egui::Id::new(("panel", key)))
                    .default_pos(pos)
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .show(ui.ctx(), |ui| {
                        if key == "styles" {
                            self.tray_ui.styles(ui, &mut self.tray, &mut self.window);
                        } else {
                            self.tray_ui.dimstyle(ui, &mut self.tray, &mut self.window);
                        }
                    });
                // a dialog opened from the panel closes the drop-down first
                if !open || self.dialogs.borrow().pending.is_some() {
                    self.open_panel = None;
                }
            }
        }
    }
}
