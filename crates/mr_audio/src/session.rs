//! The iPhone audio session (`Audio.js` `askForPlayback`).
//!
//! An iPhone plays Web Audio as "ambient" sound: silenced by the Silent
//! switch, mixed under other apps. Ask for "playback", like a media app: the
//! game is heard in Silent mode, and other apps' audio pauses while it plays.
//! (Safari 17+ has `navigator.audioSession`; elsewhere this does nothing.)

/// `navigator.audioSession`: its `type`, read and written. Either may throw
/// (an error string).
pub trait AudioSession {
    fn get_type(&self) -> Result<String, String>;
    fn set_type(&self, t: &str) -> Result<(), String>;
}

/// `askForPlayback(nav)`: `session` is `navigator.audioSession` when the
/// platform has one.
pub fn ask_for_playback(session: Option<&dyn AudioSession>) {
    let Some(s) = session else {
        return;
    };
    // try { ... } catch { /* read-only or unsupported: stay ambient */ }
    let _ = (|| -> Result<(), String> {
        if s.get_type()? != "playback" {
            s.set_type("playback")?;
        }
        Ok(())
    })();
}

/// The browser's `navigator.audioSession`, if it has one.
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub mod web {
    use super::AudioSession;
    use wasm_bindgen::JsValue;

    pub struct WebAudioSession(JsValue);

    /// `navigator.audioSession` (None where the API is missing).
    pub fn navigator_session() -> Option<WebAudioSession> {
        let global = js_sys::global();
        let nav = js_sys::Reflect::get(&global, &JsValue::from_str("navigator")).ok()?;
        if nav.is_undefined() || nav.is_null() {
            return None;
        }
        let s = js_sys::Reflect::get(&nav, &JsValue::from_str("audioSession")).ok()?;
        if s.is_undefined() || s.is_null() {
            return None;
        }
        Some(WebAudioSession(s))
    }

    fn err(e: JsValue) -> String {
        format!("{e:?}")
    }

    impl AudioSession for WebAudioSession {
        fn get_type(&self) -> Result<String, String> {
            let v = js_sys::Reflect::get(&self.0, &JsValue::from_str("type")).map_err(err)?;
            Ok(v.as_string().unwrap_or_default())
        }

        fn set_type(&self, t: &str) -> Result<(), String> {
            js_sys::Reflect::set(&self.0, &JsValue::from_str("type"), &JsValue::from_str(t))
                .map_err(err)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/audio-session.test.js`.
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Session {
        t: RefCell<String>,
        sets: Cell<u32>,
        refuse: bool,
    }

    impl AudioSession for Session {
        fn get_type(&self) -> Result<String, String> {
            Ok(self.t.borrow().clone())
        }
        fn set_type(&self, t: &str) -> Result<(), String> {
            if self.refuse {
                return Err("TypeError: read-only".into());
            }
            self.sets.set(self.sets.get() + 1);
            *self.t.borrow_mut() = t.into();
            Ok(())
        }
    }

    fn session(t: &str, refuse: bool) -> Session {
        Session {
            t: RefCell::new(t.into()),
            sets: Cell::new(0),
            refuse,
        }
    }

    #[test]
    fn asks_an_audio_session_for_playback() {
        let s = session("auto", false);
        ask_for_playback(Some(&s));
        assert_eq!(*s.t.borrow(), "playback");
    }

    #[test]
    fn leaves_a_session_already_playing_back_alone() {
        let s = session("playback", false);
        ask_for_playback(Some(&s));
        assert_eq!(s.sets.get(), 0);
    }

    #[test]
    fn no_audio_session_api_or_one_that_refuses_no_error() {
        ask_for_playback(None);
        let s = session("auto", true);
        ask_for_playback(Some(&s));
        assert_eq!(*s.t.borrow(), "auto");
    }
}
