use alloc::collections::VecDeque;
use async_task::Runnable;

use crate::sync::SpinNoIrqLock;

pub struct TaskQueue {
    queue: SpinNoIrqLock<VecDeque<Runnable>>,
}

/// 一个任务队列, 内含一队的 Runnable 对象
impl TaskQueue {
    pub const fn new() -> Self {
        Self {
            queue: SpinNoIrqLock::new(VecDeque::new()),
        }
    }

    /// 向队列中放入一个对象
    pub fn push(&self, task: Runnable) {
        self.queue.lock(here!()).push_back(task);
    }

    /// 从队列中取出第一个对象
    pub fn fetch(&self) -> Option<Runnable> {
        self.queue.lock(here!()).pop_front()
    }
}
