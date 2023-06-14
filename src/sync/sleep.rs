//! 睡眠锁

use super::SpinNoIrqLock;
use alloc::collections::VecDeque;
use core::{task::{Waker, Context, Poll}, cell::UnsafeCell, future::{Future}, pin::Pin, ops::{Deref, DerefMut}};
use crate::here;


/// 睡眠锁本体, 保存数据和等待队列
/// 使用方法: `let guard = A.lock().await;`
pub struct SleepLock<T: ?Sized> {
    inner: SpinNoIrqLock<SleepLockInner>,
    data: UnsafeCell<T>,
}

// 锁自然是可以在多个线程之间共享的
unsafe impl<T> Sync for SleepLock<T> {}
unsafe impl<T> Send for SleepLock<T> {}

/// 睡眠锁内部数据
/// 反正修改队列都要获取锁, 干脆把 flag 也放在里边
struct SleepLockInner {
    // holding 假 & 队列空: 无人持有锁
    // holding 真 & 队列空: 有人持有锁, 但是没有人在等待锁
    // holding 真 & 队列非空: 有人持有锁, 也有人在等待锁
    holding: bool,
    waiting: VecDeque<Waker>,
}

impl<T: ?Sized> SleepLock<T> {
    pub fn lock(&self) -> SleepLockFuture<'_, T> {
        SleepLockFuture { mutex: self }
    }
}

pub struct SleepLockFuture<'a, T: ?Sized + 'a> {
    mutex: &'a SleepLock<T>,
}

impl<'a, T> Future for SleepLockFuture<'a, T> {
    type Output = SleepLockGuard<'a, T>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let mut inner = this.mutex.inner.lock(here!());
        if inner.holding {
            // 如果锁已经被持有, 则将当前线程加入等待队列
            inner.waiting.push_back(cx.waker().clone());
            Poll::Pending
        } else {
            // 如果锁没有被持有, 则将锁标记为被持有, 并返回锁的 guard
            inner.holding = true;
            Poll::Ready(SleepLockGuard { mutex: this.mutex })
        }
    }
}

pub struct SleepLockGuard<'a, T: ?Sized + 'a> {
    mutex: &'a SleepLock<T>,
}

// 睡眠锁的 guard 不能跨线程传递, 或者说, 不能跨越 .await
// 这在根本上阻止了在持有睡眠锁 A 的情况下, 再因为获取睡眠锁 B 或等待其他 Future 而睡眠
// 防止了多个睡眠锁之间互相等待的死锁情况
impl<T> !Send for SleepLockGuard<'_, T> {}
impl<T> !Sync for SleepLockGuard<'_, T> {}

impl <'a, T: ?Sized> Drop for SleepLockGuard<'a, T> {
    fn drop(&mut self) {
        let mut inner = self.mutex.inner.lock(here!());
        // 因为新等待的人再次被唤醒时会获得新的 Guard, 而新的 guard.await 中会检查锁是否被持有
        // 所以即时下一个人马上会将这个 flag 设为 true, 也不能不修改它为 false
        // 否则下一个人会认为锁仍在被某人持有, 从而进入等待; 而再也不会有人来唤醒这个锁了
        inner.holding = false;
        // 当睡眠锁的 Guard 被 drop 时, 尝试唤醒等待队列中的第一个线程
        if let Some(waker) = inner.waiting.pop_front() {
            waker.wake();
        }
    }
}

impl<T> Deref for SleepLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<T> DerefMut for SleepLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}