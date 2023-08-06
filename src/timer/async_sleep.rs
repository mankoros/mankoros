use crate::{executor::util_futures::get_waker, sync::SpinNoIrqLock};
use alloc::collections::BinaryHeap;
use core::{cmp::Reverse, mem::MaybeUninit, task::Waker};

type AbsTimeT = usize;

static mut SLEEP_QUEUE: MaybeUninit<SleepQueue> = MaybeUninit::uninit();

pub(super) fn init_sleep_queue() {
    unsafe {
        SLEEP_QUEUE = MaybeUninit::new(SleepQueue::new());
    }
}

fn get_sleep_queue() -> &'static SleepQueue {
    unsafe { &*SLEEP_QUEUE.as_ptr() }
}

pub async fn wake_after(ms: usize) {
    let waker = get_waker().await;
    let wake_up_time = crate::timer::get_time_ms() + ms;

    get_sleep_queue().push(Node {
        wake_up_time,
        waker,
    });
}

pub(super) fn at_tick() {
    let now = crate::timer::get_time_ms();
    get_sleep_queue().wake_ready(now);
}

struct Node {
    wake_up_time: AbsTimeT,
    waker: Waker,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.wake_up_time == other.wake_up_time
    }
}

impl Eq for Node {}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.wake_up_time.partial_cmp(&other.wake_up_time)
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.wake_up_time.cmp(&other.wake_up_time)
    }
}

struct SleepQueue {
    heap: SpinNoIrqLock<BinaryHeap<Reverse<Node>>>,
}

impl SleepQueue {
    fn new() -> Self {
        Self {
            heap: SpinNoIrqLock::new(BinaryHeap::new()),
        }
    }

    fn push(&self, node: Node) {
        self.heap.lock(here!()).push(Reverse(node));
    }

    fn wake_ready(&self, now: AbsTimeT) {
        let mut heap = self.heap.lock(here!());
        while let Some(Reverse(node)) = heap.peek() {
            if node.wake_up_time > now {
                break;
            }
            let node = heap.pop().unwrap().0;
            node.waker.wake();
        }
    }
}
