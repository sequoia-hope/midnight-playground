//! Numbers that may not be finite. JSON has no Infinity or NaN, so the file
//! writes them as `{"num": "Infinity"}`, `{"num": "-Infinity"}` or
//! `{"num": "NaN"}` (JavaScript's `String(x)`), and plain numbers otherwise.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// One number, in its JSON form.
pub(crate) fn to_value(x: f64) -> Value {
    if x.is_finite() {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    } else {
        let s = if x.is_nan() {
            "NaN"
        } else if x > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        };
        serde_json::json!({ "num": s })
    }
}

pub(crate) fn from_value(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Object(o) if o.len() == 1 => match o.get("num")?.as_str()? {
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            "NaN" => Some(f64::NAN),
            _ => None,
        },
        _ => None,
    }
}

fn de_seq<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
    let raw = Vec::<Value>::deserialize(d)?;
    raw.iter()
        .map(|v| from_value(v).ok_or_else(|| D::Error::custom(format!("not a number: {v}"))))
        .collect()
}

fn ser_seq<S: Serializer>(v: &[f64], s: S) -> Result<S::Ok, S::Error> {
    v.iter()
        .map(|&x| to_value(x))
        .collect::<Vec<_>>()
        .serialize(s)
}

pub(crate) mod one {
    use super::*;
    pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        to_value(*v).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        let v = Value::deserialize(d)?;
        from_value(&v).ok_or_else(|| D::Error::custom(format!("not a number: {v}")))
    }
}

pub(crate) mod opt_seq {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<Vec<f64>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => ser_seq(v, s),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<f64>>, D::Error> {
        let v = Option::<Value>::deserialize(d)?;
        match v {
            None | Some(Value::Null) => Ok(None),
            Some(v) => de_seq(v).map(Some).map_err(D::Error::custom),
        }
    }
}

pub(crate) mod arr16 {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[f64; 16], s: S) -> Result<S::Ok, S::Error> {
        ser_seq(v, s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f64; 16], D::Error> {
        let v = de_seq(d)?;
        v.try_into().map_err(|v: Vec<f64>| {
            D::Error::custom(format!("expected 16 numbers, got {}", v.len()))
        })
    }
}
