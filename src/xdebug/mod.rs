mod backtrace;

pub use backtrace::*;

#[macro_export]
macro_rules! here {
    () => {
        concat!(file!(), ":", line!())
    };
}
