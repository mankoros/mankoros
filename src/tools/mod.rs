pub mod arena;
pub mod errors;
pub mod handler_pool;
pub mod hash;
pub mod pointers;
pub mod sync_ptr;
pub mod user_check;

#[macro_export]
macro_rules! when_debug {
    ($blk:block) => {
        cfg_if::cfg_if! {
            if #[cfg(debug_assertions)] {
                $blk
            }
        }
    };
}

pub use when_debug;
