use super::get_time_ms;
use crate::{
    executor::util_futures::{always_pending, get_waker},
    sync::SpinNoIrqLock,
};
use alloc::collections::BinaryHeap;
use core::{
    cmp::Reverse,
    mem::MaybeUninit,
    pin::Pin,
    task::{Poll, Waker},
};
use futures::Future;

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
    let r = SleepFuture::new(get_time_ms() + ms, always_pending()).await;
    debug_assert!(r.is_none());
}

pub async fn with_timeout<F: Future>(ms: usize, future: F) -> Option<F::Output> {
    SleepFuture::new(get_time_ms() + ms, future).await
}

struct SleepFuture<F: Future> {
    wake_up_time: AbsTimeT,
    is_done: bool,
    is_registered: bool,
    future: F,
}
impl<F: Future> SleepFuture<F> {
    fn new(wake_up_time: AbsTimeT, future: F) -> Self {
        Self {
            wake_up_time,
            is_done: false,
            is_registered: false,
            future,
        }
    }
}

impl<F: Future> Future for SleepFuture<F> {
    type Output = Option<F::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };

        if this.is_done {
            return Poll::Pending;
        }

        let mut inner = unsafe { Pin::new_unchecked(&mut this.future) };
        if let Poll::Ready(v) = inner.as_mut().poll(cx) {
            this.is_done = true;
            return Poll::Ready(Some(v));
        }

        if this.wake_up_time <= get_time_ms() {
            this.is_done = true;
            return Poll::Ready(None);
        }

        if !this.is_registered {
            this.is_registered = true;
            get_sleep_queue().push(Node {
                wake_up_time: this.wake_up_time,
                waker: cx.waker().clone(),
            });
        }

        Poll::Pending
    }
}

pub(super) fn at_tick() {
    get_sleep_queue().wake_ready(get_time_ms());
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
            log::trace!(
                "wake_ready: now: {}, node.wake_up_time: {}",
                now,
                node.wake_up_time
            );
            if node.wake_up_time > now {
                break;
            }
            let node = heap.pop().unwrap().0;
            node.waker.wake();
        }
    }
}
