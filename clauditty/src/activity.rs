//! Tracking whether a pane is working, ready for the user, or idle.

use std::time::{Duration, Instant};

use crate::harness::ActivityRule;

/// How long an agent must keep printing before it counts as working.
const BUSY_THRESHOLD: Duration = Duration::from_secs(2);

/// Quiet time after which a working agent counts as finished.
const QUIET_THRESHOLD: Duration = Duration::from_secs(2);

/// Output this soon after typing is treated as the echo of the input.
const ECHO_WINDOW: Duration = Duration::from_millis(500);

/// Commands finishing faster than this don't need the user's attention.
const MIN_COMMAND_DURATION: Duration = Duration::from_secs(5);

/// What a pane is doing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Nothing running, or finished while the user was watching.
    #[default]
    Idle,
    /// An agent or command is running.
    Working { since: Instant },
    /// Finished while the user was away, waiting to be read.
    Ready { since: Instant },
}

/// Turns output, input and the running program into an [`Activity`].
#[derive(Debug, Default)]
pub struct ActivityTracker {
    activity: Activity,
    /// Start of the current run of output.
    output_start: Option<Instant>,
    last_output: Option<Instant>,
    last_input: Option<Instant>,
}

impl ActivityTracker {
    pub fn activity(&self) -> Activity {
        self.activity
    }

    /// Record that the user typed into the pane.
    pub fn on_input(&mut self, now: Instant) {
        self.last_input = Some(now);
    }

    /// Record that the pane printed output.
    pub fn on_output(&mut self, now: Instant) {
        // Ignore the echo of what the user types.
        if self.last_input.is_some_and(|input| now.duration_since(input) < ECHO_WINDOW) {
            return;
        }

        let quiet = self.last_output.is_none_or(|last| now.duration_since(last) >= QUIET_THRESHOLD);
        if quiet {
            self.output_start = Some(now);
        }
        self.last_output = Some(now);
    }

    /// Mark a ready pane as read.
    pub fn mark_read(&mut self) {
        if matches!(self.activity, Activity::Ready { .. }) {
            self.activity = Activity::Idle;
        }
    }

    /// Update the activity following the harness's `rule`.
    ///
    /// A pane finishing while `visible` goes straight to idle. Returns whether the activity
    /// changed.
    pub fn update(
        &mut self,
        now: Instant,
        rule: ActivityRule,
        shell_in_front: bool,
        visible: bool,
    ) -> bool {
        let previous = self.activity;

        match (rule, self.activity) {
            (ActivityRule::OutputQuiet, Activity::Working { .. }) => {
                if self.is_quiet(now) {
                    self.activity = Self::finished(now, visible);
                }
            },
            (ActivityRule::OutputQuiet, _) => {
                if let Some(since) = self.busy_since(now) {
                    self.activity = Activity::Working { since };
                }
            },
            (ActivityRule::ForegroundProgram, Activity::Working { since }) if shell_in_front => {
                self.activity = if now.duration_since(since) >= MIN_COMMAND_DURATION {
                    Self::finished(now, visible)
                } else {
                    Activity::Idle
                };
            },
            (ActivityRule::ForegroundProgram, Activity::Working { .. }) => (),
            (ActivityRule::ForegroundProgram, _) if !shell_in_front => {
                self.activity = Activity::Working { since: now };
            },
            (ActivityRule::ForegroundProgram, _) => (),
        }

        self.activity != previous
    }

    fn finished(now: Instant, visible: bool) -> Activity {
        if visible { Activity::Idle } else { Activity::Ready { since: now } }
    }

    fn is_quiet(&self, now: Instant) -> bool {
        self.last_output.is_none_or(|last| now.duration_since(last) >= QUIET_THRESHOLD)
    }

    /// Start of the current run of output, if it has lasted long enough to count as work.
    fn busy_since(&self, now: Instant) -> Option<Instant> {
        let (start, last) = (self.output_start?, self.last_output?);
        let busy = last.duration_since(start) >= BUSY_THRESHOLD && !self.is_quiet(now);
        busy.then_some(start)
    }
}

/// Short duration label, like `12s`, `4m` or `2h`.
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: f32) -> Duration {
        Duration::from_secs_f32(seconds)
    }

    /// Print output every 100ms between `from` and `to` seconds.
    fn print(tracker: &mut ActivityTracker, start: Instant, from: f32, to: f32) {
        let mut time = from;
        while time <= to {
            tracker.on_output(start + secs(time));
            time += 0.1;
        }
    }

    #[test]
    fn agent_finishing_in_background_is_ready() {
        let start = Instant::now();
        let mut tracker = ActivityTracker::default();

        print(&mut tracker, start, 0., 3.);
        assert!(tracker.update(start + secs(3.), ActivityRule::OutputQuiet, false, false));
        assert_eq!(tracker.activity(), Activity::Working { since: start });

        assert!(!tracker.update(start + secs(4.), ActivityRule::OutputQuiet, false, false));
        assert!(tracker.update(start + secs(5.5), ActivityRule::OutputQuiet, false, false));
        assert_eq!(tracker.activity(), Activity::Ready { since: start + secs(5.5) });

        tracker.mark_read();
        assert_eq!(tracker.activity(), Activity::Idle);
    }

    #[test]
    fn agent_finishing_while_watched_is_idle() {
        let start = Instant::now();
        let mut tracker = ActivityTracker::default();

        print(&mut tracker, start, 0., 3.);
        tracker.update(start + secs(3.), ActivityRule::OutputQuiet, false, true);
        tracker.update(start + secs(6.), ActivityRule::OutputQuiet, false, true);
        assert_eq!(tracker.activity(), Activity::Idle);
    }

    #[test]
    fn short_output_and_typing_are_not_work() {
        let start = Instant::now();
        let mut tracker = ActivityTracker::default();

        // A redraw after a resize.
        print(&mut tracker, start, 0., 0.5);
        assert!(!tracker.update(start + secs(0.5), ActivityRule::OutputQuiet, false, false));

        // Typing a long prompt.
        let mut time = 5.;
        while time <= 10. {
            tracker.on_input(start + secs(time));
            tracker.on_output(start + secs(time + 0.05));
            time += 0.2;
        }
        assert!(!tracker.update(start + secs(10.), ActivityRule::OutputQuiet, false, false));
        assert_eq!(tracker.activity(), Activity::Idle);
    }

    #[test]
    fn long_commands_are_ready_when_done() {
        let start = Instant::now();
        let mut tracker = ActivityTracker::default();
        let rule = ActivityRule::ForegroundProgram;

        assert!(tracker.update(start, rule, false, false));
        assert!(tracker.update(start + secs(6.), rule, true, false));
        assert_eq!(tracker.activity(), Activity::Ready { since: start + secs(6.) });

        // Quick commands go straight back to idle.
        let mut tracker = ActivityTracker::default();
        tracker.update(start, rule, false, false);
        tracker.update(start + secs(1.), rule, true, false);
        assert_eq!(tracker.activity(), Activity::Idle);
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(secs(12.)), "12s");
        assert_eq!(format_duration(secs(250.)), "4m");
        assert_eq!(format_duration(secs(7300.)), "2h");
    }
}
