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
