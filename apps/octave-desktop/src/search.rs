//! A single worker and a replaceable pending request bound search concurrency.
use octave_core::Track;
use octave_provider::{Error, Provider};
use std::sync::{
    Arc, Condvar, Mutex,
    mpsc::{self, Receiver},
};
type Pending = Arc<(Mutex<(bool, Option<(u64, String)>)>, Condvar)>;
pub struct Search {
    pending: Pending,
    pub results: Receiver<(u64, Result<Vec<Track>, Error>)>,
    pub generation: u64,
}
impl Search {
    pub fn new() -> Self {
        let pending: Pending = Arc::new((Mutex::new((false, None)), Condvar::new()));
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
                drop(state);
                let result = Provider::new().and_then(|p| p.search(&query));
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
        self.pending.0.lock().unwrap().1 = None;
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
        self.pending.0.lock().unwrap().0 = true;
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
        s.invalidate();
        assert!(!s.accepts(old));
        assert!(s.accepts(s.generation));
    }
}
