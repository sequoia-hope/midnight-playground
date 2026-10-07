//! Joining by link in the client (MULTIPLAYER 8, DECISIONS D1124): which
//! signalling server to use, the invitation a host hands out, and what
//! the lobby's status line says while WebRTC connects. Pure functions, so
//! the rules are tested natively; the page's own address comes in from
//! `ws_web` on the web.

use mp_net::rtc::RtcStatus;
use mp_net::signal::{JoinLink, query_escape};

/// Where the game is published (D1101), for an invitation made natively
/// (a native host has no page of its own to point at).
pub const PAGES: &str = "https://sequoia-hope.github.io/midnight-racer/dist/next/";

/// The signalling server built into this build (`MP_SIGNAL_URL` when it was
/// compiled; Pages' workflow passes the repository variable), if any.
pub fn built_in_signal() -> Option<&'static str> {
    option_env!("MP_SIGNAL_URL").filter(|s| !s.trim().is_empty())
}

/// A query parameter of a URL (the part before `#`), decoded.
pub fn url_param(url: &str, key: &str) -> Option<String> {
    let before = url.split('#').next()?;
    let (_, q) = before.split_once('?')?;
    crate::options::parse_query(q)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
}

/// Whether `s` is an invitation (a link with `#join=…`, or the bare code)
/// rather than a WebSocket address.
pub fn is_link(s: &str) -> bool {
    !s.starts_with("ws://") && !s.starts_with("wss://") && JoinLink::parse(s).is_some()
}

/// The signalling server, in D1124's order: the link's own `?signal=`, the
/// page's `?signal=`, the build's, then the one beside the page (an
/// `mp-host` serves it there; on the web only).
pub fn signal_url(
    link: Option<&str>,
    page_param: Option<&str>,
    built_in: Option<&str>,
    beside_page: Option<&str>,
) -> Option<String> {
    link.and_then(|l| url_param(l, "signal"))
        .or_else(|| page_param.filter(|s| !s.is_empty()).map(str::to_string))
        .or_else(|| built_in.map(str::to_string))
        .or_else(|| beside_page.map(str::to_string))
}

/// The invitation for a room: the page (without its fragment), with the
/// signalling server in the query when it isn't the build's own (so a
/// guest's page finds the same one), and the room after `#`.
pub fn invite_url(page: &str, signal: &str, built_in: Option<&str>, link: &JoinLink) -> String {
    let page = page.split('#').next().unwrap_or(page);
    let has = url_param(page, "signal").is_some_and(|s| s == signal);
    let mut url = page.to_string();
    if !has && built_in != Some(signal) {
        let sep = if url.contains('?') { '&' } else { '?' };
        url.push(sep);
        url.push_str("signal=");
        url.push_str(&query_escape(signal));
    }
    format!("{url}#{}", link.fragment())
}

/// The lobby's status line while WebRTC connects (empty when there is
/// nothing to say).
pub fn status_line(hosting: bool, status: &RtcStatus) -> String {
    match (hosting, status) {
        (_, RtcStatus::Failed(e)) => format!("Can't reach the signalling server: {e}"),
        (true, RtcStatus::Signalling) => "Opening the room…".into(),
        (true, _) => String::new(),
        (false, RtcStatus::Signalling) => "Reaching the signalling server…".into(),
        (false, RtcStatus::InRoom) => "Waiting for the host…".into(),
        (false, RtcStatus::FoundHost) => "Joining…".into(),
    }
}

/// Whether a status line is one of [`status_line`]'s (so the room's next
/// state may replace it; what the host said may not be).
pub fn is_room_line(s: &str) -> bool {
    s.starts_with("Can't reach the signalling server")
        || [
            "Opening the room…",
            "Reaching the signalling server…",
            "Waiting for the host…",
            "Joining…",
        ]
        .contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> JoinLink {
        let mut n = 0u8;
        JoinLink::new(&mut |b: &mut [u8]| {
            for x in b {
                n = n.wrapping_add(37);
                *x = n;
            }
        })
    }

    #[test]
    fn links_are_told_from_websocket_addresses() {
        let l = link();
        assert!(is_link(&format!("https://x/dist/next/#{}", l.fragment())));
        assert!(is_link(&format!("#{}", l.fragment())));
        assert!(is_link(&l.code()));
        assert!(!is_link("ws://192.168.1.5/midnight-racer/dist/next/ws"));
        assert!(!is_link("wss://host.example/ws"));
        assert!(!is_link("https://x/dist/next/"));
        assert!(!is_link(""));
    }

    #[test]
    fn the_signalling_server_comes_from_the_link_the_page_the_build_or_beside_it() {
        let l = format!(
            "https://x/next/?level=coast&signal=wss%3A%2F%2Fsig.example%2F#{}",
            link().fragment()
        );
        let any = |link, page, built, beside| signal_url(link, page, built, beside);
        assert_eq!(
            any(Some(l.as_str()), Some("wss://p/"), Some("wss://b/"), Some("ws://h/signal")),
            Some("wss://sig.example/".into())
        );
        let bare = format!("#{}", link().fragment());
        assert_eq!(
            any(Some(bare.as_str()), Some("wss://p/"), Some("wss://b/"), None),
            Some("wss://p/".into())
        );
        assert_eq!(
            any(None, Some(""), Some("wss://b/"), Some("ws://h/signal")),
            Some("wss://b/".into())
        );
        assert_eq!(
            any(None, None, None, Some("ws://h/signal")),
            Some("ws://h/signal".into())
        );
        assert_eq!(any(None, None, None, None), None);
        assert_eq!(url_param("https://x/?a=1&signal=&b=2", "signal"), None);
        assert_eq!(url_param("https://x/#signal=wss://y/", "signal"), None);
    }

    #[test]
    fn an_invitation_carries_the_signalling_server_only_when_it_must() {
        let l = link();
        let frag = l.fragment();
        // The build's own server: nothing to add.
        assert_eq!(
            invite_url("https://x/next/?level=coast#old", "wss://b/", Some("wss://b/"), &l),
            format!("https://x/next/?level=coast#{frag}")
        );
        // Another one: in the query, escaped.
        assert_eq!(
            invite_url("https://x/next/", "wss://s.example/", Some("wss://b/"), &l),
            format!("https://x/next/?signal=wss%3A%2F%2Fs.example%2F#{frag}")
        );
        assert_eq!(
            invite_url("https://x/next/?hq=1", "wss://s/", None, &l),
            format!("https://x/next/?hq=1&signal=wss%3A%2F%2Fs%2F#{frag}")
        );
        // Already in the page's query: not twice.
        let page = "https://x/next/?signal=wss%3A%2F%2Fs%2F";
        assert_eq!(
            invite_url(page, "wss://s/", None, &l),
            format!("{page}#{frag}")
        );
        // What a guest makes of it: the same room, the same server.
        let url = invite_url("https://x/next/", "wss://s/", None, &l);
        assert_eq!(JoinLink::parse(&url), Some(l));
        assert_eq!(signal_url(Some(&url), None, None, None), Some("wss://s/".into()));
    }

    #[test]
    fn the_status_line() {
        let failed = RtcStatus::Failed("refused".into());
        assert_eq!(
            status_line(false, &failed),
            "Can't reach the signalling server: refused"
        );
        assert_eq!(status_line(true, &failed), status_line(false, &failed));
        assert_eq!(status_line(true, &RtcStatus::Signalling), "Opening the room…");
        assert_eq!(status_line(true, &RtcStatus::InRoom), "");
        assert_eq!(
            status_line(false, &RtcStatus::Signalling),
            "Reaching the signalling server…"
        );
        assert_eq!(status_line(false, &RtcStatus::InRoom), "Waiting for the host…");
        assert_eq!(status_line(false, &RtcStatus::FoundHost), "Joining…");
        // Every line it makes can be replaced by the next; the host's own
        // words can't.
        for hosting in [false, true] {
            for st in [
                RtcStatus::Signalling,
                RtcStatus::InRoom,
                RtcStatus::FoundHost,
                failed.clone(),
            ] {
                let l = status_line(hosting, &st);
                assert!(l.is_empty() || is_room_line(&l), "{l}");
            }
        }
        assert!(!is_room_line("The host said no: the lobby is full"));
        assert!(!is_room_line("Lost the host"));
        assert!(!is_room_line(""));
    }
}
