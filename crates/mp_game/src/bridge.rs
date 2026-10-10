//! `window.__mp`'s per-frame trees, built when they are read (DECISIONS
//! D1181).
//!
//! The race (`__mp.race`), the HUD, the start lamps, the pursuit, the
//! canvas UI's controls (`__mp.uiNodes`), the settings and the drop-downs'
//! choices are what the test bridge and the tools read. Built as JS objects
//! every frame they cost about 3.5 % of a phone's frame (some 80
//! `Reflect.set` calls with their keys decoded, and a JSON round trip),
//! for readers that look a few times a second at most. Instead each frame
//! keeps a plain Rust tree ([`V`]) of the same fields, and the key is an
//! accessor on `__mp` whose getter turns the latest tree into JS objects,
//! once per frame however often it is read (so `__mp.race === __mp.race`
//! within a frame, as before). The values are the last frame's, as they
//! were; an absent tree (no race) deletes the key, as before.

use js_sys::{Array, Object, Reflect};
use std::borrow::Cow;
use std::cell::RefCell;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::Closure;

/// A JS value as plain Rust data.
pub enum V {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    /// A u64 or i64, which wasm-bindgen gives JS as a BigInt.
    Big(i128),
    Str(Cow<'static, str>),
    Arr(Vec<V>),
    Obj(Obj),
    /// A JSON tree, as `JSON.parse` of its text would give it.
    Json(serde_json::Value),
}

/// A JS object's fields, in insertion order (as `Reflect.set` made them).
#[derive(Default)]
pub struct Obj(Vec<(Cow<'static, str>, V)>);

impl Obj {
    pub fn new() -> Obj {
        Obj(Vec::new())
    }

    /// Sets a field; a key set again keeps its place, as in JS.
    pub fn set(&mut self, k: impl Into<Cow<'static, str>>, v: impl Into<V>) {
        let k = k.into();
        let v = v.into();
        match self.0.iter_mut().find(|(key, _)| *key == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k, v)),
        }
    }

    /// A field that is itself an object.
    pub fn get_mut(&mut self, k: &str) -> Option<&mut Obj> {
        self.0.iter_mut().find_map(|(key, v)| match v {
            V::Obj(o) if key == k => Some(o),
            _ => None,
        })
    }
}

impl V {
    fn to_js(&self) -> JsValue {
        match self {
            V::Undefined => JsValue::UNDEFINED,
            V::Null => JsValue::NULL,
            V::Bool(b) => JsValue::from_bool(*b),
            V::Num(x) => JsValue::from_f64(*x),
            V::Big(x) => JsValue::from(*x),
            V::Str(s) => JsValue::from_str(s),
            V::Arr(a) => a.iter().map(V::to_js).collect::<Array>().into(),
            V::Obj(o) => {
                let js = Object::new();
                for (k, v) in &o.0 {
                    let _ = Reflect::set(&js, &JsValue::from_str(k), &v.to_js());
                }
                js.into()
            }
            V::Json(j) => json_to_js(j),
        }
    }
}

fn json_to_js(j: &serde_json::Value) -> JsValue {
    use serde_json::Value as J;
    match j {
        J::Null => JsValue::NULL,
        J::Bool(b) => JsValue::from_bool(*b),
        J::Number(n) => JsValue::from_f64(n.as_f64().unwrap_or(0.0)),
        J::String(s) => JsValue::from_str(s),
        J::Array(a) => a.iter().map(json_to_js).collect::<Array>().into(),
        J::Object(m) => {
            let js = Object::new();
            for (k, v) in m {
                let _ = Reflect::set(&js, &JsValue::from_str(k), &json_to_js(v));
            }
            js.into()
        }
    }
}

macro_rules! num {
    ($($t:ty),*) => {$(
        impl From<$t> for V {
            fn from(x: $t) -> V {
                V::Num(x as f64)
            }
        }
    )*};
}
num!(f64, f32, i8, i16, i32, u8, u16, u32, isize, usize);

impl From<u64> for V {
    fn from(x: u64) -> V {
        V::Big(i128::from(x))
    }
}

impl From<i64> for V {
    fn from(x: i64) -> V {
        V::Big(i128::from(x))
    }
}

impl From<bool> for V {
    fn from(b: bool) -> V {
        V::Bool(b)
    }
}

impl From<&str> for V {
    fn from(s: &str) -> V {
        V::Str(Cow::Owned(s.to_owned()))
    }
}

impl From<String> for V {
    fn from(s: String) -> V {
        V::Str(Cow::Owned(s))
    }
}

impl From<Obj> for V {
    fn from(o: Obj) -> V {
        V::Obj(o)
    }
}

impl From<Vec<V>> for V {
    fn from(a: Vec<V>) -> V {
        V::Arr(a)
    }
}

/// `null` for none.
impl<T: Into<V>> From<Option<T>> for V {
    fn from(x: Option<T>) -> V {
        x.map_or(V::Null, Into::into)
    }
}

struct Entry {
    key: &'static str,
    /// The latest tree, None while the key is absent.
    value: Option<V>,
    /// Its JS objects, once read.
    js: Option<JsValue>,
    /// Whether the accessor is on `__mp` now.
    defined: bool,
    /// The getter, kept alive while the page lives.
    _getter: Option<Closure<dyn Fn() -> JsValue>>,
}

thread_local! {
    static ENTRIES: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

fn mp() -> Option<Object> {
    use wasm_bindgen::JsCast;
    let w = web_sys::window()?;
    Reflect::get(&w, &JsValue::from_str("__mp"))
        .ok()?
        .dyn_into::<Object>()
        .ok()
}

/// What `__mp[key]` reads.
fn read(key: &'static str) -> JsValue {
    ENTRIES.with(|e| {
        // Read from inside a publish (it never is): nothing.
        let Ok(mut e) = e.try_borrow_mut() else {
            return JsValue::UNDEFINED;
        };
        let Some(en) = e.iter_mut().find(|en| en.key == key) else {
            return JsValue::UNDEFINED;
        };
        if en.js.is_none() {
            en.js = Some(en.value.as_ref().map_or(JsValue::UNDEFINED, V::to_js));
        }
        en.js.clone().unwrap_or(JsValue::UNDEFINED)
    })
}

/// This frame's `__mp[key]`, or None to delete the key.
pub fn publish(key: &'static str, value: Option<V>) {
    ENTRIES.with(|e| {
        let Ok(mut e) = e.try_borrow_mut() else {
            return;
        };
        let at = match e.iter().position(|en| en.key == key) {
            Some(i) => i,
            None => {
                e.push(Entry {
                    key,
                    value: None,
                    js: None,
                    defined: false,
                    _getter: None,
                });
                e.len() - 1
            }
        };
        let en = &mut e[at];
        let want = value.is_some();
        en.value = value;
        en.js = None;
        if want == en.defined {
            return;
        }
        let Some(mp) = mp() else { return };
        let k = JsValue::from_str(key);
        if want {
            let getter = en
                ._getter
                .get_or_insert_with(|| Closure::new(move || read(key)));
            let desc = Object::new();
            let _ = Reflect::set(&desc, &JsValue::from_str("get"), getter.as_ref());
            let _ = Reflect::set(&desc, &JsValue::from_str("configurable"), &JsValue::TRUE);
            let _ = Reflect::set(&desc, &JsValue::from_str("enumerable"), &JsValue::TRUE);
            // A data property of that name (none is ever made) goes first.
            let _ = Reflect::delete_property(&mp, &k);
            Object::define_property(&mp, &k, &desc);
        } else {
            let _ = Reflect::delete_property(&mp, &k);
        }
        en.defined = want;
    });
}
