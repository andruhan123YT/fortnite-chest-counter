//! With "start when Fortnite starts" the app goes back to waiting once the game closes:
//! when the last match is counted it starts a waiting copy of itself and quits, so it
//! takes nothing while the game is not running and opens again with the next game.

use std::time::{Duration, Instant};

/// Time after Fortnite closes for the last replay to be finished and counted.
pub const SETTLE: Duration = Duration::from_secs(60);
/// How often to look for the game.
pub const CHECK_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct Sleeper {
    pub checked_at: Option<Instant>,
    /// Fortnite ran at some point while the app was up.
    seen_running: bool,
    closed_at: Option<Instant>,
}

impl Sleeper {
    /// Takes whether Fortnite runs now; true once the app should go to sleep. It waits
    /// `SETTLE` after the game closed, until nothing is being counted, and while the
    /// player keeps the window open.
    pub fn observe(&mut self, running: bool, now: Instant, counting: bool, window_shown: bool) -> bool {
        if running {
            self.seen_running = true;
            self.closed_at = None;
            return false;
        }
        if !self.seen_running {
            return false;
        }
        let closed = *self.closed_at.get_or_insert(now);
        now.duration_since(closed) >= SETTLE && !counting && !window_shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeps_a_while_after_the_game_closes_unless_busy_or_shown() {
        let t = Instant::now();
        let at = |s: u64| t + Duration::from_secs(s);
        let mut s = Sleeper::default();
        // The game never ran: the app was opened by hand, it stays.
        assert!(!s.observe(false, at(0), false, false));
        assert!(!s.observe(false, at(600), false, false));

        assert!(!s.observe(true, at(700), false, false));
        assert!(!s.observe(false, at(710), false, false));
        assert!(!s.observe(false, at(760), false, false));
        // A replay is still being counted.
        assert!(!s.observe(false, at(771), true, false));
        // The window is open.
        assert!(!s.observe(false, at(772), false, true));
        assert!(s.observe(false, at(773), false, false));

        // The game started again before the time was up: the clock starts over.
        let mut s = Sleeper::default();
        s.observe(true, at(0), false, false);
        s.observe(false, at(10), false, false);
        s.observe(true, at(40), false, false);
        assert!(!s.observe(false, at(80), false, false));
        assert!(s.observe(false, at(140), false, false));
    }
}
