//! Cooperative cancellation shared between a sync worker and the blocking
//! protocol calls it makes.
//!
//! A flag alone can't interrupt a thread blocked in a socket read (IMAP IDLE,
//! a stalled server), so connections register a hook (typically
//! `TcpStream::shutdown` on a cloned socket) that [`CancelToken::cancel`] runs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Hook = Box<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    next_id: AtomicU64,
    hooks: Mutex<Vec<(u64, Hook)>>,
}

/// Clonable cancellation handle; clones share state.
#[derive(Clone, Default)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the flag and runs every registered hook once.
    pub fn cancel(&self) {
        if self.inner.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        let hooks = std::mem::take(&mut *self.lock_hooks());
        for (_, hook) in hooks {
            hook();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Registers `hook` to run on cancellation (immediately when already
    /// cancelled). Dropping the returned guard unregisters it, so a closed
    /// connection is never "shut down" again later.
    pub fn on_cancel(&self, hook: impl Fn() + Send + Sync + 'static) -> CancelGuard {
        if self.is_cancelled() {
            hook();
            return CancelGuard {
                token: self.clone(),
                id: None,
            };
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        self.lock_hooks().push((id, Box::new(hook)));
        CancelGuard {
            token: self.clone(),
            id: Some(id),
        }
    }

    fn lock_hooks(&self) -> std::sync::MutexGuard<'_, Vec<(u64, Hook)>> {
        // A panicking hook must not disable cancellation for everyone else.
        self.inner
            .hooks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn remove(&self, id: u64) {
        self.lock_hooks().retain(|(hook_id, _)| *hook_id != id);
    }

    #[cfg(test)]
    fn hook_count(&self) -> usize {
        self.lock_hooks().len()
    }
}

/// Unregisters its hook when dropped.
#[must_use = "dropping the guard unregisters the hook"]
pub struct CancelGuard {
    token: CancelToken,
    id: Option<u64>,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.token.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cancel_sets_flag_and_runs_hooks_once() {
        let token = CancelToken::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let _guard = token.on_cancel(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!token.is_cancelled());
        token.clone().cancel();
        token.cancel();
        assert!(token.is_cancelled());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dropped_guard_unregisters_hook() {
        let token = CancelToken::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        drop(token.on_cancel(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(token.hook_count(), 0);
        token.cancel();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn hook_registered_after_cancel_runs_immediately() {
        let token = CancelToken::new();
        token.cancel();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let _guard = token.on_cancel(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(token.hook_count(), 0);
    }

    #[test]
    fn debug_shows_state() {
        assert!(format!("{:?}", CancelToken::new()).contains("cancelled: false"));
    }
}
