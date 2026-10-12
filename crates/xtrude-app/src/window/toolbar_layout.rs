// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Where the toolbars sit, in four docking areas around the viewport: every
//! bar is movable, floatable and dockable in all four areas, and the
//! arrangement is saved and restored. No egui types: the shell measures
//! the bars each frame and hands the rectangles back to [`ToolbarLayout::
//! drop_target`].
//!
//! The rules:
//! - the grip is a column of dots at the start of a horizontal bar and a
//!   row of dots on top of a vertical one; it is the only handle;
//! - a drop on a bar inserts before it, or after it past its middle (along
//!   the line); past the last bar of a line, at the end of that line;
//! - a drop within 80 px inside an area's inner edge (the window edge for
//!   an empty area) opens a new line on the inner side; nothing opens a
//!   line on the outer side: beyond the outermost line (over the menu
//!   bar) the bar floats, as anywhere else;
//! - a floating bar is horizontal whatever its area was, a grip and the
//!   buttons in a thin frame, no title, no close button; double-clicking
//!   it does nothing (so no "last docked" spot to go back to);
//! - only a menu hides a bar (Window ▸ Toolbars).

use std::collections::BTreeSet;
use xtrude_model::settings::{self, keys};

use serde_json::{json, Map, Value};

/// The bars the parts layout replaced (Modify became Model and Transform;
/// Sections, Solids and Panels joined other bars). A saved layout that
/// names one loads as the factory layout.
pub const REPLACED_BARS: [&str; 4] = ["modify", "sections", "solids", "panels"];

/// How far inside an area's inner edge a drop still
/// opens a new line there.
pub const NEW_LINE_REACH: f32 = 80.0;

/// Where a toolbar docks (the four toolbar areas).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolbarArea {
    Top,
    Bottom,
    Left,
    Right,
}

impl ToolbarArea {
    pub const ALL: [ToolbarArea; 4] = [
        ToolbarArea::Top,
        ToolbarArea::Bottom,
        ToolbarArea::Left,
        ToolbarArea::Right,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Left and right lines are columns.
    pub fn vertical(self) -> bool {
        matches!(self, ToolbarArea::Left | ToolbarArea::Right)
    }

    fn key(self) -> &'static str {
        match self {
            ToolbarArea::Top => "top",
            ToolbarArea::Bottom => "bottom",
            ToolbarArea::Left => "left",
            ToolbarArea::Right => "right",
        }
    }
}

/// One floating bar: its top-left in window points.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatingBar {
    pub id: String,
    pub pos: (f32, f32),
}

/// A docked bar's place: its area, line (0 = outermost) and index along
/// the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockSpot {
    pub area: ToolbarArea,
    pub line: usize,
    pub index: usize,
}

/// Where a dragged bar goes. Indices are those of the layout before the
/// bar is taken out ([`ToolbarLayout::place`] adjusts them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropTarget {
    /// Insert into an existing line at `index`.
    Into(DockSpot),
    /// Open a new line in `area` at `line` (0 = outermost).
    NewLine {
        area: ToolbarArea,
        line: usize,
    },
    Float {
        pos: (f32, f32),
    },
}

/// An axis-aligned rectangle in window points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: (f32, f32),
    pub max: (f32, f32),
}

impl Rect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect {
            min: (x0, y0),
            max: (x1, y1),
        }
    }

    pub fn contains(&self, p: (f32, f32)) -> bool {
        p.0 >= self.min.0 && p.0 < self.max.0 && p.1 >= self.min.1 && p.1 < self.max.1
    }
}

/// What the shell measured in the last frame: each area's rectangle (an
/// empty area is a zero-thickness rectangle at its window edge), each
/// drawn line and each drawn bar.
#[derive(Clone, Debug, Default)]
pub struct LayoutGeometry {
    pub areas: Vec<(ToolbarArea, Rect)>,
    pub lines: Vec<(ToolbarArea, usize, Rect)>,
    pub bars: Vec<(String, Rect)>,
}

/// The saved arrangement: per area, its lines
/// (rows for top/bottom, columns for left/right, outermost first), each
/// an ordered list of toolbar ids; the floating bars; the hidden ones.
///
/// It stores order and lines, never lengths, so a bar can not come back
/// with a stale length and packing has nothing to repair.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarLayout {
    /// Indexed by [`ToolbarArea::index`].
    pub lines: [Vec<Vec<String>>; 4],
    pub floating: Vec<FloatingBar>,
    pub hidden: BTreeSet<String>,
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

impl ToolbarLayout {
    /// The factory layout: one top line in the order of the work (it wraps
    /// into a second row in a narrow window), and Annotate hidden on the
    /// left, where it comes back when shown.
    pub fn factory() -> ToolbarLayout {
        ToolbarLayout {
            lines: [
                vec![ids(&[
                    "main",
                    "draw",
                    "model",
                    "transform",
                    "inspect",
                    "view",
                ])],
                Vec::new(),
                vec![ids(&["annotate"])],
                Vec::new(),
            ],
            floating: Vec::new(),
            hidden: ids(&["annotate"]).into_iter().collect(),
        }
    }

    /// The factory layout for the bars `known` (in creation order), as
    /// [`ToolbarLayout::load`] repairs a saved one: a known bar it misses
    /// joins the end of the innermost top line.
    pub fn factory_for(known: &[String]) -> ToolbarLayout {
        let mut layout = Self::factory();
        layout.repair(known);
        layout
    }

    /// The saved arrangement for the bars `known` (in creation order).
    /// Unknown ids (a bar this version no longer has) are dropped; a known
    /// bar missing from it goes to the end of the innermost top line, where
    /// a new bar goes. Anything unreadable gives the factory layout, and so
    /// does a layout that names a bar of [`REPLACED_BARS`]: their buttons
    /// moved to other bars, and dropping them would leave a muddle. That
    /// factory layout is saved at once, so it happens once.
    pub fn load(known: &[String]) -> ToolbarLayout {
        let saved = settings::stored(&keys::TOOLBAR_LAYOUT).and_then(|v| Self::from_json(&v));
        let mut layout = match saved {
            Some(l) if l.names_any(&REPLACED_BARS) => {
                let factory = Self::factory_for(known);
                factory.save();
                factory
            }
            Some(l) => l,
            None => Self::factory(),
        };
        layout.repair(known);
        layout
    }

    /// Whether any of `bars` is docked, floating or hidden in it.
    fn names_any(&self, bars: &[&str]) -> bool {
        let named = |id: &String| bars.contains(&id.as_str());
        self.lines.iter().flatten().flatten().any(named)
            || self.floating.iter().any(|f| named(&f.id))
            || self.hidden.iter().any(named)
    }

    /// Write the arrangement to the settings now (every change saves).
    pub fn save(&self) {
        settings::write(&keys::TOOLBAR_LAYOUT, &self.to_json());
        settings::sync();
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        for area in ToolbarArea::ALL {
            m.insert(area.key().into(), json!(self.lines[area.index()]));
        }
        let floating: Vec<Value> = self
            .floating
            .iter()
            .map(|f| json!({"id": f.id, "pos": [f.pos.0, f.pos.1]}))
            .collect();
        m.insert("floating".into(), Value::Array(floating));
        m.insert("hidden".into(), json!(self.hidden));
        Value::Object(m)
    }

    /// `None` when the value is not a layout (wrong shape or empty).
    pub fn from_json(v: &Value) -> Option<ToolbarLayout> {
        fn strings(v: &Value) -> Option<Vec<String>> {
            v.as_array()?
                .iter()
                .map(|s| s.as_str().map(String::from))
                .collect()
        }
        let m = v.as_object()?;
        let mut lines: [Vec<Vec<String>>; 4] = Default::default();
        for area in ToolbarArea::ALL {
            if let Some(a) = m.get(area.key()) {
                lines[area.index()] = a.as_array()?.iter().map(strings).collect::<Option<_>>()?;
            }
        }
        let mut floating = Vec::new();
        if let Some(f) = m.get("floating") {
            for bar in f.as_array()? {
                let id = bar.get("id")?.as_str()?.to_string();
                let pos = bar.get("pos")?.as_array()?;
                let (x, y) = (pos.first()?.as_f64()?, pos.get(1)?.as_f64()?);
                floating.push(FloatingBar {
                    id,
                    pos: (x as f32, y as f32),
                });
            }
        }
        let hidden = match m.get("hidden") {
            Some(h) => strings(h)?.into_iter().collect(),
            None => BTreeSet::new(),
        };
        let layout = ToolbarLayout {
            lines,
            floating,
            hidden,
        };
        let empty = layout.lines.iter().all(|a| a.is_empty()) && layout.floating.is_empty();
        (!empty).then_some(layout)
    }

    /// Keep the known ids exactly once each, drop empty lines, add the
    /// missing bars.
    fn repair(&mut self, known: &[String]) {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut keep = |id: &String| known.contains(id) && seen.insert(id.clone());
        for area in self.lines.iter_mut() {
            for line in area.iter_mut() {
                line.retain(&mut keep);
            }
            area.retain(|l| !l.is_empty());
        }
        self.floating.retain(|f| keep(&f.id));
        self.hidden.retain(|id| known.contains(id));
        let missing: Vec<String> = known
            .iter()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let top = &mut self.lines[ToolbarArea::Top.index()];
            if top.is_empty() {
                top.push(Vec::new());
            }
            if let Some(last) = top.last_mut() {
                last.extend(missing);
            }
        }
    }

    /// Where `id` is docked, `None` when it floats or is unknown.
    pub fn spot(&self, id: &str) -> Option<DockSpot> {
        for area in ToolbarArea::ALL {
            for (line, bars) in self.lines[area.index()].iter().enumerate() {
                if let Some(index) = bars.iter().position(|b| b == id) {
                    return Some(DockSpot { area, line, index });
                }
            }
        }
        None
    }

    pub fn is_floating(&self, id: &str) -> bool {
        self.floating.iter().any(|f| f.id == id)
    }

    pub fn is_shown(&self, id: &str) -> bool {
        !self.hidden.contains(id)
    }

    pub fn set_shown(&mut self, id: &str, shown: bool) {
        if shown {
            self.hidden.remove(id);
        } else {
            self.hidden.insert(id.to_string());
        }
    }

    /// Move `id` to `target`: out of its line (or out of the floating
    /// bars), an emptied line closing, then in at the target.
    pub fn place(&mut self, id: &str, target: DropTarget) {
        let from = self.spot(id);
        let was_floating = self.is_floating(id);
        if from.is_none() && !was_floating {
            return;
        }
        let mut target = target;
        if let Some(src) = from {
            let area_lines = &mut self.lines[src.area.index()];
            area_lines[src.line].remove(src.index);
            let emptied = area_lines[src.line].is_empty();
            if emptied {
                area_lines.remove(src.line);
            }
            target = match target {
                DropTarget::Into(mut spot) if spot.area == src.area => {
                    if spot.line == src.line {
                        if emptied {
                            // its own line, now gone: it comes back alone
                            return self.insert_line(src.area, src.line, id);
                        }
                        if spot.index > src.index {
                            spot.index -= 1;
                        }
                    } else if emptied && spot.line > src.line {
                        spot.line -= 1;
                    }
                    DropTarget::Into(spot)
                }
                DropTarget::NewLine { area, line } if area == src.area => {
                    let line = if emptied && line > src.line {
                        line - 1
                    } else {
                        line
                    };
                    DropTarget::NewLine { area, line }
                }
                t => t,
            };
        } else {
            self.floating.retain(|f| f.id != id);
        }
        match target {
            DropTarget::Into(spot) => match self.lines[spot.area.index()].get_mut(spot.line) {
                Some(line) => line.insert(spot.index.min(line.len()), id.to_string()),
                None => {
                    let n = self.lines[spot.area.index()].len();
                    self.insert_line(spot.area, n, id);
                }
            },
            DropTarget::NewLine { area, line } => self.insert_line(area, line, id),
            DropTarget::Float { pos } => self.floating.push(FloatingBar {
                id: id.to_string(),
                pos,
            }),
        }
    }

    fn insert_line(&mut self, area: ToolbarArea, line: usize, id: &str) {
        let lines = &mut self.lines[area.index()];
        lines.insert(line.min(lines.len()), vec![id.to_string()]);
    }

    /// Where a bar dragged with the pointer at `pointer`, held `grab` from
    /// its top-left, would go: on a
    /// line, before the bar under the pointer or after it past its middle,
    /// else at the line's end; within [`NEW_LINE_REACH`] inside an area's
    /// inner edge, a new innermost line there (the nearest area wins, in
    /// the order left, right, top, bottom); anywhere else it floats.
    pub fn drop_target(
        &self,
        geometry: &LayoutGeometry,
        pointer: (f32, f32),
        grab: (f32, f32),
    ) -> DropTarget {
        let (x, y) = pointer;
        for (area, rect) in &geometry.areas {
            if !rect.contains(pointer) {
                continue;
            }
            let vertical = area.vertical();
            let along = |p: (f32, f32)| if vertical { p.1 } else { p.0 };
            let across = |p: (f32, f32)| if vertical { p.0 } else { p.1 };
            // the line under the pointer, or the nearest one across
            let line = geometry
                .lines
                .iter()
                .filter(|(a, _, _)| a == area)
                .min_by(|(_, _, a), (_, _, b)| {
                    let d = |r: &Rect| {
                        let (lo, hi) = (across(r.min), across(r.max));
                        (lo - across(pointer)).max(across(pointer) - hi).max(0.0)
                    };
                    d(a).total_cmp(&d(b))
                })
                .map(|(_, l, _)| *l);
            let Some(line) = line.filter(|l| *l < self.lines[area.index()].len()) else {
                break;
            };
            let bars = &self.lines[area.index()][line];
            let mut drawn: Vec<(usize, &Rect)> = geometry
                .bars
                .iter()
                .filter_map(|(id, r)| bars.iter().position(|b| b == id).map(|i| (i, r)))
                .collect();
            drawn.sort_by(|a, b| along(a.1.min).total_cmp(&along(b.1.min)));
            let p = along(pointer);
            let index = drawn
                .iter()
                .find(|(_, r)| along(r.max) > p)
                .map(|(i, r)| {
                    if p > (along(r.min) + along(r.max)) / 2.0 {
                        i + 1
                    } else {
                        *i
                    }
                })
                .unwrap_or(bars.len());
            return DropTarget::Into(DockSpot {
                area: *area,
                line,
                index,
            });
        }
        let mut best: Option<(f32, ToolbarArea)> = None;
        for area in [
            ToolbarArea::Left,
            ToolbarArea::Right,
            ToolbarArea::Top,
            ToolbarArea::Bottom,
        ] {
            let Some((_, r)) = geometry.areas.iter().find(|(a, _)| *a == area) else {
                continue;
            };
            // how far the pointer is inside the area's inner edge
            let dist = match area {
                ToolbarArea::Left if y < r.max.1 => x - r.max.0,
                ToolbarArea::Right if y < r.max.1 => r.min.0 - x,
                ToolbarArea::Top if x < r.max.0 => y - r.max.1,
                ToolbarArea::Bottom if x < r.max.0 => r.min.1 - y,
                _ => continue,
            };
            if dist >= 0.0 && best.map_or(NEW_LINE_REACH, |b| b.0) > dist {
                best = Some((dist, area));
            }
        }
        match best {
            Some((_, area)) => DropTarget::NewLine {
                area,
                line: self.lines[area.index()].len(),
            },
            None => DropTarget::Float {
                pos: (x - grab.0, y - grab.1),
            },
        }
    }
}
