//! Canonical JSON, as `tools/music-lab/test/golden.mjs`'s `canon` writes
//! it: object keys sorted, `undefined` members dropped, numbers as JS
//! prints them (`String(n)`: integers without a fraction, otherwise the
//! shortest round-trip decimal; `-0` is `0`). A track or an event list
//! becomes a [`Val`] tree (`to_val`) and then this text, so its SHA-256 can
//! be compared with the lab's.

/// A JSON value with its object keys in insertion order.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

impl Val {
    /// An object from `(key, value)` pairs; a `None` value is a missing
    /// key, as `JSON.stringify` drops an `undefined` member.
    pub fn obj(pairs: Vec<(&str, Option<Val>)>) -> Val {
        Val::Obj(
            pairs
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| (k.to_owned(), v)))
                .collect(),
        )
    }
    pub fn num(x: f64) -> Option<Val> {
        Some(Val::Num(x))
    }
    pub fn onum(x: Option<f64>) -> Option<Val> {
        x.map(Val::Num)
    }
    pub fn str(s: &str) -> Option<Val> {
        Some(Val::Str(s.to_owned()))
    }
    pub fn ostr(s: &Option<String>) -> Option<Val> {
        s.as_ref().map(|s| Val::Str(s.clone()))
    }
    pub fn obool(b: Option<bool>) -> Option<Val> {
        b.map(Val::Bool)
    }
    /// An object of string pairs (a pattern table, a progression table).
    pub fn strs(pairs: &[(String, String)]) -> Val {
        Val::Obj(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), Val::Str(v.clone())))
                .collect(),
        )
    }
    /// An object of number pairs (`lay`).
    pub fn nums(pairs: &[(String, f64)]) -> Val {
        Val::Obj(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), Val::Num(*v)))
                .collect(),
        )
    }
    pub fn pair(p: &[f64; 2]) -> Val {
        Val::Arr(vec![Val::Num(p[0]), Val::Num(p[1])])
    }
}

/// `String(n)` for a finite double.
pub fn js_num(x: f64) -> String {
    if x == 0.0 {
        return "0".to_owned();
    }
    if x.fract() == 0.0 && x.abs() < 1e21 {
        return format!("{}", x as i64);
    }
    let a = x.abs();
    if a >= 1e21 || a < 1e-6 {
        // JS switches to exponent form here; Rust's `{:e}` prints the
        // shortest round-trip mantissa too, with the exponent as `e-7`.
        let s = format!("{:e}", x);
        let (m, e) = s.split_once('e').expect("an exponent");
        let e: i32 = e.parse().expect("an exponent");
        return format!("{m}e{}{}", if e < 0 { "-" } else { "+" }, e.abs());
    }
    // Rust's Display is the shortest decimal that round-trips, no exponent.
    format!("{}", x)
}

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The canonical text.
pub fn canon(v: &Val) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Val, out: &mut String) {
    match v {
        Val::Null => out.push_str("null"),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Num(x) => {
            if x.is_finite() {
                out.push_str(&js_num(*x));
            } else {
                out.push_str("null");
            }
        }
        Val::Str(s) => escape(s, out),
        Val::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Val::Obj(o) => {
            let mut keys: Vec<&(String, Val)> = o.iter().collect();
            // JS `sort()` compares UTF-16 code units; the keys are ASCII.
            keys.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (i, (k, x)) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                escape(k, out);
                out.push(':');
                write(x, out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_as_js_does() {
        assert_eq!(js_num(1.0), "1");
        assert_eq!(js_num(-0.0), "0");
        assert_eq!(js_num(0.75), "0.75");
        assert_eq!(js_num(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(js_num(124.0), "124");
        assert_eq!(js_num(-3.5), "-3.5");
        assert_eq!(js_num(1e-7), "1e-7");
        assert_eq!(js_num(1e21), "1e+21");
        assert_eq!(js_num(0.0001), "0.0001");
    }

    #[test]
    fn objects_sort_and_drop_missing() {
        let v = Val::obj(vec![
            ("b", Val::num(2.0)),
            ("a", None),
            ("c", Val::str("x\"y")),
            ("aa", Some(Val::Arr(vec![Val::Null, Val::Bool(true)]))),
        ]);
        assert_eq!(canon(&v), r#"{"aa":[null,true],"b":2,"c":"x\"y"}"#);
    }
}
