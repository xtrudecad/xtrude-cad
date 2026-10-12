// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Window ▸ Preferences — the scattered settings, gathered in one dialog, as a
//! toolkit-neutral model: the shell edits the fields, OK calls
//! [`Preferences::accept`], Cancel drops it.
//!
//! Five pages ([`PrefsPage`]), each a few titled groups of fields
//! ([`PrefsGroup`], [`PrefsField`]). **General**: theme, toolbar icons, how
//! the rest of the model reads while editing a group (applied through
//! `set_edit_rest_mode`, the Camera menu's path), the section-plane prompt,
//! backups, undo depth. **Navigation**: the mouse style, the wheel and orbit
//! directions. **Display**: anti-aliasing, graphics server. **Import**: the
//! units the OBJ and DXF import dialogs preselect. **Units**: the open
//! document's units.

use serde_json::json;
use xtrude_model::settings::{self, keys};
use xtrude_model::units::{self, Units};

use super::MainWindow;
use crate::platform_choice::{AUTO, WAYLAND, XCB};
use crate::theme;
use crate::viewport::{NavStyle, RestMode};

/// The import dialogs' unit vocabularies.
pub const OBJ_UNITS: [(&str, &str); 5] = [
    ("m", "Metres"),
    ("cm", "Centimetres"),
    ("mm", "Millimetres"),
    ("in", "Inches"),
    ("ft", "Feet"),
];
pub const DXF_UNITS: [(&str, &str); 5] = [
    ("mm", "Millimetres"),
    ("cm", "Centimetres"),
    ("m", "Metres"),
    ("in", "Inches"),
    ("ft", "Feet"),
];
pub const REST_MODES: [(RestMode, &str); 3] = [
    (RestMode::Normal, "Show normally"),
    (RestMode::Fade, "Fade"),
    (RestMode::Hide, "Hide (fastest)"),
];
pub const THEME_CHOICES: [(&str, &str); 3] = [
    (theme::DARK, "Dark"),
    (theme::LIGHT, "Light"),
    (theme::SYSTEM, "Same as the system"),
];
pub const MSAA_CHOICES: [u32; 4] = [0, 2, 4, 8];
pub const PLATFORM_CHOICES: [(&str, &str); 3] = [
    (AUTO, "Automatic (Wayland when available)"),
    (WAYLAND, "Wayland"),
    (XCB, "X11 (XWayland)"),
];

/// Toolbar icon sizes.
pub const TOOLBAR_ICON_SIZES: [(u32, &str); 4] = [
    (20, "Small"),
    (24, "Normal"),
    (32, "Large"),
    (40, "Extra large"),
];
pub const DEFAULT_TOOLBAR_ICON_PX: u32 = 24;

/// The toolbar icon size from the settings, migrating the old «large
/// icons» toggle on first read.
pub fn toolbar_icon_px() -> u32 {
    let px = match settings::stored(&keys::TOOLBAR_ICON_PX) {
        None => {
            if settings::stored(&keys::LARGE_TOOLBAR_ICONS).as_deref() == Some("1") {
                32
            } else {
                DEFAULT_TOOLBAR_ICON_PX
            }
        }
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map_or(DEFAULT_TOOLBAR_ICON_PX, |p| p.max(0) as u32),
    };
    if TOOLBAR_ICON_SIZES.iter().any(|(s, _)| *s == px) {
        px
    } else {
        DEFAULT_TOOLBAR_ICON_PX
    }
}

pub fn save_toolbar_icon_px(px: u32) {
    settings::write(&keys::TOOLBAR_ICON_PX, &json!(px));
    settings::clear(&keys::LARGE_TOOLBAR_ICONS);
}

/// The saved anti-aliasing sample count (`display/msaa`, 4 by default).
pub fn saved_msaa() -> u32 {
    let n = settings::read(&keys::MSAA);
    if MSAA_CHOICES.contains(&(n.max(0) as u32)) {
        n as u32
    } else {
        4
    }
}

/// A page of the dialog, in the order of its page list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefsPage {
    General,
    Navigation,
    Display,
    Import,
    Units,
}

/// A setting of the dialog: one row of a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefsField {
    Theme,
    ToolbarIcons,
    RestOfModel,
    AskSectionName,
    Backup,
    UndoSteps,
    /// The mouse style, with the bindings it sets.
    NavStyle,
    InvertWheel,
    InvertOrbit,
    Msaa,
    Platform,
    ObjUnit,
    DxfUnit,
    LengthUnit,
    Decimals,
    UnitsForNew,
}

/// A titled group of fields on a page, with an optional note under them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefsGroup {
    /// The untranslated title.
    pub title: &'static str,
    pub fields: &'static [PrefsField],
    /// An untranslated explanation drawn under the fields.
    pub note: Option<&'static str>,
}

impl PrefsField {
    /// Every field, one per editable field of [`Preferences`].
    pub const ALL: [PrefsField; 16] = [
        PrefsField::Theme,
        PrefsField::ToolbarIcons,
        PrefsField::RestOfModel,
        PrefsField::AskSectionName,
        PrefsField::Backup,
        PrefsField::UndoSteps,
        PrefsField::NavStyle,
        PrefsField::InvertWheel,
        PrefsField::InvertOrbit,
        PrefsField::Msaa,
        PrefsField::Platform,
        PrefsField::ObjUnit,
        PrefsField::DxfUnit,
        PrefsField::LengthUnit,
        PrefsField::Decimals,
        PrefsField::UnitsForNew,
    ];
}

impl PrefsPage {
    pub const ALL: [PrefsPage; 5] = [
        PrefsPage::General,
        PrefsPage::Navigation,
        PrefsPage::Display,
        PrefsPage::Import,
        PrefsPage::Units,
    ];

    /// The untranslated name, in the page list and as the page's heading.
    pub fn label(self) -> &'static str {
        match self {
            PrefsPage::General => "General",
            PrefsPage::Navigation => "Navigation",
            PrefsPage::Display => "Display",
            PrefsPage::Import => "Import",
            PrefsPage::Units => "Units",
        }
    }

    /// The page's groups, top to bottom.
    pub fn groups(self) -> &'static [PrefsGroup] {
        use PrefsField as F;
        match self {
            PrefsPage::General => &[
                PrefsGroup {
                    title: "Interface",
                    fields: &[F::Theme, F::ToolbarIcons],
                    note: None,
                },
                PrefsGroup {
                    title: "Editing",
                    fields: &[F::RestOfModel, F::AskSectionName],
                    note: None,
                },
                PrefsGroup {
                    title: "Saving and undo",
                    fields: &[F::Backup, F::UndoSteps],
                    note: None,
                },
            ],
            PrefsPage::Navigation => &[PrefsGroup {
                title: "Mouse",
                fields: &[F::NavStyle, F::InvertWheel, F::InvertOrbit],
                note: None,
            }],
            PrefsPage::Display => &[
                PrefsGroup {
                    title: "Rendering",
                    fields: &[F::Msaa],
                    note: None,
                },
                PrefsGroup {
                    title: "System",
                    fields: &[F::Platform],
                    note: None,
                },
            ],
            PrefsPage::Import => &[PrefsGroup {
                title: "Suggested units",
                fields: &[F::ObjUnit, F::DxfUnit],
                note: None,
            }],
            PrefsPage::Units => &[PrefsGroup {
                title: "Open document",
                fields: &[F::LengthUnit, F::Decimals, F::UnitsForNew],
                note: Some(UNITS_NOTE),
            }],
        }
    }
}

/// The Units page's explanation.
const UNITS_NOTE: &str = "These are the units of the document you have open — they are saved with it. A number typed without a unit is in this unit («2» is 2 mm in a millimetre document; «2m» is always 2 m), and every length and area on screen is shown in it. The dimension style follows it; its own panel can still set it apart.";

fn pick(choices: &[(&str, &str)], value: &str, default: &str) -> String {
    if choices.iter().any(|(k, _)| *k == value) {
        value.to_string()
    } else {
        default.to_string()
    }
}

/// The dialog's fields, read from the settings and the window when it opens.
#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub theme: String,
    pub rest: RestMode,
    pub backup: bool,
    pub undo_steps: u32,
    pub ask_section: bool,
    pub nav_style: NavStyle,
    pub invert: bool,
    pub invert_orbit: bool,
    pub msaa: u32,
    pub icon_px: u32,
    pub platform: String,
    pub obj_unit: String,
    pub dxf_unit: String,
    pub unit: String,
    pub decimals: u32,
    pub units_for_new: bool,
}

impl Preferences {
    pub fn load(w: &MainWindow) -> Preferences {
        let current = w.viewport.scene.units.clone();
        let platform = settings::read(&keys::PLATFORM);
        Preferences {
            theme: theme::saved_theme(),
            rest: w.viewport.edit_rest_mode(),
            backup: settings::read(&keys::BACKUP),
            undo_steps: settings::read(&keys::UNDO_STEPS).clamp(0, 5000) as u32,
            ask_section: settings::read(&keys::ASK_SECTION_NAME),
            nav_style: NavStyle::from_key(&settings::read(&keys::NAV_STYLE)),
            invert: settings::read(&keys::INVERT_WHEEL),
            invert_orbit: settings::read(&keys::INVERT_ORBIT_Y),
            msaa: saved_msaa(),
            icon_px: toolbar_icon_px(),
            platform: pick(&PLATFORM_CHOICES, &platform, AUTO),
            obj_unit: pick(&OBJ_UNITS, &settings::read(&keys::OBJ_UNIT), "mm"),
            dxf_unit: pick(&DXF_UNITS, &settings::read(&keys::DXF_UNIT), "mm"),
            unit: current.length.clone(),
            decimals: current.precision.clamp(0, 6) as u32,
            units_for_new: units::new_document_units() == current,
        }
    }

    /// OK: write every value, apply the live ones, flush.
    pub fn accept(&self, w: &mut MainWindow) {
        // Units of the open document.
        let chosen = Units {
            length: self.unit.clone(),
            precision: self.decimals as i64,
        };
        if chosen != w.viewport.scene.units {
            w.viewport.scene.apply_units(&chosen);
            w.viewport.scene.version += 1;
            w.viewport.update();
        }
        if self.units_for_new {
            units::remember_new_document_units(&chosen);
        }
        if self.theme != theme::saved_theme() {
            theme::save_theme(&self.theme);
        }
        if self.rest != w.viewport.edit_rest_mode() {
            w.viewport.set_edit_rest_mode(self.rest);
        }
        settings::write(&keys::BACKUP, &self.backup);
        settings::write(&keys::ASK_SECTION_NAME, &self.ask_section);
        settings::write(&keys::UNDO_STEPS, &i64::from(self.undo_steps));
        w.viewport.history.max_steps = self.undo_steps as usize;
        settings::write(&keys::PLATFORM, &self.platform);
        settings::write(&keys::NAV_STYLE, &self.nav_style.key().to_string());
        w.viewport.prefs.nav_style = self.nav_style;
        settings::write(&keys::INVERT_WHEEL, &self.invert);
        w.viewport.prefs.invert_wheel = self.invert;
        settings::write(&keys::INVERT_ORBIT_Y, &self.invert_orbit);
        w.viewport.prefs.invert_orbit_y = self.invert_orbit;
        settings::write(&keys::MSAA, &i64::from(self.msaa));
        if w.viewport.prefs.msaa != self.msaa {
            w.viewport.prefs.msaa = self.msaa;
            w.viewport.update();
        }
        if self.icon_px != toolbar_icon_px() {
            save_toolbar_icon_px(self.icon_px);
        }
        settings::write(&keys::OBJ_UNIT, &self.obj_unit);
        settings::write(&keys::DXF_UNIT, &self.dxf_unit);
        settings::sync();
    }
}
