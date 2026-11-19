//! Update-check progress state and deadlines.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Outcome of one animation tick of [`UpdateCheckProgress`].
pub enum UpdateCheckTick {
    Running,
    Complete,
    /// The deadline passed with checks still pending.
    TimedOut {
        remaining: usize,
    },
}

#[derive(Debug, Clone)]
pub struct UpdateCheckProgress {
    pub(crate) pending: HashSet<String>,
    total: usize,
    /// Smoothly animated value shown by the progress button.
    display_fraction: f32,
    /// Hard upper bound so a vanished app or a dropped event can never wedge
    /// the "Check All Updates" button forever.
    deadline: Instant,
}

impl UpdateCheckProgress {
    /// Smooth, steady fill speed. The bar takes about three seconds to cross
    /// the button, even if all network checks finish much sooner.
    pub(crate) const FILL_PER_SECOND: f32 = 0.35;
    /// Minimum time allowed for a batch of checks.
    pub(crate) const MIN_TIMEOUT: Duration = Duration::from_secs(90);
    /// Extra time allowed per tracked app.
    pub(crate) const PER_APP_TIMEOUT: Duration = Duration::from_secs(15);

    pub fn timeout_for(total: usize) -> Duration {
        Self::MIN_TIMEOUT.max(Self::PER_APP_TIMEOUT * total as u32)
    }

    /// Queues one check per id, keeping only the ones the command channel
    /// accepted. Returns `None` when nothing could be queued.
    pub fn start(
        ids: Vec<String>,
        mut send: impl FnMut(&str) -> bool,
        now: Instant,
    ) -> Option<Self> {
        let mut pending = HashSet::new();
        for id in ids {
            if send(&id) {
                pending.insert(id);
            }
        }
        if pending.is_empty() {
            return None;
        }
        let total = pending.len();
        Some(Self {
            pending,
            total,
            display_fraction: 0.0,
            deadline: now + Self::timeout_for(total),
        })
    }

    pub fn mark_finished(&mut self, app_id: &str) {
        self.pending.remove(app_id);
    }

    pub(crate) fn target_fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.total.saturating_sub(self.pending.len())) as f32 / self.total as f32
    }

    pub fn fraction(&self) -> f32 {
        self.display_fraction
    }

    pub fn tick(&mut self, dt: f32, now: Instant) -> UpdateCheckTick {
        if !self.pending.is_empty() && now >= self.deadline {
            return UpdateCheckTick::TimedOut {
                remaining: self.pending.len(),
            };
        }
        let target = self.target_fraction();
        if self.display_fraction < target {
            self.display_fraction =
                (self.display_fraction + dt * Self::FILL_PER_SECOND).min(target);
        }
        if self.pending.is_empty() && self.display_fraction >= 1.0 {
            UpdateCheckTick::Complete
        } else {
            UpdateCheckTick::Running
        }
    }
}
