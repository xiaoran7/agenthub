/// Idle release state machine (ported from Magpie's `lightStep(on, shown, ticks)`).
///
/// Tracks consecutive 1-second ticks while the isolated `--ui` WebView2 window
/// is minimized, hidden, or inactive in the background. When `inactive_ticks`
/// reaches `limit_seconds`, the host closes the window and exits the `--ui`
/// process so WebView2 memory returns to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    KeepAlive,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleTracker {
    limit_seconds: u64,
    inactive_ticks: u64,
}

impl IdleTracker {
    pub fn new(limit_seconds: u64) -> Self {
        Self {
            limit_seconds,
            inactive_ticks: 0,
        }
    }

    /// Advances the state machine by one 1-second tick.
    ///
    /// When `active` is `true` (window is visible, non-minimized, and in the
    /// foreground or recently interacted with), the counter resets immediately.
    /// A `limit_seconds == 0` disables idle auto-release.
    pub fn step(&mut self, active: bool) -> IdleAction {
        if self.limit_seconds == 0 || active {
            self.inactive_ticks = 0;
            return IdleAction::KeepAlive;
        }
        self.inactive_ticks = self.inactive_ticks.saturating_add(1);
        if self.inactive_ticks >= self.limit_seconds {
            IdleAction::Release
        } else {
            IdleAction::KeepAlive
        }
    }

    pub fn reset(&mut self) {
        self.inactive_ticks = 0;
    }

    pub fn inactive_ticks(&self) -> u64 {
        self.inactive_ticks
    }

    pub fn limit_seconds(&self) -> u64 {
        self.limit_seconds
    }
}
