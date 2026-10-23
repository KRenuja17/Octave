//! UI ownership of an optional background request, including drop/track change.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub struct RequestCancel(Arc<AtomicBool>);
impl RequestCancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn begin(&mut self) -> Arc<AtomicBool> {
        self.cancel();
        self.0 = Arc::new(AtomicBool::new(false));
        self.0.clone()
    }
}
impl Drop for RequestCancel {
    fn drop(&mut self) {
        self.cancel();
    }
}
