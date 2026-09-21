//! Only task IDs and wake signals cross threads; local futures never leave the UI.
use super::runtime::TaskId;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::Wake,
};

type WakeCallback = Arc<dyn Fn() + Send + Sync>;
#[derive(Default)]
struct QueueState {
    ready: VecDeque<TaskId>,
    notified: bool,
    closed: bool,
    callback: Option<WakeCallback>,
    notifications: u64,
}
#[derive(Default)]
pub(super) struct ReadyQueue(Mutex<QueueState>);
impl ReadyQueue {
    fn notify(state: &mut QueueState) -> Option<WakeCallback> {
        if state.closed || state.notified || state.ready.is_empty() {
            return None;
        }
        let callback = state.callback.clone()?;
        state.notified = true;
        state.notifications += 1;
        Some(callback)
    }
    fn push(&self, id: TaskId) {
        let callback = {
            let mut state = self.0.lock().unwrap();
            if state.closed {
                return;
            }
            state.ready.push_back(id);
            Self::notify(&mut state)
        };
        // A host callback may itself inspect the queue. Never call it under a lock.
        if let Some(callback) = callback {
            callback();
        }
    }
    pub fn set_waker(&self, callback: WakeCallback) {
        let callback = {
            let mut state = self.0.lock().unwrap();
            state.callback = Some(callback);
            state.notified = false;
            Self::notify(&mut state)
        };
        if let Some(callback) = callback {
            callback();
        }
    }
    pub fn take_batch(&self, batch: &mut VecDeque<TaskId>) {
        let mut state = self.0.lock().unwrap();
        std::mem::swap(&mut state.ready, batch);
        state.notified = false;
    }
    pub fn finish_batch(&self, batch: &mut VecDeque<TaskId>) {
        let callback = {
            let mut state = self.0.lock().unwrap();
            // Older ready tasks keep their place ahead of tasks woken this turn.
            if !batch.is_empty() {
                batch.append(&mut state.ready);
                std::mem::swap(&mut state.ready, batch);
            }
            Self::notify(&mut state)
        };
        if let Some(callback) = callback {
            callback();
        }
    }
    pub fn stats(&self) -> (usize, u64) {
        let state = self.0.lock().unwrap();
        (state.ready.len(), state.notifications)
    }
    pub fn close(&self) {
        let mut state = self.0.lock().unwrap();
        state.closed = true;
        state.ready.clear();
        state.callback = None;
    }
}

pub(super) struct Control {
    id: TaskId,
    queue: Weak<ReadyQueue>,
    queued: AtomicBool,
    pub cancelled: AtomicBool,
    pub finished: AtomicBool,
}
impl Control {
    pub fn new(id: TaskId, queue: &Arc<ReadyQueue>) -> Arc<Self> {
        Arc::new(Self {
            id,
            queue: Arc::downgrade(queue),
            queued: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        })
    }
    pub fn schedule(&self) {
        if !self.finished.load(Ordering::Acquire)
            && !self.queued.swap(true, Ordering::AcqRel)
            && let Some(queue) = self.queue.upgrade()
        {
            queue.push(self.id);
        }
    }
    pub fn before_poll(&self) {
        self.queued.store(false, Ordering::Release);
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.schedule();
    }
}
impl Wake for Control {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}
