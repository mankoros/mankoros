mod mutex;
mod sleep;

pub type SpinNoIrqLock<T> = mutex::Mutex<T, mutex::SpinNoIrq>;
pub type SpinNoIrqLockGuard<'a, T> = mutex::MutexGuard<'a, T, mutex::SpinNoIrq>;
pub type SleepLock<T> = sleep::SleepLock<T>;
pub type SleepLockFuture<'a, T> = sleep::SleepLockFuture<'a, T>;
pub type SleepLockGuard<'a, T> = sleep::SleepLockGuard<'a, T>;
