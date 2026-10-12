// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The typed entries of a form ([`super::Dialogs::ask_form`]): a value as
//! the text its entry shows, and the text the user typed read back.
//!
//! A length is typed as in the value box (`12`, `1.5cm`, `3/4"`): a
//! bare number is in the document's unit, and the value is in model units.
//! A whole number and a plain number take a decimal comma too.

use xtrude_ext::{ParamKind, ParamSpec, ParamValue};
use xtrude_model::i18n::tr;
use xtrude_model::units;

use crate::value_box;

/// The text an entry shows for `value` (a whole number, a number or a
/// length; empty for a check box or a choice).
pub fn field_text(value: &ParamValue) -> String {
    match value {
        ParamValue::Int(v) => v.to_string(),
        ParamValue::Number(v) | ParamValue::Length(v) => v.to_string(),
        ParamValue::Bool(_) | ParamValue::Choice(_) => String::new(),
    }
}

/// The value typed in the entry of `spec`, checked against its bounds; the
/// error says what is wrong. A check box or a choice has no text: its
/// value is `current`, checked.
pub fn parse_field(
    spec: &ParamSpec,
    text: &str,
    current: &ParamValue,
) -> Result<ParamValue, String> {
    let text = text.trim();
    let not_a = |what: &str| {
        tr(
            "{label}: '{text}' is not {what}.",
            &[("label", &spec.label), ("text", &text), ("what", &what)],
        )
    };
    let value = match &spec.kind {
        ParamKind::Int { .. } => ParamValue::Int(
            text.parse::<i64>()
                .map_err(|_| not_a(&tr("a whole number", &[])))?,
        ),
        ParamKind::Number { .. } => ParamValue::Number(
            text.replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| not_a(&tr("a number", &[])))?,
        ),
        ParamKind::Length { .. } => {
            let metres = value_box::parse_length_field(&text.replace(',', "."));
            let unit = units::bare_scale_of(&units::model_unit()).unwrap_or(1.0);
            ParamValue::Length(metres.ok_or_else(|| not_a(&tr("a length", &[])))? / unit)
        }
        ParamKind::Bool | ParamKind::Choice(_) => current.clone(),
    };
    spec.check(&value)?;
    Ok(value)
}
