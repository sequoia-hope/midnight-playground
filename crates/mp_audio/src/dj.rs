//! The radio DJs' recorded clips (radio.md 7: idents and song intros at the
//! seams the station schedule gives; `tools/dj-voice/` records them): which
//! clips exist (`audio/dj/index.json`), their bytes fetched ahead of need,
//! and an AudioBuffer decoded when one is said. The police radio's
//! [`crate::radio::RadioVoice`] is the pattern; the index differs:
//! `{ "clips": { "<id>": { "takes": n, "text": .., "voice": .. } } }`, with
//! take 1 at `<id>.mp3` and take `k` at `<id>.<k>.mp3`.
//!
//! Fetching is the platform's ([`Fetch`], over `audio/dj/`); decoded clips
//! are not kept.

use crate::radio::{Bytes, Fetch, Random};
use crate::wa::{AudioBuffer, AudioContext, Pending};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Clip id → number of takes.
pub type Index = HashMap<String, u32>;

struct Inner {
    fetcher: Rc<dyn Fetch>,
    random: Random,
    index_p: RefCell<Option<Pending<Rc<Index>>>>,
    bytes: RefCell<HashMap<String, Pending<Bytes>>>,
}

/// The DJ clips.
#[derive(Clone)]
pub struct DjVoice(Rc<Inner>);

impl DjVoice {
    pub fn new(fetch: Rc<dyn Fetch>, random: Random) -> Self {
        DjVoice(Rc::new(Inner {
            fetcher: fetch,
            random,
            index_p: RefCell::new(None),
            bytes: RefCell::new(HashMap::new()),
        }))
    }

    /// `index.json`, fetched once: clip id → takes (empty without it).
    pub fn index(&self) -> Pending<Rc<Index>> {
        if let Some(p) = self.0.index_p.borrow().clone() {
            return p;
        }
        let p: Pending<Rc<Index>> = Pending::new();
        *self.0.index_p.borrow_mut() = Some(p.clone());
        let out = p.clone();
        self.0.fetcher.fetch("index.json").then(move |r| {
            let index = parse_index(r.as_ref().ok().cloned().flatten());
            out.resolve(Ok(Rc::new(index)));
        });
        p
    }

    /// A take's file name.
    pub fn file(id: &str, take: u32) -> String {
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

    /// Fetch every take of these clips, one after another, so a break is
    /// ready the moment it is said. Settles when all are in (or failed).
    pub fn prefetch(&self, ids: &[String]) -> Pending<()> {
        let done = Pending::new();
        let me = self.clone();
        let ids = ids.to_vec();
        let out = done.clone();
        self.index().then(move |r| {
            let index = r.as_ref().map(|i| i.clone()).unwrap_or_default();
            let files: Vec<String> = ids
                .iter()
                .flat_map(|id| {
                    let n = index.get(id).copied().unwrap_or(0);
                    (1..=n).map(move |k| DjVoice::file(id, k))
                })
                .collect();
            fetch_all(me, files, 0, out);
        });
        done
    }

    /// Clip `id`'s take `take` (1-based; 0: a random take) as an
    /// AudioBuffer, or `None` when the clip is not recorded, its fetch
    /// failed or it did not decode.
    pub fn buffer(&self, ctx: &AudioContext, id: &str, take: u32) -> Pending<Option<AudioBuffer>> {
        let out = Pending::new();
        let me = self.clone();
        let ctx = ctx.clone();
        let id = id.to_owned();
        let o = out.clone();
        self.index().then(move |r| {
            let index = r.as_ref().map(|i| i.clone()).unwrap_or_default();
            let n = index.get(&id).copied().unwrap_or(0);
            if n == 0 {
                o.resolve(Ok(None));
                return;
            }
            let take = if take == 0 {
                1 + (me.0.random.borrow_mut().next_f64() * n as f64).floor() as u32
            } else {
                take.min(n)
            };
            me.fetch(&DjVoice::file(&id, take)).then(move |r| {
                let Ok(Some(bytes)) = r else {
                    o.resolve(Ok(None));
                    return;
                };
                ctx.decode_audio_data(bytes).then(move |r| {
                    o.resolve(Ok(r.as_ref().ok().cloned()));
                });
            });
        });
        out
    }
}

/// Fetches `files[i..]` in turn, then settles `done`.
fn fetch_all(me: DjVoice, files: Vec<String>, i: usize, done: Pending<()>) {
    if i >= files.len() {
        done.resolve(Ok(()));
        return;
    }
    let p = me.fetch(&files[i]);
    p.then(move |_| fetch_all(me, files, i + 1, done));
}

/// `r.ok ? r.json() : null` then `j?.clips ?? {}`, each clip's `takes`.
fn parse_index(bytes: Bytes) -> Index {
    let mut out = Index::new();
    let Some(b) = bytes else {
        return out;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) else {
        return out;
    };
    if let Some(clips) = v.get("clips").and_then(|c| c.as_object()) {
        for (k, c) in clips {
            // `{ takes, text, voice }`; a bare number is tolerated as the
            // police radio's index has it.
            let takes = c
                .get("takes")
                .and_then(|n| n.as_f64())
                .or_else(|| c.as_f64());
            if let Some(n) = takes {
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
    fn index_reads_takes() {
        let j = br#"{"clips":{"marisol-ident-3":{"takes":2,"text":"x","voice":"m"},"kit-ads-1":{"takes":1},"old":3}}"#;
        let i = parse_index(Some(Rc::new(j.to_vec())));
        assert_eq!(i.get("marisol-ident-3"), Some(&2));
        assert_eq!(i.get("kit-ads-1"), Some(&1));
        assert_eq!(i.get("old"), Some(&3));
        assert!(parse_index(None).is_empty());
        assert!(parse_index(Some(Rc::new(b"nope".to_vec()))).is_empty());
    }

    #[test]
    fn take_files() {
        assert_eq!(DjVoice::file("kit-ads-1", 1), "kit-ads-1.mp3");
        assert_eq!(DjVoice::file("kit-ads-1", 2), "kit-ads-1.2.mp3");
    }
}
