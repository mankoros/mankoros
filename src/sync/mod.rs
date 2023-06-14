mod mutex;
mod sleep;

pub type SpinNoIrqLock<T> = mutex::Mutex<T, mutex::SpinNoIrq>;
pub type SleepLock<T> = sleep::SleepLock<T>;
