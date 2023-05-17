mod mutex;

pub type SpinNoIrqLock<T> = mutex::Mutex<T, mutex::SpinNoIrq>;
