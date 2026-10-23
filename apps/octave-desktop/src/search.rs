//! A single worker and a replaceable pending request bound search concurrency.
use octave_core::Track;
use octave_provider::{Error, Provider};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
type Pending = Arc<(
    Mutex<(bool, Option<(u64, String)>, Option<(u64, Arc<AtomicBool>)>)>,
    Condvar,
)>;
pub struct Search {
    pending: Pending,
    pub results: Receiver<(u64, Result<Vec<Track>, Error>)>,
    pub generation: u64,
}
impl Search {
    pub fn new() -> Self {
        let pending: Pending = Arc::new((Mutex::new((false, None, None)), Condvar::new()));
        let worker = pending.clone();
        let (tx, results) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let (lock, signal) = &*worker;
                let mut state = lock.lock().unwrap();
                while !state.0 && state.1.is_none() {
                    state = signal.wait(state).unwrap();
                }
                if state.0 {
                    break;
                }
                let (generation, query) = state.1.take().unwrap();
                let cancel = Arc::new(AtomicBool::new(false));
                state.2 = Some((generation, cancel.clone()));
                drop(state);
                let result =
                    Provider::new().and_then(|p| p.with_cancellation(cancel).search(&query));
                worker.0.lock().unwrap().2 = None;
                if tx.send((generation, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            pending,
            results,
            generation: 0,
        }
    }
    pub fn invalidate(&mut self) {
        self.generation += 1;
        let mut state = self.pending.0.lock().unwrap();
        state.1 = None;
        if let Some((_, cancel)) = &state.2 {
            cancel.store(true, Ordering::Release);
        }
    }
    pub fn submit(&self, query: String) {
        self.pending.0.lock().unwrap().1 = Some((self.generation, query));
        self.pending.1.notify_one();
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
}
impl Drop for Search {
    fn drop(&mut self) {
        let mut state = self.pending.0.lock().unwrap();
        state.0 = true;
        if let Some((_, cancel)) = &state.2 {
            cancel.store(true, Ordering::Release);
        }
        drop(state);
        self.pending.1.notify_one();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_invalidate_inflight_results_before_debounce() {
        let mut s = Search::new();
        let old = s.generation;
        let token = Arc::new(AtomicBool::new(false));
        s.pending.0.lock().unwrap().2 = Some((old, token.clone()));
        s.invalidate();
        assert!(!s.accepts(old));
        assert!(token.load(Ordering::Acquire));
        assert!(s.accepts(s.generation));
    }
}
