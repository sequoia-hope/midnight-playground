//! The call log's text encoding, as `tools/parity/lib/webaudio-fake.mjs`
//! writes it (`num`, `enc`): numbers in JavaScript's shortest round-trip
//! decimal (`Number.prototype.toString`), `-0` kept bare, non-finite numbers
//! as strings, `"$undefined"` for an explicit `undefined`.

use std::fmt::Write;

/// `Number.prototype.toString()` for a finite or non-finite double.
pub fn js_number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if x == 0.0 {
        return "0".into();
    }
    // Rust's `{:e}` gives the shortest round-trip length k. Where two
    // k-digit strings both round-trip (the value sits exactly between them,
    // as some float32 values do at 17 digits), ECMAScript takes the one
    // closest to the value, ties to even, and Rust's shortest printer may
    // take the other: so the digits are the value correctly rounded to k
    // digits (`{:.*e}` rounds the exact value half to even).
    let k = {
        let e = format!("{:e}", x.abs());
        let mant = e.split_once('e').expect("exponent").0;
        mant.chars().filter(|c| *c != '.').count()
    };
    let e = format!("{:.*e}", k - 1, x.abs());
    let (mant, exp) = e.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent digits");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1; // value = 0.digits × 10^n
    let mut s = String::new();
    if x < 0.0 {
        s.push('-');
    }
    if k <= n && n <= 21 {
        s.push_str(&digits);
        for _ in 0..(n - k) {
            s.push('0');
        }
    } else if 0 < n && n <= 21 {
        s.push_str(&digits[..n as usize]);
        s.push('.');
        s.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        s.push_str("0.");
        for _ in 0..(-n) {
            s.push('0');
        }
        s.push_str(&digits);
    } else {
        s.push_str(&digits[..1]);
        if k > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('e');
        let e1 = n - 1;
        s.push(if e1 < 0 { '-' } else { '+' });
        let _ = write!(s, "{}", e1.abs());
    }
    s
}

/// `num(x)` of the fake: a number in the log.
pub fn num(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        return "-0".into();
    }
    if x.is_finite() {
        return js_number(x);
    }
    json_string(&js_number(x))
}

/// `JSON.stringify(s)` for a string.
pub fn json_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A value in a log line (`enc`).
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Undefined,
    List(Vec<Val>),
    /// An object: keys in order; `Undefined` values are left out.
    Obj(Vec<(String, Val)>),
}

impl Val {
    pub fn str(s: impl Into<String>) -> Val {
        Val::Str(s.into())
    }

    pub fn encode(&self, out: &mut String) {
        match self {
            Val::Num(x) => out.push_str(&num(*x)),
            Val::Str(s) => out.push_str(&json_string(s)),
            Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Val::Null => out.push_str("null"),
            Val::Undefined => out.push_str("\"$undefined\""),
            Val::List(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.encode(out);
                }
                out.push(']');
            }
            Val::Obj(kv) => {
                out.push('{');
                let mut first = true;
                for (k, v) in kv {
                    if *v == Val::Undefined {
                        continue;
                    }
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    out.push_str(&json_string(k));
                    out.push(':');
                    v.encode(out);
                }
                out.push('}');
            }
        }
    }
}

impl From<f64> for Val {
    fn from(x: f64) -> Val {
        Val::Num(x)
    }
}

impl From<&str> for Val {
    fn from(s: &str) -> Val {
        Val::Str(s.into())
    }
}

impl From<String> for Val {
    fn from(s: String) -> Val {
        Val::Str(s)
    }
}

impl From<bool> for Val {
    fn from(b: bool) -> Val {
        Val::Bool(b)
    }
}

/// One log line: `[time,"op",...args]`.
pub fn line(time: f64, op: &str, args: &[Val]) -> String {
    let mut s = String::with_capacity(48);
    s.push('[');
    s.push_str(&num(time));
    s.push(',');
    s.push_str(&json_string(op));
    for a in args {
        s.push(',');
        a.encode(&mut s);
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn numbers_print_as_javascript_does() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (0.0000012345, "0.0000012345"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1.0 / 120.0, "0.008333333333333333"),
            (48000.0, "48000"),
            (2.5e-5, "0.000025"),
            (-3.4028234663852886e38, "-3.4028234663852886e+38"),
            // 1493.11187744140625 exactly (a float32), between two 17-digit
            // strings that both round-trip: ties to even, as V8.
            (1493.1118774414062, "1493.1118774414062"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for &(x, s) in cases {
            assert_eq!(js_number(x), s, "{x:e}");
        }
        assert_eq!(num(-0.0), "-0");
        assert_eq!(num(f64::NAN), "\"NaN\"");
    }

    #[test]
    fn lines_match_the_fake() {
        let l = line(
            0.5,
            "context",
            &[Val::Obj(vec![
                ("sampleRate".into(), Val::Num(48000.0)),
                ("latencyHint".into(), Val::Undefined),
            ])],
        );
        assert_eq!(l, r#"[0.5,"context",{"sampleRate":48000}]"#);
        let l = line(
            0.0,
            "data",
            &[Val::str("b1"), Val::List(vec![Val::str("#00ff")])],
        );
        assert_eq!(l, r##"[0,"data","b1",["#00ff"]]"##);
    }
}
