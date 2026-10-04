//! The recorded police radio voice (`src/game/audio/RadioVoice.js`, and
//! `clipId` from `radioLines.js`): which clips exist (`audio/radio/
//! index.json`), their compressed bytes fetched ahead of need, and
//! AudioBuffers decoded when a line is said. Decoded clips aren't kept: a
//! level's worth would be tens of MB on a phone, and a clip of a few seconds
//! decodes in milliseconds.
//!
//! Fetching is the platform's ([`Fetch`]): the browser's `fetch`, a file
//! read natively, `audio/radio/` on disk in the call-log playback. Its
//! results are [`Pending`]s, chained as the JS chains its promises.
//!
//! [`lines`] is `radioLines.js`: the lines dispatch says and the clips
//! that speak them.

pub mod lines;

use crate::wa::{AudioBuffer, AudioContext, Pending};
use mr_math::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A file's bytes, or `None` when the fetch failed (`r.ok` false, or it
/// threw).
pub type Bytes = Option<Rc<Vec<u8>>>;

/// Fetches files from the radio's directory (`audio/radio/`), by name.
pub trait Fetch {
    fn fetch(&self, file: &str) -> Pending<Bytes>;
}

/// The audio's `Math.random`, shared by everything that draws from it.
pub type Random = Rc<RefCell<dyn Rng>>;

/// A clip's name from its words: `clipId(words)`.
pub fn clip_id(words: &str) -> String {
    let lower = words.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut dash = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let s = out.strip_prefix('-').unwrap_or(&out);
    let s = s.strip_suffix('-').unwrap_or(s);
    s.to_owned()
}

/// Clip id → number of takes (`index.json`'s `clips`).
pub type Index = HashMap<String, u32>;

struct Inner {
    fetcher: Rc<dyn Fetch>,
    index: RefCell<Option<Rc<Index>>>,
    index_p: RefCell<Option<Pending<Rc<Index>>>>,
    bytes: RefCell<HashMap<String, Pending<Bytes>>>,
}

/// `RadioVoice`.
#[derive(Clone)]
pub struct RadioVoice(Rc<Inner>);

/// Settles with every result once all have settled (`Promise.all`, with a
/// failure as `Err`).
fn all<T: Clone + 'static>(ps: Vec<Pending<T>>) -> Pending<Result<Vec<T>, ()>> {
    let out = Pending::new();
    let n = ps.len();
    if n == 0 {
        out.resolve(Ok(Ok(Vec::new())));
        return out;
    }
    type Slots<T> = Rc<RefCell<Vec<Option<Result<T, ()>>>>>;
    let slots: Slots<T> = Rc::new(RefCell::new(vec![None; n]));
    for (i, p) in ps.into_iter().enumerate() {
        let slots = slots.clone();
        let out = out.clone();
        p.then(move |r| {
            let done = {
                let mut s = slots.borrow_mut();
                s[i] = Some(r.clone().map_err(|_| ()));
                s.iter().all(Option::is_some)
            };
            if done {
                let s = std::mem::take(&mut *slots.borrow_mut());
                let r: Result<Vec<T>, ()> = s.into_iter().map(|x| x.expect("settled")).collect();
                out.resolve(Ok(r));
            }
        });
    }
    out
}

impl RadioVoice {
    pub fn new(fetcher: Rc<dyn Fetch>) -> Self {
        RadioVoice(Rc::new(Inner {
            fetcher,
            index: RefCell::new(None),
            index_p: RefCell::new(None),
            bytes: RefCell::new(HashMap::new()),
        }))
    }

    /// Clip id → takes, once `index.json` is in (empty without it).
    pub fn index(&self) -> Option<Rc<Index>> {
        self.0.index.borrow().clone()
    }

    /// `load()`: `index.json`, fetched once.
    pub fn load(&self) -> Pending<Rc<Index>> {
        if let Some(p) = self.0.index_p.borrow().clone() {
            return p;
        }
        let p: Pending<Rc<Index>> = Pending::new();
        *self.0.index_p.borrow_mut() = Some(p.clone());
        let inner = Rc::downgrade(&self.0);
        let out = p.clone();
        self.0.fetcher.fetch("index.json").then(move |r| {
            let index = Rc::new(parse_index(r.as_ref().ok().cloned().flatten()));
            if let Some(i) = inner.upgrade() {
                *i.index.borrow_mut() = Some(index.clone());
            }
            out.resolve(Ok(index));
        });
        p
    }

    fn file(id: &str, take: u32) -> String {
        if take > 1 {
            format!("{id}.{take}.mp3")
        } else {
            format!("{id}.mp3")
        }
    }

    fn fetch(&self, file: &str) -> Pending<Bytes> {
        if let Some(p) = self.0.bytes.borrow().get(file) {
            return p.clone();
        }
        let p = self.0.fetcher.fetch(file);
        self.0.bytes.borrow_mut().insert(file.to_owned(), p.clone());
        p
    }

    /// Fetch every take of these clips, a few at a time, so a line is ready
    /// the moment it's said.
    pub fn prefetch(&self, ids: &[String]) -> Pending<()> {
        let done = Pending::new();
        let me = self.clone();
        let ids = ids.to_vec();
        let out = done.clone();
        self.load().then(move |r| {
            let index = r.as_ref().map(|i| i.clone()).unwrap_or_default();
            let files: Vec<String> = ids
                .iter()
                .flat_map(|id| {
                    let n = index.get(id).copied().unwrap_or(0);
                    (0..n).map(move |i| RadioVoice::file(id, i + 1))
                })
                .collect();
            let state = Rc::new(RefCell::new((files, 0usize, 3usize)));
            for _ in 0..3 {
                worker(me.clone(), state.clone(), out.clone());
            }
        });
        done
    }

    /// A line's parts as AudioBuffers (a random take of each), or `None` if
    /// any part has no recording.
    pub fn buffers(
        &self,
        ctx: &AudioContext,
        parts: &[String],
        rng: Random,
    ) -> Pending<Option<Vec<AudioBuffer>>> {
        let out = Pending::new();
        let me = self.clone();
        let ctx = ctx.clone();
        let parts = parts.to_vec();
        let o = out.clone();
        self.load().then(move |r| {
            let index = r.as_ref().map(|i| i.clone()).unwrap_or_default();
            let mut files = Vec::new();
            for p in &parts {
                let id = clip_id(p);
                let n = index.get(&id).copied().unwrap_or(0);
                if n == 0 {
                    o.resolve(Ok(None));
                    return;
                }
                let take = 1 + (rng.borrow_mut().next_f64() * n as f64).floor() as u32;
                files.push(RadioVoice::file(&id, take));
            }
            let fetches = files.iter().map(|f| me.fetch(f)).collect();
            all(fetches).then(move |r| {
                let bytes = match r {
                    Ok(Ok(b)) if b.iter().all(Option::is_some) => b,
                    _ => {
                        o.resolve(Ok(None));
                        return;
                    }
                };
                // decodeAudioData takes its ArrayBuffer over (detaches it):
                // the JS hands it a copy; here the bytes are only read.
                let decodes = bytes
                    .iter()
                    .map(|b| ctx.decode_audio_data(b.as_ref().expect("fetched")))
                    .collect();
                all(decodes).then(move |r| {
                    o.resolve(Ok(match r {
                        Ok(Ok(v)) => Some(v.clone()),
                        _ => None,
                    }));
                });
            });
        });
        out
    }
}

fn worker(me: RadioVoice, state: Rc<RefCell<(Vec<String>, usize, usize)>>, done: Pending<()>) {
    let next = {
        let mut s = state.borrow_mut();
        if s.1 < s.0.len() {
            s.1 += 1;
            Some(s.0[s.1 - 1].clone())
        } else {
            None
        }
    };
    match next {
        Some(f) => {
            let p = me.fetch(&f);
            p.then(move |_| worker(me, state, done));
        }
        None => {
            let all_done = {
                let mut s = state.borrow_mut();
                s.2 -= 1;
                s.2 == 0
            };
            if all_done {
                done.resolve(Ok(()));
            }
        }
    }
}

/// `r.ok ? r.json() : null` then `j?.clips ?? {}`.
fn parse_index(bytes: Bytes) -> Index {
    let mut out = Index::new();
    let Some(b) = bytes else {
        return out;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) else {
        return out;
    };
    if let Some(clips) = v.get("clips").and_then(|c| c.as_object()) {
        for (k, n) in clips {
            if let Some(n) = n.as_f64() {
                out.insert(k.clone(), n as u32);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_ids() {
        assert_eq!(clip_id("Unit 12."), "unit-12");
        assert_eq!(
            clip_id("Heat level 4. Bring in everything we've got."),
            "heat-level-4-bring-in-everything-we-ve-got"
        );
        assert_eq!(clip_id("  Spike strip deployed!  "), "spike-strip-deployed");
    }
}
