use super::*;

pub(crate) const ACTIVE_READ_TIMEOUT: Duration = Duration::from_millis(4);
pub(crate) const IDLE_READ_TIMEOUT: Duration = Duration::from_millis(20);
pub(crate) const ACTIVE_HOLD_WINDOW: Duration = Duration::from_millis(500);
pub(crate) const OUTBOUND_DRAIN_LIMIT: usize = 64;

/// Drains up to `limit` queued outbound frames for `epoch` and schedules
/// high-priority control/ack frames ahead of sequenced data frames.
///
/// Sequenced `Data` frames strictly preserve their arrival order so Gateway's
/// contiguous monotonic `seq` watermark invariant is never violated. Adjacent
/// `Ack` frames are collapsed to the highest command sequence number.
pub(crate) fn drain_prioritized(
    outgoing: &Receiver<NetworkSend>,
    epoch: u64,
    limit: usize,
) -> Vec<NetworkSend> {
    let mut control_lane: Vec<NetworkSend> = Vec::new();
    let mut data_lane: Vec<NetworkSend> = Vec::new();
    let mut latest_ack: Option<i64> = None;

    for frame in outgoing.try_iter().take(limit) {
        match frame {
            NetworkSend::Ack(e, seq) if e == epoch => {
                latest_ack = Some(latest_ack.map_or(seq, |prev| prev.max(seq)));
            }
            NetworkSend::Control(e, payload) if e == epoch => {
                control_lane.push(NetworkSend::Control(e, payload));
            }
            NetworkSend::Data(e, seq, payload) if e == epoch => {
                data_lane.push(NetworkSend::Data(e, seq, payload));
            }
            _ => {}
        }
    }

    let mut ordered = Vec::with_capacity(
        usize::from(latest_ack.is_some()) + control_lane.len() + data_lane.len(),
    );
    if let Some(seq) = latest_ack {
        ordered.push(NetworkSend::Ack(epoch, seq));
    }
    ordered.extend(control_lane);
    ordered.extend(data_lane);
    ordered
}

/// Switches socket read timeout between a low-latency active cadence (`4ms`)
/// during streaming/interactive bursts and a quiet idle cadence (`20ms`), only
/// issuing a `set_read_timeout` syscall when the tier actually changes.
pub(crate) struct AdaptiveReadTimeout {
    active_until: Instant,
    current: Duration,
}

impl AdaptiveReadTimeout {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            active_until: now + ACTIVE_HOLD_WINDOW,
            current: ACTIVE_READ_TIMEOUT,
        }
    }

    pub(crate) fn current(&self) -> Duration {
        self.current
    }

    pub(crate) fn note_activity(&mut self, now: Instant) -> Option<Duration> {
        self.active_until = now + ACTIVE_HOLD_WINDOW;
        if self.current != ACTIVE_READ_TIMEOUT {
            self.current = ACTIVE_READ_TIMEOUT;
            return Some(self.current);
        }
        None
    }

    pub(crate) fn maybe_relax(&mut self, now: Instant) -> Option<Duration> {
        if self.current != IDLE_READ_TIMEOUT && now >= self.active_until {
            self.current = IDLE_READ_TIMEOUT;
            return Some(self.current);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prioritizes_acks_and_control_ahead_of_data_while_preserving_data_seq() {
        let (tx, rx) = mpsc::sync_channel(16);
        tx.send(NetworkSend::Data(1, 10, json!({"k": "a"})))
            .unwrap();
        tx.send(NetworkSend::Ack(1, 3)).unwrap();
        tx.send(NetworkSend::Data(1, 11, json!({"k": "b"})))
            .unwrap();
        tx.send(NetworkSend::Control(1, json!({"type": "plugin_state"})))
            .unwrap();
        tx.send(NetworkSend::Ack(1, 5)).unwrap();
        tx.send(NetworkSend::Data(2, 99, json!({"stale": true})))
            .unwrap();

        let drained = drain_prioritized(&rx, 1, 64);
        assert_eq!(drained.len(), 4);
        assert!(matches!(drained[0], NetworkSend::Ack(1, 5)));
        assert!(matches!(drained[1], NetworkSend::Control(1, _)));
        assert!(matches!(drained[2], NetworkSend::Data(1, 10, _)));
        assert!(matches!(drained[3], NetworkSend::Data(1, 11, _)));
    }

    #[test]
    fn adaptive_read_timeout_relaxes_after_hold_window_and_snaps_back() {
        let t0 = Instant::now();
        let mut timeout = AdaptiveReadTimeout::new(t0);
        assert_eq!(timeout.current(), ACTIVE_READ_TIMEOUT);
        assert_eq!(timeout.maybe_relax(t0 + Duration::from_millis(200)), None);
        assert_eq!(
            timeout.maybe_relax(t0 + ACTIVE_HOLD_WINDOW),
            Some(IDLE_READ_TIMEOUT)
        );
        assert_eq!(
            timeout.maybe_relax(t0 + ACTIVE_HOLD_WINDOW + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            timeout.note_activity(t0 + ACTIVE_HOLD_WINDOW + Duration::from_millis(60)),
            Some(ACTIVE_READ_TIMEOUT)
        );
        assert_eq!(
            timeout.note_activity(t0 + ACTIVE_HOLD_WINDOW + Duration::from_millis(70)),
            None
        );
    }
}
