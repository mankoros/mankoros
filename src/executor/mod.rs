use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use async_task::{Runnable, Task};

use self::task_queue::TaskQueue;
use crate::lazy_static;

pub mod hart_local;
pub mod task_queue;
pub mod yield_future;

lazy_static! {
    // 这个 Queue 要用到 VecDeque, 需要内存系统初始化完成才能被初始化
    // 所以得放在 lazy_static! 里
    static ref TASK_QUEUE: TaskQueue = TaskQueue::new();
}

/// 将 Future 打包成 Task, 并放入全局队列中
/// 返回的 Runnable 用于唤醒 Future, Task 用于获取 Future 的结果
/// Task 内部的状态还维护了该 Future 是否位于队列中, 因此该抽象是必要的 (显然我们不想让一个 Future 多次被放入队列中)
pub fn spawn<F>(future: F) -> (Runnable, Task<F::Output>)
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    async_task::spawn(future, |runnable| TASK_QUEUE.push(runnable))
}

/// 持续运行队列中的任务, 直到队列为空
/// 注意队列为空不代表没有 Future 了, 只代表没有可以立即被执行的 Future 了
pub fn run_until_idle() {
    while let Some(task) = TASK_QUEUE.fetch() {
        task.run();
    }
}

/// 一直疯狂地调用 Future::poll (类似自旋锁一样一直 poll 它), 直到 Future 返回 Poll::Ready
pub fn block_on<F: Future + 'static>(future: F) -> F::Output {
    // 构建一个被调用了也什么都不干的 Waker 和对应的 Context
    fn new_noop_raw_waker() -> RawWaker {
        RawWaker::new(
            core::ptr::null(),
            &RawWakerVTable::new(|_| new_noop_raw_waker(), |_| {}, |_| {}, |_| {}),
        )
    }
    let noop_waker = unsafe { Waker::from_raw(new_noop_raw_waker()) };
    let mut noop_cx = Context::from_waker(&noop_waker);

    // 必须是可变的一个 pin 对象, 这样才能让 rustc 认为它要活过整个 loop 循环体 (因为循环里一直在用)
    // 如果不是 mut 的, 在第一次循环迭代的时候 rustc 就会认为它的所有权已经被移交了, 那么接下来的循环迭代
    // 就不能再从它这里拿到一个 future, 于是就会报所有权错
    let mut future_slot = pin!(future);
    loop {
        // 一直尝试去 poll Future, 直到它返回 Poll::Ready
        // 理论上这个 Future 里的代码会把 cx 传给别人, 但是别人调用没有也不需要有任何效果,
        // 反正我都一直在 Poll; 如果我下一次 poll 它的时候导致它返回的那个 await 操作还没好的话
        // 它就会直接再次返回 Pending, 不会有问题
        if let Poll::Ready(ret) = future_slot.as_mut().poll(&mut noop_cx) {
            return ret;
        }
    }
}
