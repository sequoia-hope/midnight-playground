//! The client's WebSocket in the browser: `new WebSocket(url)` with binary
//! messages as ArrayBuffers; its callbacks queue events for `poll`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use mp_net::transport::{Channel, NetEvent, PeerId, Transport};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, WebSocket};

type Queue = Rc<RefCell<VecDeque<NetEvent>>>;

pub struct WebSocketTransport {
    ws: WebSocket,
    queue: Queue,
    /// Sends made before the socket opened, sent when it does.
    early: Rc<RefCell<Vec<Vec<u8>>>>,
    // The callbacks live as long as the socket.
    _on: Vec<Closure<dyn FnMut(JsValue)>>,
}

/// `ws` beside the page: `new URL('ws', location.href)` with http(s)
/// turned into ws(s).
pub fn page_url() -> Option<String> {
    let loc = web_sys::window()?.location();
    let href = loc.href().ok()?;
    let url = web_sys::Url::new_with_base("ws", &href).ok()?;
    let proto = if url.protocol() == "https:" {
        "wss:"
    } else {
        "ws:"
    };
    url.set_protocol(proto);
    Some(url.href())
}

/// The page's whole address (with its query and `#` fragment).
pub fn page_href() -> Option<String> {
    web_sys::window()?.location().href().ok()
}

/// The signalling server an `mp-host` serves beside the page:
/// `new URL('signal', location.href)` on ws(s) (DECISIONS D1123).
pub fn signal_beside() -> Option<String> {
    let ws = page_url()?;
    Some(format!("{}signal", ws.strip_suffix("ws")?))
}

impl WebSocketTransport {
    pub fn open(url: &str) -> Result<WebSocketTransport, String> {
        let ws = WebSocket::new(url).map_err(|e| format!("{e:?}"))?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let queue: Queue = Rc::default();
        let early: Rc<RefCell<Vec<Vec<u8>>>> = Rc::default();
        let mut on = Vec::new();

        let (q, ws2, e2) = (queue.clone(), ws.clone(), early.clone());
        let open = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            for b in e2.borrow_mut().drain(..) {
                let _ = ws2.send_with_u8_array(&b);
            }
            q.borrow_mut().push_back(NetEvent::Connected(0));
        });
        ws.set_onopen(Some(open.as_ref().unchecked_ref()));
        on.push(open);

        let q = queue.clone();
        let message = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
            let Ok(e) = e.dyn_into::<MessageEvent>() else {
                return;
            };
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                q.borrow_mut().push_back(NetEvent::Message(0, bytes));
            }
        });
        ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
        on.push(message);

        let q = queue.clone();
        let close = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            let mut q = q.borrow_mut();
            if q.back() != Some(&NetEvent::Disconnected(0)) {
                q.push_back(NetEvent::Disconnected(0));
            }
        });
        ws.set_onclose(Some(close.as_ref().unchecked_ref()));
        ws.set_onerror(Some(close.as_ref().unchecked_ref()));
        on.push(close);

        Ok(WebSocketTransport {
            ws,
            queue,
            early,
            _on: on,
        })
    }
}

impl Transport for WebSocketTransport {
    fn send(&mut self, _peer: PeerId, _channel: Channel, bytes: &[u8]) {
        match self.ws.ready_state() {
            WebSocket::CONNECTING => self.early.borrow_mut().push(bytes.to_vec()),
            WebSocket::OPEN => {
                let _ = self.ws.send_with_u8_array(bytes);
            }
            _ => {}
        }
    }

    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        out.extend(self.queue.borrow_mut().drain(..));
    }

    fn close(&mut self, _peer: PeerId) {
        let _ = self.ws.close();
    }
}

impl Drop for WebSocketTransport {
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        let _ = self.ws.close();
    }
}

/// The connection's counters on `window.__mp.net` for the tests and for
/// finding stutter: round trip, rollbacks and the ticks they re-ran,
/// desyncs and rebuilds, how far ahead of the host's confirmed tick this
/// client runs, stalls, and this frame's ticks and drawn fraction.
pub fn publish(
    net: bevy::prelude::NonSend<super::Net>,
    play: bevy::prelude::Res<crate::play::Play>,
    cams: bevy::prelude::Query<
        &bevy::prelude::Transform,
        bevy::prelude::With<bevy::prelude::Camera3d>,
    >,
) {
    let Some(race) = &play.race else { return };
    let Some(w) = web_sys::window() else { return };
    let Ok(mr) = js_sys::Reflect::get(&w, &JsValue::from_str("__mp")) else {
        return;
    };
    if !mr.is_object() {
        return;
    }
    let o = js_sys::Object::new();
    let set = |k: &str, v: f64| {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &JsValue::from_f64(v));
    };
    if let Some(c) = &net.client {
        let s = c.stats;
        set("rtt", s.rtt);
        set("rollbacks", s.rollbacks as f64);
        set("resimulated", s.resimulated as f64);
        set("hashes", s.hashes_checked as f64);
        set("desyncs", s.desyncs as f64);
        set("unrepaired", s.unrepaired as f64);
        set("ahead", s.ahead as f64);
        set("stalls", s.stalls as f64);
        if let Some(r) = &c.race {
            set("local", r.local() as f64);
            set("confirmed", r.confirmed() as f64);
        }
    }
    let ss = &race.session;
    let a = ss.alpha();
    set("alpha", a);
    set("ticks", ss.ticks as f64);
    set("tick", ss.curr.tick as f64);
    set("me", ss.me as f64);
    // Each human's car where it is drawn (between the last two ticks).
    let xs = js_sys::Array::new();
    for (p, q) in ss.prev.players.iter().zip(&ss.curr.players) {
        let x = p.v.x + (q.v.x - p.v.x) * a;
        let z = p.v.z + (q.v.z - p.v.z) * a;
        xs.push(&JsValue::from_f64(x));
        xs.push(&JsValue::from_f64(z));
    }
    let _ = js_sys::Reflect::set(&o, &JsValue::from_str("cars"), &xs);
    if let Some(t) = cams.iter().next() {
        set("camX", t.translation.x as f64);
        set("camY", t.translation.y as f64);
        set("camZ", t.translation.z as f64);
    }
    let _ = js_sys::Reflect::set(&mr, &JsValue::from_str("net"), &o);
}
