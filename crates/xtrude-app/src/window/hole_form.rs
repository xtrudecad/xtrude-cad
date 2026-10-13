// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The Hole tool's form (Tab in the tool): the whole hole — diameter,
//! depth, head — as a form ([`super::Dialogs::ask_form`]). Lengths are in
//! model units, as the form reads them; a depth of 0 is through.

use xtrude_core::hole::{Depth, Head, HoleSpec, COUNTERSINK_DEG};
use xtrude_ext::{ParamKind, ParamSpec, ParamValue};
use xtrude_model::i18n::tr;
use xtrude_model::units;

use super::{Asker, MainWindow};
use crate::tools::hole::{describe, hole_spec, set_hole_spec};

/// The heads, in the order of the form's choice.
pub const HEADS: [&str; 3] = ["None", "Countersink", "Counterbore"];

/// Metres per model unit.
fn unit_scale() -> f64 {
    units::bare_scale_of(&units::model_unit()).unwrap_or(1.0)
}

fn length(key: &str, label: &str, metres: f64, scale: f64) -> ParamSpec {
    ParamSpec {
        key: key.into(),
        label: tr(label, &[]),
        kind: ParamKind::Length {
            min: Some(0.0),
            max: None,
        },
        default: ParamValue::Length(metres / scale),
    }
}

/// The form's fields for `spec`, `scale` metres per model unit; a head
/// the spec has not got offers twice the bore, 90° and the bore's depth.
pub fn fields(spec: &HoleSpec, scale: f64) -> Vec<ParamSpec> {
    let d = spec.diameter;
    let (head, head_d, angle, bore_depth) = match spec.head {
        Head::None => (0, 2.0 * d, COUNTERSINK_DEG, d),
        Head::Countersink {
            diameter,
            angle_deg,
        } => (1, diameter, angle_deg, d),
        Head::Counterbore { diameter, depth } => (2, diameter, COUNTERSINK_DEG, depth),
    };
    let depth = match spec.depth {
        Depth::Through => 0.0,
        Depth::Blind(x) => x,
    };
    vec![
        length("diameter", "Diameter", d, scale),
        length("depth", "Depth (0 = through)", depth, scale),
        ParamSpec {
            key: "head".into(),
            label: tr("Head", &[]),
            kind: ParamKind::Choice(HEADS.iter().map(|h| tr(h, &[])).collect()),
            default: ParamValue::Choice(head),
        },
        length("head_diameter", "Head diameter", head_d, scale),
        ParamSpec {
            key: "countersink_angle".into(),
            label: tr("Countersink angle (°)", &[]),
            kind: ParamKind::Number {
                min: Some(1.0),
                max: Some(179.0),
            },
            default: ParamValue::Number(angle),
        },
        length("counterbore_depth", "Counterbore depth", bore_depth, scale),
    ]
}

/// The hole the form's answer describes, or `None` when the answer does
/// not fit the fields.
pub fn spec_from(values: &[ParamValue], scale: f64) -> Option<HoleSpec> {
    let len = |i: usize| match values.get(i) {
        Some(ParamValue::Length(v)) => Some(v * scale),
        _ => None,
    };
    let depth = len(1)?;
    let head = match values.get(2)? {
        ParamValue::Choice(0) => Head::None,
        ParamValue::Choice(1) => Head::Countersink {
            diameter: len(3)?,
            angle_deg: match values.get(4)? {
                ParamValue::Number(a) => *a,
                _ => return None,
            },
        },
        ParamValue::Choice(2) => Head::Counterbore {
            diameter: len(3)?,
            depth: len(5)?,
        },
        _ => return None,
    };
    Some(HoleSpec {
        diameter: len(0)?,
        depth: if depth > 0.0 {
            Depth::Blind(depth)
        } else {
            Depth::Through
        },
        head,
    })
}

impl MainWindow {
    /// Ask for the whole hole (the Hole tool's Tab).
    pub fn prompt_hole_spec(&mut self) {
        self.ask_as(Asker::HoleSpec, |w| w.ask_hole_spec());
    }

    fn ask_hole_spec(&mut self) {
        let scale = unit_scale();
        let fields = fields(&hole_spec(), scale);
        let offered: Vec<ParamValue> = fields.iter().map(|f| f.default.clone()).collect();
        let Some(given) = self.dialogs.ask_form(&tr("Hole", &[]), &fields, &offered) else {
            return;
        };
        let Some(spec) = spec_from(&given, scale) else {
            return;
        };
        set_hole_spec(spec);
        let msg = tr("Hole {hole} — click a face.", &[("hole", &describe(&spec))]);
        self.show_message(&msg, 3000);
        self.viewport.update();
    }
}
