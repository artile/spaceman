//! When `NSWorkspace` last reported a different active application. Input times (seconds since
//! the last key, click or modifier change) are read later, when a cycle decides what an
//! activation means; this time lets it measure them back from the activation itself.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Process-relative reference for the stored timestamp.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Nanoseconds after `EPOCH` of the latest activation, plus one (zero means none).
static ACTIVATED_AT: AtomicU64 = AtomicU64::new(0);

fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

/// Records an application activation happening now.
pub(super) fn note_activation() {
    let elapsed = u64::try_from(epoch().elapsed().as_nanos()).unwrap_or(u64::MAX - 1);
    ACTIVATED_AT.store(elapsed.saturating_add(1), Ordering::Release);
}

/// Seconds since the last application activation, if any happened since launch.
#[must_use]
pub fn seconds_since_activation() -> Option<f64> {
    match ACTIVATED_AT.load(Ordering::Acquire) {
        0 => None,
        stored => Some(
            (epoch() + Duration::from_nanos(stored - 1))
                .elapsed()
                .as_secs_f64(),
        ),
    }
}
