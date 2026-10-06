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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finite_numbers_are_plain_and_the_rest_are_tagged() {
        assert_eq!(to_value(1.5), json!(1.5));
        assert_eq!(to_value(f64::INFINITY), json!({ "num": "Infinity" }));
        assert_eq!(to_value(f64::NEG_INFINITY), json!({ "num": "-Infinity" }));
        assert_eq!(to_value(f64::NAN), json!({ "num": "NaN" }));
        assert_eq!(to_value(-f64::NAN), json!({ "num": "NaN" }));
        // -0 stays a negative zero through the JSON text.
        let text = serde_json::to_string(&to_value(-0.0)).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        let z = from_value(&back).unwrap();
        assert!(z == 0.0 && z.is_sign_negative(), "{text}");
    }

    #[test]
    fn every_value_reads_back_bit_for_bit() {
        for x in [
            0.0,
            -0.0,
            0.1,
            -1e300,
            5e-324,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let text = serde_json::to_string(&to_value(x)).unwrap();
            let v: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(from_value(&v).unwrap().to_bits(), x.to_bits(), "{text}");
        }
        assert!(from_value(&to_value(f64::NAN)).unwrap().is_nan());
    }

    #[test]
    fn anything_else_is_not_a_number() {
        for v in [
            json!(null),
            json!(true),
            json!("1"),
            json!([1]),
            json!({}),
            json!({ "num": "inf" }),
            json!({ "num": "infinity" }),
            json!({ "num": "nan" }),
            json!({ "num": 1 }),
            json!({ "num": "NaN", "other": 0 }),
            json!({ "value": "NaN" }),
        ] {
            assert_eq!(from_value(&v), None, "{v}");
        }
        // Integers, including ones past 2^53, read as the nearest f64.
        assert_eq!(from_value(&json!(u64::MAX)), Some(u64::MAX as f64));
        assert_eq!(from_value(&json!(-3)), Some(-3.0));
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Holder {
        #[serde(with = "opt_seq")]
        seq: Option<Vec<f64>>,
        #[serde(with = "arr16")]
        m: [f64; 16],
        #[serde(with = "one")]
        x: f64,
    }

    #[test]
    fn the_field_adapters_round_trip_and_name_what_is_wrong() {
        let z16 = Value::from(vec![0; 16]);
        let z15 = Value::from(vec![0; 15]);
        let h = Holder {
            seq: Some(vec![1.0, f64::INFINITY]),
            m: [f64::NEG_INFINITY; 16],
            x: f64::INFINITY,
        };
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["seq"], json!([1.0, { "num": "Infinity" }]));
        let back: Holder = serde_json::from_value(v).unwrap();
        assert_eq!(back.seq, h.seq);
        assert_eq!(back.m, h.m);
        assert_eq!(back.x, h.x);

        let none: Holder =
            serde_json::from_value(json!({ "seq": null, "m": z16, "x": 0 })).unwrap();
        assert_eq!(none.seq, None);
        assert_eq!(
            serde_json::to_value(&none).unwrap()["seq"],
            Value::Null,
            "None is written as null"
        );

        let e = serde_json::from_value::<Holder>(json!({ "seq": [], "m": z15, "x": 0 }))
            .unwrap_err()
            .to_string();
        assert!(e.contains("expected 16 numbers, got 15"), "{e}");
        let e = serde_json::from_value::<Holder>(json!({ "seq": ["x"], "m": z16, "x": 0 }))
            .unwrap_err()
            .to_string();
        assert!(e.contains("not a number"), "{e}");
        assert!(
            serde_json::from_value::<Holder>(json!({ "seq": [], "m": z16, "x": null })).is_err()
        );
    }
}
