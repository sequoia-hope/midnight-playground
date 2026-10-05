//! The viewer on the web: the race's pixel ratio, the page's scale and
//! safe-area insets, `window.__mr.viewer` for the tests (it reports the
//! mode, the pose and the panel's state, and `__mr.viewer.set({...})`,
//! which the page wires to [`viewer_set`], takes a pose), the panel's
//! controls in `__mr.uiNodes` as the menus' are, the screenshot as a
//! download, the link in the address bar and on the clipboard, and the
//! way to and from the menu.

use super::{Viewer, input::rect, panel::VControl};
use crate::status::Status;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use js_sys::{Function, Object, Reflect};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::prelude::*;

/// `__mr.viewer.set(json)` calls, in order.
static SETS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The link as last settled (the address), for the gesture bridge.
static LINK: Mutex<String> = Mutex::new(String::new());
/// The Copy link button's box (CSS px), for the gesture bridge.
static LINK_RECT: Mutex<Option<(f32, f32, f32, f32)>> = Mutex::new(None);
/// The gesture bridge copied the link inside the tap.
static COPIED: AtomicBool = AtomicBool::new(false);

fn mr() -> Option<Object> {
    let w = web_sys::window()?;
    Reflect::get(&w, &JsValue::from_str("__mr"))
        .ok()?
        .dyn_into::<Object>()
        .ok()
}

fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &JsValue::from_str(k), &v.into());
}

/// `__mr.viewer.set(o)`: a pose and the panel's state (`Viewer::apply`).
#[wasm_bindgen]
pub fn viewer_set(json: String) {
    SETS.lock().unwrap_or_else(|e| e.into_inner()).push(json);
}

/// The page's gesture handlers call this inside the event: a tap or
/// click on Copy link writes the link to the clipboard there, where
/// Safari allows it (a frame later it does not).
#[wasm_bindgen]
pub fn viewer_gesture(kind: &str, x: f64, y: f64) {
    if kind != "pointerup" && kind != "touchend" {
        return;
    }
    let Some((rx, ry, rw, rh)) = *LINK_RECT.lock().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };
    let (x, y) = (x as f32, y as f32);
    if x < rx || x > rx + rw || y < ry || y > ry + rh {
        return;
    }
    let q = LINK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if q.is_empty() || COPIED.swap(true, Ordering::Relaxed) {
        return;
    }
    clipboard(&q);
}

fn url_of(q: &str) -> String {
    let loc = web_sys::window().map(|w| w.location());
    let origin = loc
        .as_ref()
        .and_then(|l| l.origin().ok())
        .unwrap_or_default();
    let path = loc
        .as_ref()
        .and_then(|l| l.pathname().ok())
        .unwrap_or_default();
    format!("{origin}{path}?{q}")
}

fn clipboard(q: &str) {
    let f = Function::new_with_args(
        "u",
        "try { const c = navigator.clipboard; if (c && c.writeText) { c.writeText(u).catch(() => {}); return true; } } catch (e) {} return false;",
    );
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(&url_of(q)));
}

pub fn screenshot(name: &str) {
    crate::web::screenshot(name.to_owned());
}

/// The link on the clipboard (if the tap did not already put it there)
/// and in the address bar; what the panel says.
pub fn copy_link(q: &str) -> String {
    set_address(q);
    if !COPIED.swap(false, Ordering::Relaxed) {
        clipboard(q);
    }
    "Link copied (and in the address bar)".into()
}

/// The page's address follows the view (`history.replaceState`, after the
/// camera settles, so the address is always the view's link).
pub fn set_address(q: &str) {
    *LINK.lock().unwrap_or_else(|e| e.into_inner()) = q.to_owned();
    let f = Function::new_with_args(
        "u",
        "try { history.replaceState(history.state, '', u); } catch (e) {}",
    );
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(&format!("?{}", keep(q))));
}

/// The page's own parameters that are not the viewer's (`backend`,
/// `hq`, `world`, `stats`, …), kept across the viewer's links.
fn keep(q: &str) -> String {
    const VIEWER: &[&str] = &[
        "view",
        "level",
        "mode",
        "cam",
        "orbit",
        "s",
        "h",
        "back",
        "lat",
        "v",
        "yaw",
        "pitch",
        "fog",
        "far",
        "anim",
        "t",
        "hide",
        "speed",
        "panel",
        "autostart",
        "race",
    ];
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let mut out = vec![q.to_owned()];
    for (k, v) in crate::options::parse_query(&search) {
        if !VIEWER.contains(&k.as_str()) {
            out.push(format!(
                "{}={}",
                super::link::encode(&k),
                super::link::encode(&v)
            ));
        }
    }
    out.retain(|s| !s.is_empty());
    out.join("&")
}

/// The menu: the page without the viewer's parameters, the viewed level
/// saved as the menu's choice.
pub fn open_menu(level: &str) {
    crate::ui::store::Store::platform().set_str("level", level);
    let q = keep("");
    go(&if q.is_empty() {
        "?".to_owned()
    } else {
        format!("?{q}")
    });
}

/// The viewer on a level (the menu's button).
pub fn navigate(q: &str) {
    go(&format!("?{}", keep(q)));
}

fn go(href: &str) {
    if let Some(w) = web_sys::window() {
        let path = w.location().pathname().unwrap_or_default();
        let _ = w.location().set_href(&format!("{path}{href}"));
    }
}

/// At start: the race's pixel ratio (`hq ? min(devicePixelRatio, 1.5) :
/// 1`), so the viewer's frames cost what the race's do.
fn setup(opts: Res<crate::Opts>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
    let pr = if opts.hq { dpr.min(1.5) } else { 1.0 };
    if let Ok(mut w) = windows.single_mut()
        && (pr - dpr).abs() > 1e-3
    {
        w.resolution.set_scale_factor_override(Some(pr as f32));
    }
}

/// Each frame: the page's scale and insets in, `__mr.viewer.set` calls
/// applied.
fn frame_in(
    mut v: ResMut<Viewer>,
    tr: Res<crate::TrackRes>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if let Ok(w) = windows.single() {
        let css =
            f64::from(w.resolution.scale_factor() / w.resolution.base_scale_factor().max(0.01));
        if (css - v.css).abs() > 1e-4 {
            v.css = css;
            v.dirty = true;
        }
    }
    if let Some(mr) = mr()
        && let Ok(ins) = Reflect::get(&mr, &JsValue::from_str("insets"))
        && ins.is_object()
    {
        let n = |k: &str| {
            Reflect::get(&ins, &JsValue::from_str(k))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32
        };
        let i = [n("top"), n("right"), n("bottom"), n("left")];
        if i != v.insets {
            v.insets = i;
            v.dirty = true;
        }
    }
    let Some(track) = &tr.track else { return };
    let sets: Vec<String> = std::mem::take(&mut *SETS.lock().unwrap_or_else(|e| e.into_inner()));
    for s in sets {
        match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(j) => v.apply(&j, track),
            Err(e) => warn!("__mr.viewer.set: {e}"),
        }
    }
}

/// `__mr.screen`, `__mr.mode`, `__mr.viewer` and `__mr.uiNodes`.
#[allow(clippy::type_complexity)]
fn publish(
    v: Res<Viewer>,
    status: Res<Status>,
    controls: Query<(
        &VControl,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    mut last: Local<String>,
) {
    let Some(mr) = mr() else { return };
    set(&mr, "screen", "viewer");
    set(&mr, "mode", "viewer");
    let css = v.css as f32;
    let mut nodes = serde_json::Map::new();
    let mut link_rect = None;
    for (c, n, t, vis) in &controls {
        let r = rect(n, t);
        let (x, y, w, h) = (
            r.min.x * css,
            r.min.y * css,
            r.width() * css,
            r.height() * css,
        );
        if c.id == "vw-link" && vis.get() {
            link_rect = Some((x, y, w, h));
        }
        nodes.insert(
            c.id.clone(),
            serde_json::json!({
                "x": x, "y": y, "w": w, "h": h, "visible": vis.get(), "enabled": c.act.is_some(),
                "value": c.value, "sel": c.sel, "z": 0,
            }),
        );
    }
    *LINK_RECT.lock().unwrap_or_else(|e| e.into_inner()) = link_rect;
    let json = serde_json::Value::Object(nodes).to_string();
    if *last != json {
        if let Ok(o) = js_sys::JSON::parse(&json) {
            set(&mr, "uiNodes", o);
        }
        *last = json;
    }
    let state = serde_json::to_string(&v.report(&status)).unwrap_or_default();
    let Ok(o) = Reflect::get(&mr, &JsValue::from_str("viewer")) else {
        return;
    };
    let Some(o) = o.dyn_ref::<Object>() else {
        return;
    };
    if let Ok(r) = js_sys::JSON::parse(&state)
        && let Some(r) = r.dyn_ref::<Object>()
    {
        let _ = Object::assign(o, r);
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(Update, frame_in.before(super::input::gather))
        .add_systems(Last, publish);
}
