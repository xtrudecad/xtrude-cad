// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The application window on screen: eframe/egui around the toolkit-neutral
//! [`MainWindow`]. Menus and toolbars come from the window's action list; the
//! viewport is the wgpu frame shown as an image with the 2D overlay painted
//! over it; the status bar carries the tool's hint and the value box.

mod clipboard;
pub mod dialogs;
pub mod frame_stats;
pub mod icons;
pub mod input;
pub mod menu_bar;
pub mod menu_item;
mod model_info_ui;
mod paint;
pub mod prefs_dialog;
mod prefs_ui;
mod question_frame;
mod solid_inspector_ui;
mod toolbars;
mod tray_ui;

pub use prefs_ui::visuals;

use crate::window::status_bar::StatusBar;
use crate::window::toolbar_layout::LayoutGeometry;
use crate::window::ToolbarArea;
use std::cell::RefCell;
use std::rc::Rc;

use xtrude_render::gpu::ViewportRenderer;

use crate::viewport::Cursor;
use crate::window::actions::MenuItem;
use crate::window::files::recent_menu;
use crate::window::{CtxItem, MainWindow};
use dialogs::{Answer, Question, ShellDialogs};
use input::{Input, InputState};
use xtrude_ext::{ParamKind, ParamSpec, ParamValue};

/// The text fields of the question on screen.
#[derive(Default)]
struct Form {
    text: String,
    number: f64,
    index: usize,
    index2: usize,
    flag: bool,
    open: bool,
    color: [f32; 3],
    /// A form's values (check boxes and choices are edited here) and the
    /// text of each entry.
    values: Vec<ParamValue>,
    texts: Vec<String>,
}

impl Form {
    /// A form's values read from its entries, or the first error.
    fn parsed(&self, fields: &[ParamSpec]) -> Result<Vec<ParamValue>, String> {
        fields
            .iter()
            .zip(self.texts.iter().zip(&self.values))
            .map(|(spec, (text, value))| crate::window::form::parse_field(spec, text, value))
            .collect()
    }

    /// The answer to `q` when its OK (or Yes) is pressed; `None` for the
    /// unsaved-changes question, whose buttons each answer it.
    fn answer(&self, q: &Question) -> Option<Answer> {
        Some(match q {
            Question::Text { .. } => Answer::Text(Some(self.text.clone())),
            Question::Number { .. } => Answer::Number(Some(self.number)),
            Question::Unsaved { .. } => return None,
            Question::Confirm { .. } => Answer::Bool(true),
            Question::Choose { .. } => Answer::Index(Some(self.index)),
            Question::Stl { .. } => Answer::Stl(Some((self.index, self.index2))),
            Question::Text3d(p) => {
                let mut np = p.clone();
                if !self.text.is_empty() {
                    np.text = self.text.clone();
                }
                Answer::Text3d(Some(np))
            }
            Question::Color { .. } => Answer::Color(Some(self.color.map(|c| c as f64))),
            Question::SectionName { symbol, .. } => {
                Answer::Section(Some(crate::window::dialogs::SectionName {
                    name: self.text.clone(),
                    symbol: symbol.clone(),
                    dont_ask: self.flag,
                }))
            }
            // not while an entry is wrong
            Question::Form { fields, .. } => Answer::Form(Some(self.parsed(fields).ok()?)),
        })
    }
}

/// Rendered icons as textures, by key, ink and size.
#[derive(Default)]
struct IconCache {
    textures:
        std::collections::HashMap<(String, u32, u32), (egui::TextureHandle, Vec<icons::IconText>)>,
}

impl IconCache {
    /// The texture of icon `key` for `ink`, rendered at `px` logical
    /// pixels: an icon drawn much smaller than its 48-px canvas (on a line
    /// of text) stays sharp only when rendered at its own size.
    fn get(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        ink: egui::Color32,
        px: f32,
    ) -> Option<(egui::TextureId, Vec<icons::IconText>)> {
        let cache_key = (
            key.to_string(),
            u32::from_be_bytes(ink.to_array()),
            px.to_bits(),
        );
        if let Some((t, texts)) = self.textures.get(&cache_key) {
            return Some((t.id(), texts.clone()));
        }
        let ppp = ctx.pixels_per_point().max(1.0);
        let scale = if px == icons::PX {
            ppp
        } else {
            (px * ppp).round().max(1.0) / icons::PX
        };
        let [r, g, b, a] = ink.to_array();
        let img = icons::render(key, scale, icons::Rgba(r, g, b, a))?;
        let color = egui::ColorImage::from_rgba_premultiplied(
            [img.size as usize, img.size as usize],
            &img.rgba,
        );
        let tex = ctx.load_texture(format!("icon:{key}"), color, egui::TextureOptions::LINEAR);
        let id = tex.id();
        self.textures.insert(cache_key, (tex, img.texts.clone()));
        Some((id, img.texts))
    }
}

pub struct XtrudeApp {
    pub window: MainWindow,
    /// Whether a frame has been drawn yet.
    painted: bool,
    dialogs: Rc<RefCell<ShellDialogs>>,
    input: InputState,
    renderer: Option<ViewportRenderer>,
    texture: Option<egui::TextureId>,
    form: Form,
    status: StatusBar,
    menu_tip: Option<String>,
    last_title: String,
    icons: IconCache,
    /// The tools' OS cursors by name, ink and pixels per point; the same
    /// `Arc` every frame, so the OS cursor is made once.
    cursors: std::collections::HashMap<(String, u32, u32), Option<egui::CustomCursorImage>>,
    tray: crate::tray::Tray,
    tray_ui: tray_ui::TrayUi,
    tray_key: Option<(u64, usize, bool)>,
    open_panel: Option<&'static str>,
    prefs: Option<prefs_dialog::PrefsState>,
    about: bool,
    /// The About box's lockup and whether it is the dark one.
    about_logo: Option<(bool, egui::TextureHandle)>,
    model_info: Option<model_info_ui::ModelInfoState>,
    theme_dark: Option<bool>,
    /// The palette in force, set with `theme_dark`.
    palette: crate::theme::Palette,
    msaa: u32,
    /// The toolbar areas, lines and bars as drawn this frame.
    toolbar_geometry: LayoutGeometry,
    toolbar_drag: Option<toolbars::ToolbarDrag>,
    /// Whether the OS window had focus last frame.
    focused: bool,
    /// Frame times, under `XTRUDE_FRAME_STATS` ([`frame_stats`]).
    frame_stats: Option<frame_stats::FrameStats>,
}

impl XtrudeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mut window: MainWindow) -> XtrudeApp {
        let dialogs = Rc::new(RefCell::new(ShellDialogs::default()));
        window.dialogs = Box::new(dialogs.clone());
        window.system_clipboard = Box::new(clipboard::system_clipboard());
        // imports parse while the frames go on, under a progress dialog
        window.background_imports = true;
        let renderer = cc.wgpu_render_state.as_ref().map(|rs| {
            ViewportRenderer::new(
                &rs.device,
                supported_samples(rs, window.viewport.prefs.msaa),
            )
        });
        let msaa = window.viewport.prefs.msaa;
        XtrudeApp {
            window,
            painted: false,
            dialogs,
            input: InputState::default(),
            renderer,
            texture: None,
            form: Form::default(),
            status: StatusBar::default(),
            menu_tip: None,
            last_title: String::new(),
            icons: Default::default(),
            cursors: Default::default(),
            tray: Default::default(),
            tray_ui: Default::default(),
            tray_key: None,
            open_panel: None,
            prefs: None,
            about: false,
            about_logo: None,
            model_info: None,
            theme_dark: None,
            palette: crate::theme::dark_palette(),
            msaa,
            toolbar_geometry: LayoutGeometry::default(),
            toolbar_drag: None,
            focused: true,
            frame_stats: frame_stats::enabled_by(
                std::env::var(frame_stats::ENV_SWITCH).ok().as_deref(),
            )
            .then(frame_stats::FrameStats::default),
        }
    }

    /// Record this frame's times, when frame statistics are on.
    fn record_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(stats) = &mut self.frame_stats else {
            return;
        };
        if stats.backend.is_none() {
            use wgpu::rwh::HasDisplayHandle;
            stats.backend = frame
                .display_handle()
                .ok()
                .map(|h| frame_stats::backend_name(&h.as_raw()));
        }
        let (interactive, scale) = ctx.input(|i| {
            (
                !i.events.is_empty(),
                i.viewport().native_pixels_per_point.unwrap_or(1.0),
            )
        });
        stats.frame(frame_stats::FrameSample {
            start: std::time::Instant::now(),
            interactive,
            cpu_seconds: frame.info().cpu_usage,
            scale,
        });
    }

    fn trigger(&mut self, id: &str) {
        match id {
            "preferences" => return self.open_preferences(),
            "about" => return self.open_about(),
            _ => {}
        }
        self.window.trigger(id);
        self.window.pump_events();
    }

    // ---- menus ---------------------------------------------------------------------------
    fn action_button(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(a) = self.window.actions.get(id) else {
            return;
        };
        let (text, enabled, checkable, checked, tip, keys) = (
            a.text.replace('&', ""),
            a.enabled,
            a.checkable,
            a.checked,
            a.status_tip.clone(),
            a.shortcut_text(),
        );
        let mut b = menu_item::entry(ui, &text, checkable && checked);
        if !keys.is_empty() {
            b = b.shortcut_text(keys);
        }
        let r = ui.add_enabled(enabled, b);
        if r.hovered() && !tip.is_empty() {
            self.menu_tip = Some(tip.clone());
        }
        if r.clicked() {
            ui.close();
            self.trigger(id);
        }
    }

    fn menu_items(&mut self, ui: &mut egui::Ui, items: &[MenuItem]) {
        for item in items {
            match item {
                MenuItem::Separator => {
                    ui.separator();
                }
                MenuItem::Action(id) => self.action_button(ui, id),
                MenuItem::Menu(title, sub) => {
                    let sub = sub.clone();
                    menu_item::submenu(ui, &title.replace('&', ""), |ui| self.menu_items(ui, &sub));
                }
                MenuItem::Dynamic("recent") => {
                    let items = recent_menu();
                    self.ctx_items(ui, &items);
                }
                MenuItem::Dynamic("panels") => {
                    let items = self.window.panels_menu();
                    self.ctx_items(ui, &items);
                }
                MenuItem::Dynamic("toolbars") => {
                    let items = self.window.toolbars_menu();
                    self.ctx_items(ui, &items);
                }
                MenuItem::Dynamic(_) => {}
            }
        }
    }

    fn ctx_items(&mut self, ui: &mut egui::Ui, items: &[CtxItem]) {
        for item in items {
            match item {
                CtxItem::Separator => {
                    ui.separator();
                }
                CtxItem::Entry {
                    id,
                    text,
                    enabled,
                    checked,
                } => {
                    let button =
                        menu_item::entry(ui, &text.replace('&', ""), *checked == Some(true));
                    if ui.add_enabled(*enabled, button).clicked() {
                        ui.close();
                        let id = id.clone();
                        self.trigger(&id);
                    }
                }
                CtxItem::Menu(title, sub) => {
                    let sub = sub.clone();
                    menu_item::submenu(ui, &title.replace('&', ""), |ui| self.ctx_items(ui, &sub));
                }
            }
        }
    }

    // ---- toolbars ------------------------------------------------------------------------
    /// The icon key of an action (its own, or the tool / view it names).
    fn icon_key(&self, id: &str) -> String {
        if let Some(i) = self.window.actions.get(id).and_then(|a| a.icon.clone()) {
            return i;
        }
        if let Some(k) = id.strip_prefix("nav:") {
            return k.to_string();
        }
        if let Some(v) = id.strip_prefix("view:") {
            return format!("view_{v}");
        }
        id.to_string()
    }

    /// The icon texture of `key` for `ink`, rendered at its full 48 logical
    /// pixels.
    fn icon(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        ink: egui::Color32,
    ) -> Option<(egui::TextureId, Vec<icons::IconText>)> {
        self.icons.get(ctx, key, ink, icons::PX)
    }

    /// The tool's pointer as the OS cursor (`icons::os_cursor`), or `None`
    /// for a stock cursor, cached under the key with `+` for the guide-mode
    /// plus.
    fn tool_cursor(&mut self, ui: &egui::Ui, cursor: Cursor) -> Option<egui::CustomCursorImage> {
        let (key, plus) = (cursor.key(), cursor.plus());
        let name = if plus {
            format!("{key}+")
        } else {
            key.to_string()
        };
        let ink = ui.visuals().text_color();
        let ppp = ui.ctx().pixels_per_point().max(1.0);
        let cache_key = (name, u32::from_be_bytes(ink.to_array()), ppp.to_bits());
        self.cursors
            .entry(cache_key)
            .or_insert_with(|| {
                let [r, g, b, a] = ink.to_array();
                icons::os_cursor(key, plus, icons::Rgba(r, g, b, a), ppp).map(|c| {
                    egui::CustomCursorImage {
                        rgba: c.rgba.into(),
                        size: c.size,
                        hotspot: c.hotspot,
                    }
                })
            })
            .clone()
    }

    /// A small «Exit clean screen» button floating at the viewport's
    /// top-right corner while everything else is hidden — the way out for
    /// whoever does not know Ctrl+0.
    fn clean_screen_exit(&mut self, ctx: &egui::Context, vp: egui::Rect) {
        let text = format!("×  {}", xtrude_model::i18n::tr("Exit clean screen", &[]));
        let font = egui::FontId::proportional(13.0);
        let w = ctx.fonts_mut(|f| {
            f.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        }) + 20.0;
        let pos = egui::pos2(vp.right() - w - 12.0, vp.top() + 12.0);
        egui::Area::new(egui::Id::new("clean_screen_exit"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
                let fill = if r.hovered() {
                    egui::Color32::from_rgba_unmultiplied(54, 137, 230, 220)
                } else {
                    egui::Color32::from_rgba_unmultiplied(30, 36, 44, 170)
                };
                ui.painter().rect(
                    rect,
                    6.0,
                    fill,
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 90),
                    ),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &text,
                    egui::FontId::new(13.0, egui::FontFamily::Proportional),
                    egui::Color32::WHITE,
                );
                let r = r
                    .on_hover_text(xtrude_model::i18n::tr(
                        "Back to the workspace (Ctrl+0)",
                        &[],
                    ))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if r.clicked() {
                    self.trigger("clean_screen");
                }
            });
    }

    /// LibreOffice's handle: a slim button on the line between the viewport
    /// and the trays, half-way down; it folds the trays away (resting at the
    /// window's edge) and brings them back.
    fn sidebar_handle(&mut self, ctx: &egui::Context, vp: egui::Rect) {
        let on = self.window.sidebar_visible();
        let (w, h) = (14.0, 56.0);
        let x = (vp.right() - w / 2.0)
            .min(ctx.content_rect().right() - w)
            .max(0.0);
        let pos = egui::pos2(x, vp.center().y - h / 2.0);
        let key = if on { "side_collapse" } else { "side_expand" };
        let ink = ctx.global_style().visuals.text_color();
        let icon = self.icon(ctx, key, ink);
        let tip = format!(
            "{} (Ctrl+F5)",
            xtrude_model::i18n::tr("Show or hide the sidebar", &[])
        );
        egui::Area::new(egui::Id::new("sidebar_handle"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
                let fill = if r.hovered() {
                    egui::Color32::from_rgb(54, 137, 230)
                } else {
                    ui.visuals().widgets.noninteractive.bg_stroke.color
                };
                ui.painter().rect_filled(rect, 4.0, fill);
                if let Some((tex, _)) = icon {
                    ui.painter().image(
                        tex,
                        egui::Rect::from_center_size(rect.center(), egui::vec2(12.0, 12.0)),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                let r = r
                    .on_hover_text(tip)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if r.clicked() {
                    self.window.set_sidebar_visible(!on);
                }
            });
    }

    /// The right-hand tray, refreshed when the document or selection moved.
    fn side_tray(&mut self, ui: &mut egui::Ui) {
        let shown = self.window.sidebar_visible() && self.window.tray_shown("tray_properties");
        let clean = self
            .window
            .actions
            .get("clean_screen")
            .is_some_and(|a| a.checked);
        if !shown || clean {
            return;
        }
        let key = (
            self.window.viewport.scene.version,
            self.window.viewport.scene.edit_stack.len(),
            self.window.is_dirty(),
        );
        if self.tray_key != Some(key) {
            self.tray_key = Some(key);
            self.tray.refresh(&self.window);
        }
        egui::Panel::right("tray")
            .resizable(true)
            .default_size(300.0)
            .min_size(240.0)
            .show(ui, |ui| {
                self.tray_ui.show(ui, &mut self.tray, &mut self.window);
            });
    }

    /// Preferences ▸ General ▸ toolbar icon size (24 px by default).
    fn icon_px(&self) -> f32 {
        crate::window::preferences::toolbar_icon_px() as f32
    }

    // ---- status bar ----------------------------------------------------------------------
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        self.status.set_standing(&self.window.status_hint());
        if let Some(m) = self.window.status_message.take() {
            // a zero timeout is a «sticky» flash: it holds for an hour
            let ms = if m.msec == 0 { 3_600_000 } else { m.msec };
            self.status.show_message(&m.text, ms, now);
        }
        self.status.tick(now);
        if let Some(s) = self.status.next_change(now) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(s));
        }
        self.status.show_tip(self.menu_tip.take().as_deref());
        let text = self.status.current_message().to_string();
        let max_w = StatusBar::message_max_width(ui.available_width() as f64) as f32;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((caption, value, typing)) = self.window.value_box() {
                    let rgb = |c: [u8; 3]| egui::Color32::from_rgb(c[0], c[1], c[2]);
                    let (fill, ink) = if typing {
                        (rgb(self.palette.typing_fill), rgb(self.palette.typing_text))
                    } else {
                        (ui.visuals().extreme_bg_color, ui.visuals().text_color())
                    };
                    egui::Frame::new()
                        .fill(fill)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.set_min_width(130.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(egui::RichText::new(value).monospace().color(ink));
                                },
                            );
                        });
                    ui.label(egui::RichText::new(caption).weak());
                }
                // the repeat hint: left of the value box, muted
                if let Some(hint) = self.window.repeat_hint() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(hint).weak());
                    ui.add_space(8.0);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_max_width(max_w);
                    let r = ui.add(egui::Label::new(text.as_str()).truncate());
                    if r.hovered() {
                        // cut short, the whole line is one hover away
                        r.on_hover_text(text.as_str());
                    }
                });
            });
        });
    }

    // ---- the viewport --------------------------------------------------------------------
    fn viewport(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = (rect.width().max(1.0), rect.height().max(1.0));
        let vp = &mut self.window.viewport;
        if (vp.width() - w as f64).abs() > 0.5 || (vp.height() - h as f64).abs() > 0.5 {
            vp.resize(w as f64, h as f64);
            vp.camera.set_aspect(w as i32, h as i32);
        }
        // input
        // a question or a running import holds the input (both are modal)
        let blocked = self.dialogs.borrow().pending.is_some() || self.window.import_job.is_some();
        let keyboard = !ui.ctx().egui_wants_keyboard_input() && !blocked;
        let (events, mods, time, hover) =
            ui.input(|i| (i.events.clone(), i.modifiers, i.time, i.pointer.hover_pos()));
        // not under a floating toolbar, a menu or another egui area
        let ctx = ui.ctx().clone();
        let inside = |p: egui::Pos2| {
            rect.contains(p)
                && ctx
                    .layer_id_at(p)
                    .is_none_or(|l| l.order == egui::Order::Background)
        };
        // the timers due by now fire before this frame's input
        let now = std::time::Instant::now();
        self.window.tick(now);
        let inputs = if blocked {
            Vec::new()
        } else {
            let frame_input = input::FrameInput {
                events: &events,
                mods,
                time_ms: (time * 1000.0) as u64,
                hover_pos: hover,
            };
            crate::input::coalesce_hovers(self.input.translate(
                &frame_input,
                rect.min,
                &inside,
                keyboard,
            ))
        };
        for ev in inputs {
            // the zoom and the orbit pivot read what the frame on screen shows
            if self.window.viewport.wants_depth_for(&ev) {
                self.read_depth(frame);
            }
            match ev {
                Input::Press(e) => self.window.mouse_press(&e),
                Input::DoubleClick(e) => self.window.mouse_double_click(&e),
                Input::Move(e) => self.window.mouse_move(&e),
                Input::Release(e) => self.window.mouse_release(&e),
                Input::Wheel { x, y, delta } => self.window.wheel(x, y, delta),
                Input::KeyPress(k) => self.window.key_press(&k),
                Input::KeyRelease(k) => self.window.key_release(&k),
            }
        }
        // a frame when the next timer is due (a hover may have started one)
        if let Some(due) = self.window.next_tick(std::time::Instant::now()) {
            ui.ctx().request_repaint_after(due);
        }
        // the frame
        let size = [(w * ppp).round() as u32, (h * ppp).round() as u32];
        if let Some(rs) = frame.wgpu_render_state() {
            if self.msaa != self.window.viewport.prefs.msaa {
                self.msaa = self.window.viewport.prefs.msaa;
                self.renderer = Some(ViewportRenderer::new(
                    &rs.device,
                    supported_samples(rs, self.msaa),
                ));
                self.window.viewport.forget_frame_on_screen();
            }
        }
        // the scene is rendered again only when its frame changed; the
        // texture keeps the last one
        if let (Some(rs), Some(renderer)) = (frame.wgpu_render_state(), self.renderer.as_mut()) {
            if let Some(data) = self.window.viewport.changed_frame(size) {
                renderer.render(&rs.device, &rs.queue, &data);
                if self.window.viewport.annotations_need_depth() {
                    if let Some((w, h, depth)) = renderer.read_depth(&rs.device, &rs.queue) {
                        self.window.viewport.set_depth_snap(depth, w, h);
                    }
                }
                if let Some(view) = renderer.color_view() {
                    let mut egui_r = rs.renderer.write();
                    match self.texture {
                        Some(id) => egui_r.update_egui_texture_from_wgpu_texture(
                            &rs.device,
                            view,
                            wgpu::FilterMode::Nearest,
                            id,
                        ),
                        None => {
                            self.texture = Some(egui_r.register_native_texture(
                                &rs.device,
                                view,
                                wgpu::FilterMode::Nearest,
                            ));
                        }
                    }
                }
            }
        }
        if let Some(id) = self.texture {
            ui.painter().image(
                id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let overlay = self.window.viewport.overlay();
        paint::overlay(&ui.painter_at(rect), rect.min, &overlay);
        // the pointer: the OS moves a tool's bitmap cursor without waiting
        // for a frame (the image is reset each frame in `ui`)
        if response.hovered() {
            match viewport_pointer(self.window.viewport.outbox.cursor) {
                ViewportPointer::Image(c) => match self.tool_cursor(ui, c) {
                    Some(image) => ui.ctx().set_cursor_image(Some(image)),
                    None => ui.ctx().set_cursor_icon(cursor_icon(Some(c))),
                },
                ViewportPointer::Icon(icon) => ui.ctx().set_cursor_icon(icon),
            }
        }
        // the right-click menu, when the viewport took the right press
        // for a click and not for a camera drag
        let open = if self.window.take_context_menu_request() {
            Some(egui::SetOpenCommand::Bool(true))
        } else if response.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        egui::Popup::menu(&response)
            .open_memory(open)
            .at_pointer_fixed()
            .show(|ui| {
                let items = self.window.context_menu();
                self.ctx_items(ui, &items);
            });
    }

    // ---- the running import ---------------------------------------------------------------
    /// Poll the import parsing on its worker; while it runs, the modal
    /// progress dialog, on screen once the import has
    /// run 400 ms. Before that a bare modal already holds the input, so
    /// nothing edits the document under the parse.
    /// Hand the depth of the frame on screen to the viewport.
    fn read_depth(&mut self, frame: &eframe::Frame) {
        if let (Some(rs), Some(renderer)) = (frame.wgpu_render_state(), self.renderer.as_mut()) {
            if let Some((w, h, depth)) = renderer.read_depth(&rs.device, &rs.queue) {
                self.window.viewport.set_depth_snap(depth, w, h);
            }
        }
    }

    fn import_progress_dialog(&mut self, ctx: &egui::Context) {
        if self.window.import_job.is_none() {
            return;
        }
        if self.window.poll_import().is_some() {
            ctx.request_repaint();
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        let Some(progress) = &self.window.progress else {
            return;
        };
        let can_cancel = self
            .window
            .import_job
            .as_ref()
            .is_some_and(|j| j.can_cancel() && !j.is_cancelled());
        // window-modal: the window is held, not dimmed
        let modal = egui::Modal::new(egui::Id::new("import-progress"))
            .backdrop_color(egui::Color32::TRANSPARENT);
        let mut cancel = false;
        if progress.is_shown() {
            modal.show(ctx, |ui| {
                ui.set_width(300.0);
                ui.vertical_centered(|ui| ui.label(&progress.label));
                let fraction = progress.value.clamp(0, 100) as f32 / 100.0;
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                if can_cancel {
                    ui.vertical_centered(|ui| {
                        cancel = ui.button(xtrude_model::i18n::tr("Cancel", &[])).clicked();
                    });
                }
            });
        } else {
            modal.frame(egui::Frame::NONE).show(ctx, |_| {});
        }
        if cancel {
            self.window.cancel_import();
        }
    }

    // ---- questions -----------------------------------------------------------------------
    /// A form's entries, one row each, and the first error under them.
    fn form_fields(&mut self, ui: &mut egui::Ui, fields: &[ParamSpec]) {
        egui::Grid::new("form-fields")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                for (i, spec) in fields.iter().enumerate() {
                    ui.label(&spec.label);
                    match (&spec.kind, self.form.values.get_mut(i)) {
                        (ParamKind::Bool, Some(ParamValue::Bool(on))) => {
                            ui.checkbox(on, "");
                        }
                        (ParamKind::Choice(items), Some(ParamValue::Choice(c))) => {
                            let shown = items.get(*c).cloned().unwrap_or_default();
                            egui::ComboBox::from_id_salt(("form-choice", i))
                                .selected_text(shown)
                                .show_ui(ui, |ui| {
                                    for (j, item) in items.iter().enumerate() {
                                        ui.selectable_value(c, j, item);
                                    }
                                });
                        }
                        _ => {
                            if let Some(text) = self.form.texts.get_mut(i) {
                                let edit = egui::TextEdit::singleline(text).desired_width(160.0);
                                let r = ui.add(edit);
                                if i == 0 && !self.form.flag {
                                    // the first entry takes the keyboard once
                                    r.request_focus();
                                    self.form.flag = true;
                                }
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        if let Err(e) = self.form.parsed(fields) {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    /// The first queued report (`Dialogs::report`): its text, scrollable
    /// and selectable, and OK.
    fn report_dialog(&mut self, ctx: &egui::Context) {
        let Some((title, text)) = self.dialogs.borrow().reports.front().cloned() else {
            return;
        };
        let mut close = false;
        let modal = question_frame::question_modal(ctx, egui::Id::new("report"), &title, |ui| {
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&text).monospace()).selectable(true),
                    );
                });
            question_frame::button_row(ui, |ui| {
                if question_frame::button(ui, xtrude_model::i18n::tr("OK", &[])).clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.dialogs.borrow_mut().reports.pop_front();
        }
    }

    fn questions(&mut self, ctx: &egui::Context) {
        let warnings = std::mem::take(&mut self.dialogs.borrow_mut().warnings);
        for (title, text) in warnings {
            let now = ctx.input(|i| i.time);
            self.status
                .show_message(&format!("{title}: {text}"), 8000, now);
        }
        let Some(q) = self.dialogs.borrow().pending.clone() else {
            self.form.open = false;
            return;
        };
        if !self.form.open {
            self.form = Form {
                open: true,
                ..Form::default()
            };
            match &q {
                Question::Text { current, .. } => self.form.text = current.clone(),
                Question::Number { value, .. } => self.form.number = *value,
                Question::Choose { current, .. } => self.form.index = *current,
                Question::Stl { unit, mode, .. } => {
                    self.form.index = *unit;
                    self.form.index2 = *mode;
                }
                Question::Color { current, .. } => {
                    self.form.color = current.map(|c| c as f32);
                }
                Question::SectionName { name, symbol } => {
                    self.form.text = name.clone();
                    self.form.flag = false;
                    let _ = symbol;
                }
                Question::Form { values, .. } => {
                    self.form.values = values.clone();
                    self.form.texts = values.iter().map(crate::window::form::field_text).collect();
                }
                _ => {}
            }
        }
        let mut answer: Option<Answer> = None;
        let mut cancel = false;
        let title = match &q {
            Question::Text { title, .. }
            | Question::Number { title, .. }
            | Question::Confirm { title, .. }
            | Question::Choose { title, .. } => title.clone(),
            Question::Unsaved { .. } => crate::window::dialogs::unsaved_title(),
            Question::Stl { .. } => "STL".into(),
            Question::Text3d(_) => "3D Text".into(),
            Question::SectionName { .. } => "Section Plane".into(),
            Question::Color { title, .. } | Question::Form { title, .. } => title.clone(),
        };
        let id = egui::Id::new("xtrude-question");
        question_frame::question_modal(ctx, id, &title, |ui| {
            // the primary button (OK, or Yes) was pressed
            let mut ok = false;
            match &q {
                Question::Text {
                    label, multiline, ..
                } => {
                    ui.label(label);
                    let edit = if *multiline {
                        egui::TextEdit::multiline(&mut self.form.text)
                    } else {
                        egui::TextEdit::singleline(&mut self.form.text)
                    };
                    ui.add(edit.desired_width(question_frame::DIALOG_MIN_WIDTH))
                        .request_focus();
                    ok = !*multiline && ui.input(|i| i.key_pressed(egui::Key::Enter));
                }
                Question::Number {
                    label,
                    min,
                    max,
                    decimals,
                    ..
                } => {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut self.form.number)
                            .range(*min..=*max)
                            .fixed_decimals(*decimals as usize),
                    );
                }
                Question::Unsaved { prompt } => {
                    ui.label(prompt);
                }
                Question::Confirm { text, .. } => {
                    ui.label(text);
                }
                Question::Choose { label, items, .. } => {
                    ui.label(label);
                    for (i, it) in items.iter().enumerate() {
                        ui.radio_value(&mut self.form.index, i, it);
                    }
                }
                Question::Stl { units, modes, .. } => {
                    for (i, u) in units.iter().enumerate() {
                        ui.radio_value(&mut self.form.index, i, u);
                    }
                    ui.separator();
                    for (i, m) in modes.iter().enumerate() {
                        ui.radio_value(&mut self.form.index2, i, m);
                    }
                }
                Question::Text3d(_) => {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.form.text)
                            .desired_width(question_frame::DIALOG_MIN_WIDTH),
                    );
                }
                Question::Color { .. } => {
                    egui::color_picker::color_edit_button_rgb(ui, &mut self.form.color);
                }
                Question::SectionName { .. } => {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form.text)
                            .desired_width(question_frame::DIALOG_MIN_WIDTH),
                    );
                    ui.checkbox(&mut self.form.flag, "Don't ask again");
                }
                Question::Form { fields, .. } => {
                    self.form_fields(ui, fields);
                    ok = ui.input(|i| i.key_pressed(egui::Key::Enter));
                }
            }
            question_frame::button_row(ui, |ui| {
                if matches!(q, Question::Unsaved { .. }) {
                    // its own Cancel is among its buttons
                    for (text, choice) in
                        crate::window::dialogs::unsaved_buttons().into_iter().rev()
                    {
                        if question_frame::button(ui, text).clicked() {
                            answer = Some(Answer::Save(choice));
                        }
                    }
                } else {
                    if question_frame::button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                    let primary = if matches!(q, Question::Confirm { .. }) {
                        "Yes"
                    } else {
                        "OK"
                    };
                    ok |= question_frame::button(ui, primary).clicked();
                }
            });
            // Save is the default button
            if matches!(q, Question::Unsaved { .. })
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                answer = Some(Answer::Save(crate::window::SaveChoice::DEFAULT));
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            if ok {
                answer = self.form.answer(&q);
            }
        });
        if let Some(a) = answer {
            self.form.open = false;
            dialogs::answer(&self.dialogs, a, &mut self.window, &mut self.tray);
        } else if cancel {
            self.form.open = false;
            dialogs::dismiss(&self.dialogs, &mut self.window, &mut self.tray);
        }
    }
}

/// The anti-aliasing sample count the GPU can do for the viewport's
/// colour and depth targets: 1 (off), 4 (always available), or 2 / 8 where
/// the adapter supports them — otherwise 4.
fn supported_samples(rs: &eframe::egui_wgpu::RenderState, n: u32) -> u32 {
    match n {
        0 | 1 => 1,
        4 => 4,
        _ => {
            let ok = |f: wgpu::TextureFormat| {
                rs.adapter
                    .get_texture_format_features(f)
                    .flags
                    .sample_count_supported(n)
            };
            let adapter_specific = rs
                .device
                .features()
                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
            if adapter_specific
                && ok(wgpu::TextureFormat::Rgba8Unorm)
                && ok(wgpu::TextureFormat::Depth32Float)
            {
                n
            } else {
                4
            }
        }
    }
}

/// The pointer over the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportPointer {
    /// The tool's bitmap (`icons::os_cursor`), as the OS cursor.
    Image(Cursor),
    /// A system cursor.
    Icon(egui::CursorIcon),
}

/// What the pointer over the viewport is for the viewport's `cursor`: a
/// bitmap when the cursor has one, the system cursor otherwise (the arrow
/// for Select).
pub fn viewport_pointer(cursor: Option<Cursor>) -> ViewportPointer {
    match cursor {
        Some(c) if icons::has_cursor(c.key()) => ViewportPointer::Image(c),
        c => ViewportPointer::Icon(cursor_icon(c)),
    }
}

fn cursor_icon(cursor: Option<Cursor>) -> egui::CursorIcon {
    match cursor {
        Some(Cursor::Orbit) => egui::CursorIcon::AllScroll,
        Some(Cursor::Pan) => egui::CursorIcon::Grab,
        Some(Cursor::Zoom) => egui::CursorIcon::ZoomIn,
        Some(Cursor::ZoomWindow) => egui::CursorIcon::Crosshair,
        Some(Cursor::NotAllowed) => egui::CursorIcon::NotAllowed,
        Some(_) => egui::CursorIcon::Crosshair,
        None => egui::CursorIcon::Default,
    }
}

impl eframe::App for XtrudeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.record_frame(&ctx, frame);
        // the bitmap cursor sticks between frames: only the viewport sets it
        ctx.set_cursor_image(None);
        // leaving the window hands the copy to the system clipboard
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        if self.focused && !focused {
            self.window.on_focus_lost();
        }
        self.focused = focused;
        // the window's close box asks first
        if ctx.input(|i| i.viewport().close_requested()) && !self.window.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.trigger("quit");
        }
        if self.window.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let clean = self
            .window
            .actions
            .get("clean_screen")
            .is_some_and(|a| a.checked);
        self.toolbar_geometry = LayoutGeometry::default();
        if !clean {
            egui::Panel::top("menubar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    let menus = self.window.menus.clone();
                    let titles: Vec<String> = menus
                        .iter()
                        .map(|(title, _)| title.replace('&', ""))
                        .collect();
                    menu_bar::menu_buttons(ui, &titles, |ui, index| {
                        self.menu_items(ui, &menus[index].1)
                    });
                });
            });
            // toolbar areas outside the side trays, top and bottom
            // across the whole width
            self.toolbar_area(ui, ToolbarArea::Top);
            egui::Panel::bottom("statusbar").show(ui, |ui| self.status_bar(ui));
            self.toolbar_area(ui, ToolbarArea::Bottom);
            self.toolbar_area(ui, ToolbarArea::Left);
            self.toolbar_area(ui, ToolbarArea::Right);
            self.side_tray(ui);
            let central = ui.available_rect_before_wrap();
            self.floating_toolbars(&ctx, central);
            self.toolbar_drag_frame(&ctx);
        } else {
            self.toolbar_drag = None;
        }
        let vp_rect = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport(ui, frame))
            .response
            .rect;
        if clean {
            self.clean_screen_exit(&ctx, vp_rect);
        } else {
            self.sidebar_handle(&ctx, vp_rect);
        }
        self.apply_theme(&ctx);
        self.preferences_dialog(&ctx);
        self.about_dialog(&ctx);
        if std::mem::take(&mut self.window.model_info_requested) {
            self.open_model_info();
        }
        if let Some(dir) = self.window.open_folder_requested.take() {
            if let Err(e) = open_folder(&dir) {
                use xtrude_model::i18n::tr;
                self.window.dialogs.warn(
                    &tr("Open Extensions Folder", &[]),
                    &tr(
                        "Cannot open {path}: {error}",
                        &[
                            ("path", &dir.display().to_string()),
                            ("error", &e.to_string()),
                        ],
                    ),
                );
            }
        }
        self.model_info_dialog(&ctx);
        self.solid_inspector_dialog(&ctx);
        self.questions(&ctx);
        self.report_dialog(&ctx);
        self.import_progress_dialog(&ctx);
        if self.window.title != self.last_title {
            self.last_title = self.window.title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.last_title.clone()));
        }
        // a .skp from the command line comes in once the window has painted
        if self.window.pending_import.is_some() {
            if self.painted {
                self.window.open_pending();
            }
            ctx.request_repaint();
        }
        self.painted = true;
        // no polling: egui draws a frame for each input and one after it;
        // what changes on its own asks for its own frame (the import above,
        // the start-up import, the status bar's timed message, the model
        // info dialog's deadline, the snap dwell timer)
    }

    fn on_exit(&mut self) {
        if let Some(stats) = &self.frame_stats {
            eprintln!("{}", stats.report());
        }
    }
}

/// Shows `dir` in the system's file manager.
fn open_folder(dir: &std::path::Path) -> std::io::Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(opener).arg(dir).spawn()?;
    // reap the opener when it exits
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xtrude_model::text3d::TextParams;

    #[test]
    fn ok_answers_a_parameter_form_only_when_every_entry_reads() {
        xtrude_model::settings::use_memory_store();
        let fields = vec![
            ParamSpec {
                key: "n".into(),
                label: "Teeth".into(),
                kind: ParamKind::Int {
                    min: Some(6),
                    max: None,
                },
                default: ParamValue::Int(20),
            },
            ParamSpec {
                key: "hub".into(),
                label: "Hub".into(),
                kind: ParamKind::Bool,
                default: ParamValue::Bool(false),
            },
        ];
        let q = Question::Form {
            title: "Gear".into(),
            fields,
            values: vec![ParamValue::Int(20), ParamValue::Bool(false)],
        };
        let mut form = Form {
            values: vec![ParamValue::Int(20), ParamValue::Bool(true)],
            texts: vec!["12".into(), String::new()],
            ..Form::default()
        };
        assert!(matches!(
            form.answer(&q),
            Some(Answer::Form(Some(v))) if v == [ParamValue::Int(12), ParamValue::Bool(true)]
        ));
        form.texts[0] = "3".into();
        assert!(form.answer(&q).is_none(), "below the minimum");
        form.texts[0] = "many".into();
        assert!(form.answer(&q).is_none(), "not a number");
    }

    #[test]
    fn ok_answers_each_question_from_the_form() {
        let form = Form {
            text: "Part".into(),
            number: 2.5,
            index: 1,
            index2: 2,
            flag: true,
            color: [0.5, 0.25, 1.0],
            open: true,
            ..Form::default()
        };
        let text = Question::Text {
            title: String::new(),
            label: String::new(),
            current: String::new(),
            multiline: false,
        };
        assert!(matches!(form.answer(&text), Some(Answer::Text(Some(t))) if t == "Part"));
        let confirm = Question::Confirm {
            title: String::new(),
            text: String::new(),
        };
        assert!(matches!(form.answer(&confirm), Some(Answer::Bool(true))));
        let stl = Question::Stl {
            units: vec![],
            unit: 0,
            modes: vec![],
            mode: 0,
        };
        assert!(matches!(form.answer(&stl), Some(Answer::Stl(Some((1, 2))))));
        let section = Question::SectionName {
            name: String::new(),
            symbol: "A".into(),
        };
        assert!(matches!(
            form.answer(&section),
            Some(Answer::Section(Some(s))) if s.name == "Part" && s.symbol == "A" && s.dont_ask
        ));
        let color = Question::Color {
            title: String::new(),
            current: [0.0; 3],
        };
        assert!(matches!(
            form.answer(&color),
            Some(Answer::Color(Some([0.5, 0.25, 1.0])))
        ));
    }

    #[test]
    fn unsaved_question_has_no_ok_answer() {
        let q = Question::Unsaved {
            prompt: String::new(),
        };
        assert!(Form::default().answer(&q).is_none());
    }

    #[test]
    fn empty_3d_text_keeps_the_previous_text() {
        let p = TextParams {
            text: "Xtrude".into(),
            ..TextParams::default()
        };
        let q = Question::Text3d(p);
        let kept = Form::default().answer(&q);
        assert!(matches!(kept, Some(Answer::Text3d(Some(np))) if np.text == "Xtrude"));
        let form = Form {
            text: "CAD".into(),
            ..Form::default()
        };
        assert!(matches!(form.answer(&q), Some(Answer::Text3d(Some(np))) if np.text == "CAD"));
    }
}
