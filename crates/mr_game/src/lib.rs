//! The client (SPEC 3, 6, 8). Only this crate depends on the engine.
//!
//! Roadmap WP 0.1: no engine yet. The web build is a bare wasm-bindgen module
//! that writes one line to the page, which proves the build pipeline, the
//! dev server and the tailnet https front without choosing a Bevy version
//! (that happens in WP 2.1).

/// The line both builds print, so the native and web pipelines are seen to
/// run the same crate.
pub fn banner() -> String {
    format!("Midnight Racer (Rust) {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// Runs when the module is instantiated: report what the page needs to
    /// know for later milestones (WebGPU, a secure context) on the page.
    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let navigator = window.navigator();
        let webgpu = js_sys::Reflect::has(&navigator, &JsValue::from_str("gpu"))?;
        let secure = window.is_secure_context();
        let yes = |b: bool| if b { "yes" } else { "no" };
        let line = format!(
            "{}: wasm running. WebGPU: {}. Secure context: {}.",
            super::banner(),
            yes(webgpu),
            yes(secure)
        );
        let out = document.get_element_by_id("out").ok_or("no #out element")?;
        out.set_text_content(Some(&line));
        out.set_attribute("data-ready", "1")?;
        out.set_attribute("data-webgpu", yes(webgpu))?;
        out.set_attribute("data-secure", yes(secure))?;
        Ok(())
    }
}
