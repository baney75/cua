//! The driver's own posts to the global HID tap.
//!
//! A post at `CGEventTapLocation::HID` resets the HID system state's
//! seconds-since-last-event counters exactly as real hardware does. The
//! user-switch guard reads those counters
//! ([`crate::key_focus_guard::user_caused_now`]), so every HID-location post
//! the driver makes goes through here and leaves a timestamp. The guard then
//! ignores input while the driver itself was posting.

use core_graphics::event::{CGEvent, CGEventTapLocation};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Nanoseconds since [`epoch`] of the last HID post, plus one (0 = never).
static LAST_POST_NANOS: AtomicU64 = AtomicU64::new(0);

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn record_now() {
    let nanos = epoch().elapsed().as_nanos().min(u64::MAX as u128 - 1) as u64;
    LAST_POST_NANOS.store(nanos + 1, Ordering::Relaxed);
}

/// Post `event` to the global HID tap and remember that the driver did.
pub(crate) fn post_hid(event: &CGEvent) {
    post_at(event, CGEventTapLocation::HID);
}

/// Post `event` at `tap`; a HID-location post is recorded.
pub(crate) fn post_at(event: &CGEvent, tap: CGEventTapLocation) {
    if matches!(tap, CGEventTapLocation::HID) {
        record_now();
    }
    event.post(tap);
}

/// Whether the driver posted to the HID tap within the last `window`.
pub(crate) fn driver_posted_hid_within(window: Duration) -> bool {
    posted_within(
        LAST_POST_NANOS.load(Ordering::Relaxed),
        epoch().elapsed(),
        window,
    )
}

fn posted_within(stored: u64, now: Duration, window: Duration) -> bool {
    if stored == 0 {
        return false;
    }
    let posted = Duration::from_nanos(stored - 1);
    now.saturating_sub(posted) < window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_posted_is_not_recent() {
        assert!(!posted_within(
            0,
            Duration::from_secs(5),
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn recent_and_old_posts() {
        let at = |s: u64| Duration::from_secs(s).as_nanos() as u64 + 1;
        let window = Duration::from_millis(1500);
        assert!(posted_within(at(10), Duration::from_secs(11), window));
        assert!(!posted_within(at(10), Duration::from_secs(12), window));
    }

    #[test]
    fn recording_is_seen_by_the_live_reader() {
        record_now();
        assert!(driver_posted_hid_within(Duration::from_secs(5)));
    }
}
