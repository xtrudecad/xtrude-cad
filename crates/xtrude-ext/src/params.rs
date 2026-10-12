// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A command's parameters: what the manifest declares, the values the
//! user gives, and how they reach Lua and the settings.
//!
//! A parameter is `{ key, label, type, default, min, max, choices }`, with
//! `type` one of `int`, `number`, `length` (in model units), `bool` and
//! `choice` (one of the `choices` strings). The values reach the `run`
//! function as a table by `key`: integers, numbers, booleans, and the
//! chosen string.

use std::collections::HashSet;

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;
use xtrude_model::i18n::tr;

use crate::error::{ErrorKind, ExtError};

/// The type of a parameter, with its bounds.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamKind {
    Int {
        min: Option<i64>,
        max: Option<i64>,
    },
    Number {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// A length in model units.
    Length {
        min: Option<f64>,
        max: Option<f64>,
    },
    Bool,
    Choice(Vec<String>),
}

/// A parameter's value.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Int(i64),
    Number(f64),
    /// In model units.
    Length(f64),
    Bool(bool),
    /// The index of the choice.
    Choice(usize),
}

/// One parameter, as the manifest declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSpec {
    pub key: String,
    pub label: String,
    pub kind: ParamKind,
    pub default: ParamValue,
}

impl ParamSpec {
    /// Whether `value` is of this parameter's type and within its bounds;
    /// the error says why not.
    pub fn check(&self, value: &ParamValue) -> Result<(), String> {
        let label = &self.label;
        let out_of_range = |min: String, max: String| {
            Err(tr(
                "{label} must be between {min} and {max}.",
                &[("label", label), ("min", &min), ("max", &max)],
            ))
        };
        let shown = |b: Option<String>, dash: &str| b.unwrap_or_else(|| dash.to_string());
        match (&self.kind, value) {
            (ParamKind::Int { min, max }, ParamValue::Int(v)) => {
                if min.is_some_and(|m| *v < m) || max.is_some_and(|m| *v > m) {
                    return out_of_range(
                        shown(min.map(|m| m.to_string()), "−∞"),
                        shown(max.map(|m| m.to_string()), "∞"),
                    );
                }
                Ok(())
            }
            (ParamKind::Number { min, max }, ParamValue::Number(v))
            | (ParamKind::Length { min, max }, ParamValue::Length(v)) => {
                if !v.is_finite() {
                    return Err(tr("{label} must be a number.", &[("label", label)]));
                }
                if min.is_some_and(|m| *v < m) || max.is_some_and(|m| *v > m) {
                    return out_of_range(
                        shown(min.map(|m| m.to_string()), "−∞"),
                        shown(max.map(|m| m.to_string()), "∞"),
                    );
                }
                Ok(())
            }
            (ParamKind::Bool, ParamValue::Bool(_)) => Ok(()),
            (ParamKind::Choice(items), ParamValue::Choice(i)) if *i < items.len() => Ok(()),
            _ => Err(tr(
                "{label} has a value of the wrong type.",
                &[("label", label)],
            )),
        }
    }

    /// The value as Lua sees it.
    pub fn to_lua(&self, lua: &Lua, value: &ParamValue) -> mlua::Result<Value> {
        Ok(match value {
            ParamValue::Int(v) => Value::Integer(*v),
            ParamValue::Number(v) | ParamValue::Length(v) => Value::Number(*v),
            ParamValue::Bool(v) => Value::Boolean(*v),
            ParamValue::Choice(i) => match &self.kind {
                ParamKind::Choice(items) => match items.get(*i) {
                    Some(s) => Value::String(lua.create_string(s)?),
                    None => Value::Nil,
                },
                _ => Value::Nil,
            },
        })
    }

    /// The value as the settings keep it: a length in metres
    /// (`metres_per_unit` is the document's model unit), a choice by its
    /// text, so that the stored value outlives a change of unit or of the
    /// choices' order.
    pub fn to_json(&self, value: &ParamValue, metres_per_unit: f64) -> Json {
        match value {
            ParamValue::Int(v) => Json::from(*v),
            ParamValue::Number(v) => Json::from(*v),
            ParamValue::Length(v) => Json::from(*v * metres_per_unit),
            ParamValue::Bool(v) => Json::from(*v),
            ParamValue::Choice(i) => match &self.kind {
                ParamKind::Choice(items) => items.get(*i).map_or(Json::Null, |s| Json::from(&**s)),
                _ => Json::Null,
            },
        }
    }

    /// A stored value read back, if it is still a valid value of this
    /// parameter.
    pub fn from_json(&self, json: &Json, metres_per_unit: f64) -> Option<ParamValue> {
        let value = match &self.kind {
            ParamKind::Int { .. } => ParamValue::Int(json.as_i64()?),
            ParamKind::Number { .. } => ParamValue::Number(json.as_f64()?),
            ParamKind::Length { .. } => {
                if metres_per_unit <= 0.0 {
                    return None;
                }
                ParamValue::Length(json.as_f64()? / metres_per_unit)
            }
            ParamKind::Bool => ParamValue::Bool(json.as_bool()?),
            ParamKind::Choice(items) => {
                let text = json.as_str()?;
                ParamValue::Choice(items.iter().position(|s| s == text)?)
            }
        };
        self.check(&value).ok().map(|()| value)
    }
}

/// The values as the `run` function's parameter table.
pub fn params_table(lua: &Lua, specs: &[ParamSpec], values: &[ParamValue]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (spec, value) in specs.iter().zip(values) {
        table.set(spec.key.as_str(), spec.to_lua(lua, value)?)?;
    }
    Ok(table)
}

/// The command `command`'s `params` list.
pub(crate) fn parse_specs(command: &str, list: &Table) -> Result<Vec<ParamSpec>, ExtError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in list.clone().sequence_values::<Value>() {
        let entry = entry.map_err(|e| invalid(command, e.to_string()))?;
        let Value::Table(entry) = entry else {
            return Err(invalid(command, tr("each parameter must be a table", &[])));
        };
        let spec = parse_spec(command, &entry)?;
        if !seen.insert(spec.key.clone()) {
            return Err(invalid(
                command,
                tr("two parameters have the key '{key}'", &[("key", &spec.key)]),
            ));
        }
        out.push(spec);
    }
    Ok(out)
}

fn parse_spec(command: &str, t: &Table) -> Result<ParamSpec, ExtError> {
    let field = |name: &str| {
        t.raw_get::<Value>(name)
            .map_err(|e| invalid(command, e.to_string()))
    };
    let text = |name: &str| -> Result<Option<String>, ExtError> {
        match field(name)? {
            Value::Nil => Ok(None),
            Value::String(s) => Ok(Some(s.to_string_lossy())),
            _ => Err(invalid(
                command,
                tr(
                    "the parameter field '{name}' must be a string",
                    &[("name", &name)],
                ),
            )),
        }
    };
    let Some(key) = text("key")?.filter(|k| !k.is_empty()) else {
        return Err(invalid(command, tr("a parameter needs a 'key'", &[])));
    };
    let fail = |message: String| invalid(command, format!("{key}: {message}"));
    let label = text("label")?.unwrap_or_else(|| key.clone());
    let kind_name = text("type")?.unwrap_or_default();
    let number = |name: &str| -> Result<Option<f64>, ExtError> {
        match field(name)? {
            Value::Nil => Ok(None),
            Value::Integer(n) => Ok(Some(n as f64)),
            Value::Number(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(fail(tr("'{name}' must be a number", &[("name", &name)]))),
        }
    };
    let integer = |name: &str| -> Result<Option<i64>, ExtError> {
        match field(name)? {
            Value::Nil => Ok(None),
            Value::Integer(n) => Ok(Some(n)),
            Value::Number(n) if n.fract() == 0.0 && n.abs() < 9e15 => Ok(Some(n as i64)),
            _ => Err(fail(tr(
                "'{name}' must be a whole number",
                &[("name", &name)],
            ))),
        }
    };
    let (kind, default) = match kind_name.as_str() {
        "int" => {
            let (min, max) = (integer("min")?, integer("max")?);
            let default = integer("default")?.unwrap_or(min.unwrap_or(0));
            (ParamKind::Int { min, max }, ParamValue::Int(default))
        }
        "number" | "length" => {
            let (min, max) = (number("min")?, number("max")?);
            let default = number("default")?.unwrap_or(min.unwrap_or(0.0));
            if kind_name == "number" {
                (ParamKind::Number { min, max }, ParamValue::Number(default))
            } else {
                (ParamKind::Length { min, max }, ParamValue::Length(default))
            }
        }
        "bool" => {
            let default = match field("default")? {
                Value::Nil => false,
                Value::Boolean(b) => b,
                _ => return Err(fail(tr("'default' must be true or false", &[]))),
            };
            (ParamKind::Bool, ParamValue::Bool(default))
        }
        "choice" => {
            let Value::Table(list) = field("choices")? else {
                return Err(fail(tr("a choice needs a list of 'choices'", &[])));
            };
            let mut items = Vec::new();
            for item in list.sequence_values::<Value>() {
                match item.map_err(|e| fail(e.to_string()))? {
                    Value::String(s) => items.push(s.to_string_lossy()),
                    _ => return Err(fail(tr("each choice must be a string", &[]))),
                }
            }
            if items.is_empty() {
                return Err(fail(tr("a choice needs a list of 'choices'", &[])));
            }
            let default = match text("default")? {
                None => 0,
                Some(d) => items
                    .iter()
                    .position(|s| *s == d)
                    .ok_or_else(|| fail(tr("the default '{d}' is not a choice", &[("d", &d)])))?,
            };
            (ParamKind::Choice(items), ParamValue::Choice(default))
        }
        other => {
            return Err(fail(tr(
                "unknown type '{type}' (int, number, length, bool or choice)",
                &[("type", &other)],
            )))
        }
    };
    let spec = ParamSpec {
        key: key.clone(),
        label,
        kind,
        default,
    };
    spec.check(&spec.default)
        .map_err(|e| fail(tr("the default is not valid: {e}", &[("e", &e)])))?;
    Ok(spec)
}

fn invalid(command: &str, message: String) -> ExtError {
    ExtError::new(
        ErrorKind::Manifest,
        tr(
            "The parameters of command '{command}': {message}",
            &[("command", &command), ("message", &message)],
        ),
    )
}
