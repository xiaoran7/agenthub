//! One change bus for every local surface of the Runner.
//!
//! The console used to learn about new sessions by asking on a timer, with
//! each provider cache expiring on its own clock. A session found by ZCode a
//! moment after Claude's cache was filled surfaced a poll later than the
//! Claude one, so the list grew a row at a time. Nothing here is a feed of
//! data: it is a single "something changed" signal, published after one scan
//! pass has refreshed every store, so every surface re-reads the same
//! snapshot and all rows arrive together.
//!
//! The bus carries no payload on purpose. The dashboard summary stays the
//! only truth, re-read through `/api/dashboard` under its ETag, so a
//! subscriber can never render state the query endpoints would not return.

use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

/// How often the single scan tick runs.
///
/// Each tick refreshes every store in one pass, and the scans are incremental
/// (per-file byte offsets and SQLite watermarks), so a tick costs a metadata
/// walk rather than a re-read. This is also what keeps every provider inside
/// its own cache TTL: one pass therefore sees every store at the same
/// instant, which is what stops the list from growing a row at a time.
const TICK_ACTIVE: Duration = Duration::from_millis(1000);
const TICK_IDLE_MAX: Duration = Duration::from_millis(10_000);
const IDLE_TICKS_BEFORE_BACKOFF: u32 = 5;

/// How many pending notifications a subscriber may fall behind by before its
/// queue is treated as dead. The channel is bounded so a tab that stopped
/// reading cannot pin memory.
const SUBSCRIBER_CAPACITY: usize = 16;

struct Bus {
    version: AtomicU64,
    next_id: AtomicU64,
    activity_seq: AtomicU64,
    subscribers: Mutex<Vec<(u64, SyncSender<u64>)>>,
}

fn bus() -> &'static Bus {
    static BUS: OnceLock<Bus> = OnceLock::new();
    BUS.get_or_init(|| Bus {
        version: AtomicU64::new(1),
        next_id: AtomicU64::new(1),
        activity_seq: AtomicU64::new(1),
        subscribers: Mutex::new(Vec::new()),
    })
}

/// Signals UI/store activity so any idle scan backoff resets immediately to 1s.
pub fn note_activity() {
    bus().activity_seq.fetch_add(1, Ordering::Relaxed);
}

fn has_subscribers() -> bool {
    bus()
        .subscribers
        .lock()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

fn next_idle_interval(idle_scans: u32, subscribers_active: bool) -> Duration {
    let threshold = if subscribers_active {
        IDLE_TICKS_BEFORE_BACKOFF.saturating_mul(2)
    } else {
        IDLE_TICKS_BEFORE_BACKOFF
    };
    if idle_scans < threshold {
        return TICK_ACTIVE;
    }
    let shift = (idle_scans - threshold).min(4);
    let max_tick = if subscribers_active {
        Duration::from_millis(5_000)
    } else {
        TICK_IDLE_MAX
    };
    let ms = TICK_ACTIVE.as_millis() << shift;
    Duration::from_millis(u64::try_from(ms).unwrap_or(u64::MAX)).min(max_tick)
}

/// The version of the last published change. Monotonic; never resets.
pub fn version() -> u64 {
    bus().version.load(Ordering::Relaxed)
}

/// Announces that at least one store changed.
///
/// Called by the scan pass when it finds a different snapshot, and by the few
/// endpoints that write state the console shows (session deletion, device
/// rename). A subscriber that stopped reading is dropped rather than blocking
/// the caller: the console re-reads a full snapshot, so the next notification
/// carries the change too.
pub fn publish() {
    let bus = bus();
    bus.activity_seq.fetch_add(1, Ordering::Relaxed);
    let next = bus.version.fetch_add(1, Ordering::Relaxed) + 1;
    let Ok(mut subscribers) = bus.subscribers.lock() else {
        return;
    };
    subscribers.retain(|(_, sender)| sender.try_send(next).is_ok());
}

/// A live subscription: the version at subscribe time, then every later one.
pub struct Subscription {
    pub current: u64,
    pub receiver: Receiver<u64>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut subscribers) = bus().subscribers.lock() {
            subscribers.retain(|(id, _)| *id != self.id);
        }
    }
}

pub fn subscribe() -> Subscription {
    let b = bus();
    b.activity_seq.fetch_add(1, Ordering::Relaxed);
    let id = b.next_id.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = sync_channel(SUBSCRIBER_CAPACITY);
    if let Ok(mut subscribers) = b.subscribers.lock() {
        subscribers.push((id, sender));
    }
    Subscription {
        current: version(),
        receiver,
        id,
    }
}

/// Starts the one thread that keeps every scan pass on one schedule.
///
/// Without it the console would have to keep polling just to make the scans
/// happen, which is the behaviour this module exists to remove. The thread
/// only asks for a pass; the pass itself (filesystem and SQLite work) still
/// runs on the scanner's own thread.
pub fn start(stop: Arc<AtomicBool>) {
    let _ = thread::Builder::new()
        .name("agenthub-live-tick".into())
        .spawn(move || {
            let b = bus();
            let mut last = Instant::now() - TICK_ACTIVE;
            let mut last_ver = version();
            let mut last_activity = b.activity_seq.load(Ordering::Relaxed);
            let mut last_plugin_gen = crate::plugins::state_generation();
            let mut idle_scans = 0_u32;
            let mut interval = TICK_ACTIVE;

            while !stop.load(Ordering::Relaxed) {
                let current_activity = b.activity_seq.load(Ordering::Relaxed);
                if current_activity != last_activity {
                    last_activity = current_activity;
                    idle_scans = 0;
                    interval = TICK_ACTIVE;
                }
                let current_plugin_gen = crate::plugins::state_generation();
                if current_plugin_gen != last_plugin_gen {
                    last_plugin_gen = current_plugin_gen;
                    crate::web_server::invalidate_dashboard_summary_cache();
                    publish();
                }
                if last.elapsed() >= interval {
                    last = Instant::now();
                    crate::remote_host::wake_scan();
                    let now_ver = version();
                    let now_activity = b.activity_seq.load(Ordering::Relaxed);
                    if now_ver != last_ver || now_activity != last_activity {
                        last_ver = now_ver;
                        last_activity = now_activity;
                        idle_scans = 0;
                    } else {
                        idle_scans = idle_scans.saturating_add(1);
                    }
                    interval = next_idle_interval(idle_scans, has_subscribers());
                }
                thread::sleep(Duration::from_millis(100));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_publish_reaches_every_subscriber_with_a_newer_version() {
        let a = subscribe();
        let b = subscribe();
        let before = version();
        publish();
        assert!(a.receiver.try_recv().unwrap() > before);
        assert!(b.receiver.try_recv().unwrap() > before);
    }

    #[test]
    fn a_dropped_subscriber_does_not_block_later_publishes() {
        let doomed = subscribe();
        drop(doomed);
        publish();
        let live = subscribe();
        publish();
        assert!(live.receiver.try_recv().is_ok());
    }

    #[test]
    fn idle_backoff_stays_fast_initially_and_caps_smoothly() {
        assert_eq!(next_idle_interval(0, false), Duration::from_millis(1000));
        assert_eq!(next_idle_interval(4, false), Duration::from_millis(1000));
        assert_eq!(next_idle_interval(5, false), Duration::from_millis(1000));
        assert_eq!(next_idle_interval(6, false), Duration::from_millis(2000));
        assert_eq!(next_idle_interval(9, false), Duration::from_millis(10_000));
        assert_eq!(next_idle_interval(10, true), Duration::from_millis(1000));
        assert_eq!(next_idle_interval(14, true), Duration::from_millis(5000));
    }
}
