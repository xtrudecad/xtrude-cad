// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Copy and paste between Xtrude CAD windows: the
//! system-clipboard half. The payload codec is [`xtrude_io::clip`].
//!
//! The model clipboard lives on the viewport, so it never leaves the
//! process. Copy also offers it on the system clipboard under [`MIME`], and
//! Paste adopts it when it came from another window. The offer is LAZY:
//! [`ClipOffer`] encodes only when another application asks for the bytes,
//! so Ctrl+C on a big model costs nothing extra in the window that copies.
//! A paste in the same window sees its own offer and keeps the in-memory
//! clipboard.
//!
//! The platform clipboard carries the payload as TEXT ([`bytes_to_text`]):
//! a header line, then the bytes in base64. A Copy puts only the header
//! there; the window writes the bytes when it loses focus
//! ([`SystemClipboard::export`]) or quits.

use std::cell::OnceCell;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use xtrude_io::clip::{self, ClipData};

pub use xtrude_io::clip::MIME;

/// The first line of a model clipboard carried as text.
pub const TEXT_HEADER: &str = "Xtrude CAD clipboard v1 — paste into Xtrude CAD";

/// The clipboard bytes as text for the platform clipboard.
pub fn bytes_to_text(bytes: &[u8]) -> String {
    format!("{TEXT_HEADER}\n{}", STANDARD.encode(bytes))
}

/// The bytes back from clipboard text, or `None` when the text is not a
/// model clipboard (any other text someone copied).
pub fn text_to_bytes(text: &str) -> Option<Vec<u8>> {
    let rest = text.strip_prefix(TEXT_HEADER)?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest).strip_prefix('\n')?;
    let b64: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(b64).ok()
}

/// True when `text` is a model clipboard (by its header).
pub fn is_clip_text(text: &str) -> bool {
    text.starts_with(TEXT_HEADER)
}

/// The system-clipboard entry of one Copy: [`MIME`] rendered on demand
/// from the clipboard it was made for.
#[derive(Debug)]
pub struct ClipOffer {
    clip: ClipData,
    bytes: OnceCell<Vec<u8>>,
}

impl ClipOffer {
    pub fn new(clip: ClipData) -> ClipOffer {
        ClipOffer {
            clip,
            bytes: OnceCell::new(),
        }
    }

    pub fn clip(&self) -> &ClipData {
        &self.clip
    }

    pub fn formats(&self) -> Vec<&'static str> {
        vec![MIME]
    }

    pub fn has_format(&self, mime: &str) -> bool {
        mime == MIME
    }

    /// The bytes under `mime`, encoded on the first request (`None` for any
    /// other type; empty when the clipboard cannot be encoded).
    pub fn retrieve_data(&self, mime: &str) -> Option<&[u8]> {
        if mime != MIME {
            return None;
        }
        Some(
            self.bytes
                .get_or_init(|| clip::encode(&self.clip).unwrap_or_default()),
        )
    }
}

/// The system clipboard, as far as the model clipboard needs it. The shell
/// provides the platform's; [`MemoryClipboard`] keeps it in the process.
pub trait SystemClipboard {
    /// Offer this window's copy, rendered only when asked for.
    fn set_offer(&mut self, offer: ClipOffer);
    /// Replace the content with plain data: `(mime, bytes)` pairs, possibly
    /// none.
    fn set_data(&mut self, data: Vec<(String, Vec<u8>)>);
    /// This window's own offer, while the clipboard still holds it.
    fn offer(&self) -> Option<&ClipOffer>;
    fn has_format(&self, mime: &str) -> bool;
    /// The bytes under `mime`, whoever put them there.
    fn data(&self, mime: &str) -> Option<Vec<u8>>;
    /// The window lost focus: hand the pending offer to the platform now,
    /// since another window can only paste after it is focused. The offer
    /// stays this window's own while the clipboard holds what was written.
    fn export(&mut self) {}
}

/// A clipboard held in memory: the window's own when the shell has no
/// platform one, and the tests'.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    offer: Option<ClipOffer>,
    data: Vec<(String, Vec<u8>)>,
}

impl SystemClipboard for MemoryClipboard {
    fn set_offer(&mut self, offer: ClipOffer) {
        self.data.clear();
        self.offer = Some(offer);
    }

    fn set_data(&mut self, data: Vec<(String, Vec<u8>)>) {
        self.offer = None;
        self.data = data;
    }

    fn offer(&self) -> Option<&ClipOffer> {
        self.offer.as_ref()
    }

    fn has_format(&self, mime: &str) -> bool {
        match &self.offer {
            Some(o) => o.has_format(mime),
            None => self.data.iter().any(|(m, _)| m == mime),
        }
    }

    fn data(&self, mime: &str) -> Option<Vec<u8>> {
        match &self.offer {
            Some(o) => o.retrieve_data(mime).map(<[u8]>::to_vec),
            None => self
                .data
                .iter()
                .find(|(m, _)| m == mime)
                .map(|(_, d)| d.clone()),
        }
    }
}

/// Offer `clip` on the system clipboard (after a Copy/Cut).
pub fn publish(cb: &mut dyn SystemClipboard, clip: ClipData) {
    cb.set_offer(ClipOffer::new(clip));
}

/// On quit, trade the lazy offer for plain data: the copy then survives
/// this window closing, and another window can still paste it.
pub fn flush(cb: &mut dyn SystemClipboard) {
    let Some(offer) = cb.offer() else {
        return;
    };
    let data = offer.retrieve_data(MIME).unwrap_or_default().to_vec();
    let plain = if data.is_empty() {
        Vec::new()
    } else {
        vec![(MIME.to_string(), data)]
    };
    cb.set_data(plain);
}

/// The clipboard another Xtrude CAD window copied, decoded — or `None` when
/// the system clipboard holds this window's own copy, or no model clipboard
/// at all.
pub fn foreign(cb: &dyn SystemClipboard) -> Option<ClipData> {
    if cb.offer().is_some() || !cb.has_format(MIME) {
        return None;
    }
    clip::decode(&cb.data(MIME)?)
}

/// True when the system clipboard offers a model clipboard (this window's
/// or another's) — for enabling Paste.
pub fn available(cb: &dyn SystemClipboard) -> bool {
    cb.has_format(MIME)
}

/// A platform clipboard that holds text (the shell's is `arboard`).
pub trait TextStore {
    fn get_text(&mut self) -> Option<String>;
    /// Replace the clipboard with `text`; false when the platform refused.
    fn set_text(&mut self, text: String) -> bool;
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The model clipboard on a text-only platform clipboard. A Copy puts
/// only [`TEXT_HEADER`] there — cheap, and it takes the clipboard over, so
/// Ctrl+V always has something to fire on — and the window keeps the
/// offer; [`export`](SystemClipboard::export) (focus lost) or a quit
/// writes the payload. The offer is this window's own while the platform
/// still holds the text it last wrote.
pub struct TextClipboard<S: TextStore> {
    store: std::cell::RefCell<S>,
    offer: Option<ClipOffer>,
    /// The hash of the text this window last wrote for its offer.
    written: Option<u64>,
    /// The offer's payload is on the platform clipboard.
    exported: bool,
}

impl<S: TextStore> TextClipboard<S> {
    pub fn new(store: S) -> TextClipboard<S> {
        TextClipboard {
            store: std::cell::RefCell::new(store),
            offer: None,
            written: None,
            exported: false,
        }
    }

    fn text(&self) -> Option<String> {
        self.store.borrow_mut().get_text()
    }

    fn write(&mut self, text: String) {
        let hash = hash_of(&text);
        self.written = self.store.get_mut().set_text(text).then_some(hash);
    }
}

impl<S: TextStore> SystemClipboard for TextClipboard<S> {
    fn set_offer(&mut self, offer: ClipOffer) {
        self.offer = Some(offer);
        self.exported = false;
        self.write(TEXT_HEADER.to_string());
    }

    fn set_data(&mut self, data: Vec<(String, Vec<u8>)>) {
        self.offer = None;
        self.written = None;
        if let Some((_, bytes)) = data.iter().find(|(m, b)| m == MIME && !b.is_empty()) {
            self.write(bytes_to_text(bytes));
        }
    }

    fn offer(&self) -> Option<&ClipOffer> {
        let offer = self.offer.as_ref()?;
        match self.written {
            // the platform would not take it: nobody else has it either
            None => Some(offer),
            Some(h) => (self.text().map(|t| hash_of(&t)) == Some(h)).then_some(offer),
        }
    }

    fn has_format(&self, mime: &str) -> bool {
        mime == MIME && (self.offer().is_some() || self.text().is_some_and(|t| is_clip_text(&t)))
    }

    fn data(&self, mime: &str) -> Option<Vec<u8>> {
        if mime != MIME {
            return None;
        }
        if let Some(o) = self.offer() {
            return o.retrieve_data(MIME).map(<[u8]>::to_vec);
        }
        text_to_bytes(&self.text()?)
    }

    fn export(&mut self) {
        // only over this window's own copy: never over a newer one
        if self.exported || self.offer().is_none() {
            return;
        }
        let Some(bytes) = self
            .offer
            .as_ref()
            .and_then(|o| o.retrieve_data(MIME))
            .filter(|b| !b.is_empty())
            .map(<[u8]>::to_vec)
        else {
            return;
        };
        self.write(bytes_to_text(&bytes));
        self.exported = true;
    }
}
