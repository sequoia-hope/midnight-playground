//! The section builder's worker on the web (docs/vision/peninsula.md
//! stage 4c; DECISIONS D1215): `mp_builder` as its own wasm behind
//! `web/builder-worker.js`, planning a streamed level and building its
//! sections off the main thread, each handed back packed
//! (`mp_worldgen::section::unpack` reads it). `crate::stream` asks and
//! collects; this module is the messages.
//!
//! One worker for the page's life, started when the first streamed level
//! is asked for (`?worker=0` never starts it). A level's requests carry a
//! generation, so a result of the level before is dropped. The worker's
//! wasm failing to load, or trapping, fails every request after; the
//! stream then builds the sections itself, stepped between frames.

use std::cell::RefCell;
use std::sync::Mutex;

use bevy::log::warn;
use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;
use web_sys::MessageEvent;

/// What the worker answered.
pub enum Msg {
    Planned(u64),
    Built {
        generation: u64,
        k: i64,
        /// The worker's own time for the build.
        ms: f64,
        bytes: Vec<u8>,
    },
    Failed {
        generation: u64,
        /// None: the plan failed (or the worker is gone).
        k: Option<i64>,
        error: String,
    },
}

struct Handle {
    worker: web_sys::Worker,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}

thread_local! {
    static WORKER: RefCell<Option<Result<Handle, String>>> = const { RefCell::new(None) };
}
static INBOX: Mutex<Vec<Msg>> = Mutex::new(Vec::new());

fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
}

fn num(o: &JsValue, k: &str) -> f64 {
    get(o, k).as_f64().unwrap_or(0.0)
}

fn push(m: Msg) {
    INBOX.lock().unwrap_or_else(|e| e.into_inner()).push(m);
}

fn start() -> Result<Handle, String> {
    let worker = web_sys::Worker::new("builder-worker.js")
        .map_err(|e| format!("starting the builder's worker: {e:?}"))?;
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let d = e.data();
        let generation = num(&d, "generation") as u64;
        match get(&d, "type").as_string().as_deref() {
            Some("planned") => push(Msg::Planned(generation)),
            Some("built") => {
                let bytes = js_sys::Uint8Array::new(&get(&d, "bytes")).to_vec();
                push(Msg::Built {
                    generation,
                    k: num(&d, "k") as i64,
                    ms: num(&d, "ms"),
                    bytes,
                });
            }
            Some("failed") => push(Msg::Failed {
                generation,
                k: get(&d, "k").as_f64().map(|k| k as i64),
                error: get(&d, "error")
                    .as_string()
                    .unwrap_or_else(|| "unknown".into()),
            }),
            _ => {}
        }
    });
    let on_error = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |e: web_sys::ErrorEvent| {
        // The script itself failed (not found, a syntax error): every
        // generation's requests are off.
        push(Msg::Failed {
            generation: 0,
            k: None,
            error: format!("builder-worker.js: {}", e.message()),
        });
        *GONE.lock().unwrap_or_else(|e| e.into_inner()) = true;
    });
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    Ok(Handle {
        worker,
        _on_message: on_message,
        _on_error: on_error,
    })
}

/// The worker's script failed: nothing more from it.
static GONE: Mutex<bool> = Mutex::new(false);

/// Whether the worker is there to ask (started on the first call).
pub fn available() -> bool {
    if *GONE.lock().unwrap_or_else(|e| e.into_inner()) {
        return false;
    }
    WORKER.with(|w| {
        let mut w = w.borrow_mut();
        if w.is_none() {
            let r = start();
            if let Err(e) = &r {
                warn!("stream: {e}");
            }
            *w = Some(r);
        }
        w.as_ref().is_some_and(|r| r.is_ok())
    })
}

fn post(msg: &Object) {
    WORKER.with(|w| {
        if let Some(Ok(h)) = w.borrow().as_ref()
            && let Err(e) = h.worker.post_message(msg)
        {
            warn!("stream: posting to the builder's worker: {e:?}");
        }
    });
}

fn message(kind: &str, generation: u64) -> Object {
    let o = Object::new();
    let _ = Reflect::set(&o, &JsValue::from_str("type"), &JsValue::from_str(kind));
    let _ = Reflect::set(
        &o,
        &JsValue::from_str("generation"),
        &JsValue::from_f64(generation as f64),
    );
    o
}

/// Asks the worker to plan `level` (for this generation of the stream).
pub fn plan(generation: u64, level: &str) {
    let o = message("plan", generation);
    let _ = Reflect::set(&o, &JsValue::from_str("level"), &JsValue::from_str(level));
    post(&o);
}

/// Asks for section `k`.
pub fn build(generation: u64, k: i64) {
    let o = message("build", generation);
    let _ = Reflect::set(&o, &JsValue::from_str("k"), &JsValue::from_f64(k as f64));
    post(&o);
}

/// What the worker has answered since the last call.
pub fn take() -> Vec<Msg> {
    std::mem::take(&mut *INBOX.lock().unwrap_or_else(|e| e.into_inner()))
}
