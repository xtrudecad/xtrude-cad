// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Application settings: a flat key → JSON value store saved as one JSON
//! file under `$XDG_CONFIG_HOME/xtrude/settings.json`.
//!
//! Every setting the app uses is declared once in [`keys`], with its type
//! and default, and read and written through [`read`], [`stored`],
//! [`write`] and [`clear`]. The key strings and stored encodings are those
//! of existing settings files and do not change. The untyped [`get`] /
//! [`set`] reach the raw store (tests check the stored encoding with them).
//!
//! Tests isolate the store per thread with [`use_memory_store`].

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;

use serde_json::{Map, Value};

#[derive(Clone, Debug, Default)]
struct Store {
    path: Option<PathBuf>,
    data: Map<String, Value>,
    loaded: bool,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

/// The app's configuration folder: `$XDG_CONFIG_HOME/xtrude`, else
/// `~/.config/xtrude`. It holds the settings file and the extensions.
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("xtrude"))
}

fn default_path() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.json"))
}

fn ensure_loaded(s: &mut Store) {
    if s.loaded {
        return;
    }
    s.loaded = true;
    if s.path.is_none() && !cfg!(test) {
        s.path = default_path();
    }
    if let Some(p) = &s.path {
        if let Ok(text) = std::fs::read_to_string(p) {
            if let Ok(Value::Object(m)) = serde_json::from_str(&text) {
                s.data = m;
            }
        }
    }
}

/// Keep settings in memory only for this thread (tests).
pub fn use_memory_store() {
    STORE.with(|s| {
        *s.borrow_mut() = Store {
            path: None,
            data: Map::new(),
            loaded: true,
        }
    });
}

/// Use the settings file at `path` for this thread.
pub fn use_file(path: PathBuf) {
    STORE.with(|s| {
        *s.borrow_mut() = Store {
            path: Some(path),
            data: Map::new(),
            loaded: false,
        }
    });
}

pub fn get(key: &str) -> Option<Value> {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_loaded(&mut s);
        s.data.get(key).cloned()
    })
}

pub fn get_str(key: &str) -> Option<String> {
    get(key).and_then(|v| v.as_str().map(str::to_string))
}

pub fn get_i64(key: &str, default: i64) -> i64 {
    get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// Every change is written to the file at once, so it survives a quit or
/// a crash that comes before anything else saves.
pub fn set(key: &str, value: Value) {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_loaded(&mut s);
        if s.data.get(key) != Some(&value) {
            s.data.insert(key.to_string(), value);
            flush(&s);
        }
    });
}

pub fn remove(key: &str) {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_loaded(&mut s);
        if s.data.remove(key).is_some() {
            flush(&s);
        }
    });
}

/// How a setting's value is kept in the store.
pub trait Stored: Sized {
    /// The value, or `None` when the stored JSON is not one.
    fn from_stored(v: &Value) -> Option<Self>;
    fn to_stored(&self) -> Value;
}

impl Stored for String {
    fn from_stored(v: &Value) -> Option<Self> {
        v.as_str().map(str::to_string)
    }
    fn to_stored(&self) -> Value {
        Value::from(self.as_str())
    }
}

impl Stored for i64 {
    fn from_stored(v: &Value) -> Option<Self> {
        v.as_i64()
    }
    fn to_stored(&self) -> Value {
        Value::from(*self)
    }
}

/// A yes/no setting, stored as the text `"1"`/`"0"`. A JSON `true`/`false`
/// (what some versions wrote) reads too, and any text but `"0"` is yes.
impl Stored for bool {
    fn from_stored(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(*b),
            Value::String(s) => Some(s != "0"),
            _ => None,
        }
    }
    fn to_stored(&self) -> Value {
        Value::from(if *self { "1" } else { "0" })
    }
}

/// Any JSON, for the settings whose readers accept several shapes.
impl Stored for Value {
    fn from_stored(v: &Value) -> Option<Self> {
        Some(v.clone())
    }
    fn to_stored(&self) -> Value {
        self.clone()
    }
}

/// A JSON value stored as its text inside a string (`styles/user`,
/// `units/new_document`). Text that does not parse reads as missing.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonText(pub Value);

impl Stored for JsonText {
    fn from_stored(v: &Value) -> Option<Self> {
        serde_json::from_str(v.as_str()?).ok().map(JsonText)
    }
    fn to_stored(&self) -> Value {
        Value::String(self.0.to_string())
    }
}

/// A setting: its key in the settings file, its type and its default.
pub struct Key<T> {
    name: Cow<'static, str>,
    default: fn() -> T,
}

impl<T> Key<T> {
    pub const fn new(name: &'static str, default: fn() -> T) -> Key<T> {
        Key {
            name: Cow::Borrowed(name),
            default,
        }
    }

    /// A key whose name is built at run time (one per dock panel).
    pub fn named(name: String, default: fn() -> T) -> Key<T> {
        Key {
            name: Cow::Owned(name),
            default,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The stored value, or `None` when it is missing or of another type.
pub fn stored<T: Stored>(key: &Key<T>) -> Option<T> {
    get(key.name()).and_then(|v| T::from_stored(&v))
}

/// The stored value, or the key's default.
pub fn read<T: Stored>(key: &Key<T>) -> T {
    stored(key).unwrap_or_else(key.default)
}

pub fn write<T: Stored>(key: &Key<T>, value: &T) {
    set(key.name(), value.to_stored());
}

pub fn clear<T>(key: &Key<T>) {
    remove(key.name());
}

/// Every setting the app keeps.
pub mod keys {
    use serde_json::Value;

    use super::{JsonText, Key};

    fn yes() -> bool {
        true
    }
    fn no() -> bool {
        false
    }
    fn empty() -> String {
        String::new()
    }
    fn mm() -> String {
        "mm".into()
    }
    fn null() -> Value {
        Value::Null
    }
    fn no_json() -> JsonText {
        JsonText(Value::Null)
    }

    // General
    /// Copy the previous file to `<name>.bak` before saving over it.
    pub const BACKUP: Key<bool> = Key::new("general/backup", yes);
    pub const UNDO_STEPS: Key<i64> = Key::new("general/undo_steps", || 200);
    /// The windowing backend chosen in Preferences (`""` = automatic).
    pub const PLATFORM: Key<String> = Key::new("general/platform", empty);
    pub const THEME: Key<String> = Key::new("general/theme", empty);
    /// The folder the file dialogs open in.
    pub const LAST_DIR: Key<String> = Key::new("files/last_dir", empty);
    /// The last documents, most recent first (a list, or one path).
    pub const RECENT_FILES: Key<Value> = Key::new("recent_files", null);
    /// Ask a new section plane's name and symbol.
    pub const ASK_SECTION_NAME: Key<bool> = Key::new("section/ask_name", yes);

    // Navigation and display
    pub const INVERT_WHEEL: Key<bool> = Key::new("nav/invert_wheel", no);
    pub const INVERT_ORBIT_Y: Key<bool> = Key::new("nav/invert_orbit_y", no);
    /// The mouse navigation style (`NavStyle::key`; unknown = the default).
    pub const NAV_STYLE: Key<String> = Key::new("nav/style", || "default".into());
    pub const MSAA: Key<i64> = Key::new("display/msaa", || 4);
    /// How the model outside the edited group draws (`RestMode::key`).
    pub const EDIT_REST_MODE: Key<String> = Key::new("display/edit_rest_mode", empty);

    // Import
    pub const OBJ_UNIT: Key<String> = Key::new("import/obj_unit", mm);
    pub const DXF_UNIT: Key<String> = Key::new("import/dxf_unit", mm);
    pub const STL_UNIT: Key<String> = Key::new("import/stl_unit", mm);
    pub const STL_SIMPLIFY_MODE: Key<String> =
        Key::new("import/stl_simplify_mode", || "principal".into());
    /// The STEP import's tessellation, a JSON object as text:
    /// `{"chord_mm": 0.01, "angle_deg": 10}` (`xtrude_io::step::Tessellation`;
    /// unset or invalid values take those defaults).
    pub const STEP_TESSELLATION: Key<JsonText> = Key::new("import/step_tessellation", no_json);

    // Interface
    /// The toolbar arrangement (`ToolbarLayout` JSON). `ui/window_state`,
    /// an older window-state blob, is never read or written.
    pub const TOOLBAR_LAYOUT: Key<Value> = Key::new("ui/toolbar_layout", null);
    /// The toolbar icon size in pixels (a number, or a number as text).
    pub const TOOLBAR_ICON_PX: Key<Value> = Key::new("ui/toolbar_icon_px", null);
    /// The member each stacked toolbar button shows (a JSON object, stack
    /// id → action id).
    pub const TOOLBAR_STACKS: Key<Value> = Key::new("ui/toolbar_stacks", null);
    /// The old «large icons» toggle (`"1"`), read once to migrate it.
    pub const LARGE_TOOLBAR_ICONS: Key<String> = Key::new("ui/large_toolbar_icons", empty);
    // The right-hand tray of 0.1.0 stored `ui/hidden_trays`,
    // `ui/hidden_tray_sections`, `ui/tray_section_order` and
    // `tray/collapsed/<class>`; the left dock replaced it and leaves them
    // unread, so a 0.1.0 layout opens as the factory one. Do not reuse
    // those names.
    /// The left dock's panels the user closed, by panel class (a list);
    /// unset, the factory ones.
    pub const HIDDEN_DOCK_PANELS: Key<Value> = Key::new("ui/hidden_dock_panels", null);
    /// The model tree's optional columns shown (a list of column keys).
    pub const MODEL_TREE_COLUMNS: Key<Value> = Key::new("ui/model_tree_columns", null);

    /// The values last given to an extension command's parameters: a JSON
    /// object by parameter key, lengths in metres.
    pub fn ext_params(ext: &str, command: &str) -> Key<JsonText> {
        Key::named(format!("ext/{ext}/{command}/params"), no_json)
    }

    /// Whether a dock panel is folded, by its panel class name.
    pub fn dock_collapsed(class: &str) -> Key<bool> {
        Key::named(format!("dock/collapsed/{class}"), no)
    }

    // Document defaults
    /// The user's display styles, a JSON list stored as text.
    pub const USER_STYLES: Key<JsonText> = Key::new("styles/user", no_json);
    /// The units a new document starts with, JSON stored as text.
    pub const NEW_DOCUMENT_UNITS: Key<JsonText> = Key::new("units/new_document", no_json);
}

/// Flush to disk now. Failures are ignored: settings never
/// break the app.
pub fn sync() {
    STORE.with(|s| flush(&s.borrow()));
}

/// Write the store to its file, if it has one (failures ignored).
fn flush(s: &Store) {
    if let Some(p) = &s.path {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&Value::Object(s.data.clone())) {
            let tmp = p.with_extension("json.part");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YES: Key<bool> = Key::new("t/yes", || true);
    const NO: Key<bool> = Key::new("t/no", || false);

    #[test]
    fn flags_read_both_encodings_and_store_text() {
        use_memory_store();
        assert!(read(&YES) && !read(&NO));
        write(&YES, &false);
        assert_eq!(get_str("t/yes").as_deref(), Some("0"));
        assert!(!read(&YES));
        set("t/no", Value::Bool(true));
        assert!(read(&NO));
        set("t/yes", Value::Bool(false));
        assert!(!read(&YES));
        set("t/no", Value::from(3));
        assert!(!read(&NO) && stored(&NO).is_none());
        clear(&YES);
        assert!(read(&YES) && get("t/yes").is_none());
    }

    #[test]
    fn json_text_is_stored_as_a_string() {
        use_memory_store();
        let k = &keys::NEW_DOCUMENT_UNITS;
        assert_eq!(stored(k), None);
        write(k, &JsonText(serde_json::json!({"length": "cm"})));
        assert_eq!(get_str(k.name()).as_deref(), Some(r#"{"length":"cm"}"#));
        assert_eq!(read(k).0["length"], "cm");
        set(k.name(), Value::from("{not json"));
        assert_eq!(stored(k), None);
    }

    #[test]
    fn typed_values_keep_their_types() {
        use_memory_store();
        assert_eq!(read(&keys::UNDO_STEPS), 200);
        set("general/undo_steps", Value::from("50"));
        assert_eq!(read(&keys::UNDO_STEPS), 200); // text is not a number
        write(&keys::UNDO_STEPS, &50);
        assert_eq!(get_i64("general/undo_steps", 0), 50);
        assert_eq!(read(&keys::OBJ_UNIT), "mm");
        write(&keys::OBJ_UNIT, &"cm".to_string());
        assert_eq!(get_str("import/obj_unit").as_deref(), Some("cm"));
    }

    #[test]
    fn a_change_reaches_the_file_without_an_explicit_sync() {
        // a setting changed and the app quit (or crashed) before anything
        // else synced: the next start still has it
        let dir = std::env::temp_dir().join(format!("xtrude-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);
        use_file(path.clone());
        set("t/kept", Value::from("yes"));
        use_file(path.clone()); // the next start
        assert_eq!(get_str("t/kept").as_deref(), Some("yes"));
        remove("t/kept");
        use_file(path.clone());
        assert_eq!(get("t/kept"), None);
        let _ = std::fs::remove_dir_all(&dir);
        use_memory_store();
    }
}
