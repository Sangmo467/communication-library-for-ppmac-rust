use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// A broadcast generation counter: every waiter owns its cursor, so notifications cannot be stolen.
pub(crate) struct CompletionSignal {
    sequence: Mutex<u64>,
    changed: Condvar,
}
impl CompletionSignal {
    pub fn new() -> Self {
        Self {
            sequence: Mutex::new(0),
            changed: Condvar::new(),
        }
    }
    pub fn wake(&self) {
        let mut sequence = self
            .sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *sequence = sequence.saturating_add(1);
        drop(sequence);
        self.changed.notify_all();
    }
    pub fn wait(&self, cursor: &mut u64, wait_ms: u32) -> bool {
        let sequence = self
            .sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (sequence, _) = self
            .changed
            .wait_timeout_while(
                sequence,
                Duration::from_millis(u64::from(wait_ms)),
                |sequence| *sequence == *cursor,
            )
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let advanced = *sequence != *cursor;
        *cursor = *sequence;
        advanced
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    #[test]
    fn completion_between_poll_and_wait_is_not_lost() {
        let signal = CompletionSignal::new();
        let mut cursor = 0;
        assert!(!signal.wait(&mut cursor, 0));
        signal.wake();
        assert!(signal.wait(&mut cursor, 0));
        assert_eq!(cursor, 1);
        assert!(!signal.wait(&mut cursor, 0));
    }
    #[test]
    fn independent_waiters_do_not_consume_each_others_notifications() {
        let signal = Arc::new(CompletionSignal::new());
        let ready = Arc::new(Barrier::new(3));
        let threads = (0..2)
            .map(|_| {
                let signal = signal.clone();
                let ready = ready.clone();
                std::thread::spawn(move || {
                    let mut cursor = 0;
                    ready.wait();
                    assert!(signal.wait(&mut cursor, 1000));
                    cursor
                })
            })
            .collect::<Vec<_>>();
        ready.wait();
        signal.wake();
        for thread in threads {
            assert_eq!(thread.join().unwrap(), 1);
        }
    }
}
