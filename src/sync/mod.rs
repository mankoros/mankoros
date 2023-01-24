pub mod mutex;

pub type SpinLock<T> = mutex::Mutex<T, mutex::Spin>;
