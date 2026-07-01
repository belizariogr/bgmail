//! Lightweight startup-time instrumentation.
//!
//! Records monotonic milestones from process entry through the first ready frame
//! so we can track regressions while the mock is still evolving. Output goes to
//! stderr in debug builds only.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();

/// Captures the process start instant. Safe to call more than once.
pub fn mark_start() {
    let _ = START.set(Instant::now());
}

/// Elapsed time since [`mark_start`], or zero if it was never called.
pub fn elapsed() -> Duration {
    START.get().map(|start| start.elapsed()).unwrap_or_default()
}

/// Formats a duration for log output.
pub fn format_elapsed(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1000.0)
}

/// Logs a named milestone when building in debug mode.
pub fn log_milestone(label: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[rMail startup] {label}: {}", format_elapsed(elapsed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn format_elapsed_shows_milliseconds() {
        assert_eq!(format_elapsed(Duration::from_millis(42)), "42.0ms");
    }

    #[test]
    fn elapsed_grows_after_mark_start() {
        mark_start();
        let first = elapsed();
        thread::sleep(Duration::from_millis(2));
        assert!(elapsed() >= first);
    }
}
