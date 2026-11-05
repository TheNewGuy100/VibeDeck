//! Pure decision rule for the byte-range scheduler: what to do on each tick given the playback
//! state. No I/O, so pause, end of track and URL renewal are testable without a network or a sound card.

/// Renew this many seconds before the signed URL's `expire` time.
pub const RENEW_MARGIN_SECS: i64 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub paused: bool,
    /// Milliseconds of media already requested, on the same clock as `position_ms`.
    pub buffered_until_ms: u64,
    pub position_ms: u64,
    pub ahead_ms: u64,
    pub plan_left: bool,
    /// Seconds until the URL's `expire`; `None` when it carries none.
    pub secs_to_expiry: Option<i64>,
    /// The last media request was refused (401/403): the URL is dead whatever its `expire` says.
    pub last_denied: bool,
    /// Test hook: renew now even though the URL is fine.
    pub force_renew: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Request the next chunk.
    Fetch,
    /// Nothing to do this tick (paused, or enough buffer).
    Wait,
    /// Resolve the track again and replace the URL before the next request.
    Renew,
    /// Nothing left to request: close the stream so the engine ends cleanly.
    Finish,
}

pub fn decide(i: &Inputs) -> Action {
    if !i.plan_left {
        return Action::Finish;
    }
    if i.last_denied || i.force_renew {
        return Action::Renew;
    }
    if i.paused {
        return Action::Wait;
    }
    if i.secs_to_expiry.is_some_and(|left| left <= RENEW_MARGIN_SECS) {
        return Action::Renew;
    }
    if i.buffered_until_ms.saturating_sub(i.position_ms) < i.ahead_ms {
        Action::Fetch
    } else {
        Action::Wait
    }
}

/// Seconds until `expire` (a unix time query value) from `now`, if the URL has a numeric one.
pub fn secs_to_expiry(url: &reqwest::Url, now_unix: i64) -> Option<i64> {
    let expire: i64 = url
        .query_pairs()
        .find(|(k, _)| k == "expire")?
        .1
        .parse()
        .ok()?;
    Some(expire - now_unix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Inputs {
        Inputs {
            paused: false,
            buffered_until_ms: 10_000,
            position_ms: 5_000,
            ahead_ms: 30_000,
            plan_left: true,
            secs_to_expiry: Some(20_000),
            last_denied: false,
            force_renew: false,
        }
    }

    #[test]
    fn fetches_while_the_buffer_is_short_and_waits_when_it_is_full() {
        assert_eq!(decide(&base()), Action::Fetch);
        let full = Inputs { buffered_until_ms: 40_000, ..base() };
        assert_eq!(decide(&full), Action::Wait);
        let edge = Inputs { buffered_until_ms: 35_000, ..base() };
        assert_eq!(decide(&edge), Action::Wait);
    }

    #[test]
    fn pause_stops_requests_even_with_an_empty_buffer() {
        let paused = Inputs { paused: true, buffered_until_ms: 0, ..base() };
        assert_eq!(decide(&paused), Action::Wait);
    }

    #[test]
    fn finishes_when_nothing_is_left_even_while_paused() {
        assert_eq!(decide(&Inputs { plan_left: false, ..base() }), Action::Finish);
        assert_eq!(decide(&Inputs { plan_left: false, paused: true, ..base() }), Action::Finish);
    }

    #[test]
    fn renews_near_expiry_but_not_while_paused_and_always_after_a_denial() {
        let soon = Inputs { secs_to_expiry: Some(RENEW_MARGIN_SECS), ..base() };
        assert_eq!(decide(&soon), Action::Renew);
        assert_eq!(decide(&Inputs { paused: true, ..soon }), Action::Wait);
        assert_eq!(decide(&Inputs { paused: true, last_denied: true, ..base() }), Action::Renew);
        let expired = Inputs { secs_to_expiry: Some(-5), ..base() };
        assert_eq!(decide(&expired), Action::Renew);
        assert_eq!(decide(&Inputs { secs_to_expiry: None, ..base() }), Action::Fetch);
    }

    #[test]
    fn forced_renewal_wins_over_waiting() {
        let full = Inputs { buffered_until_ms: 90_000, force_renew: true, ..base() };
        assert_eq!(decide(&full), Action::Renew);
    }

    #[test]
    fn expiry_is_read_from_the_url() {
        let url = reqwest::Url::parse("https://x.googlevideo.com/videoplayback?expire=1000&itag=140").unwrap();
        assert_eq!(secs_to_expiry(&url, 400), Some(600));
        let none = reqwest::Url::parse("https://x.googlevideo.com/videoplayback?itag=140").unwrap();
        assert_eq!(secs_to_expiry(&none, 400), None);
    }
}
