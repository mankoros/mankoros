pub mod arena;
pub mod errors;
pub mod handler_pool;
pub mod hash;
pub mod pointers;
pub mod sync_ptr;

#[macro_export]
macro_rules! when_debug {
    ($blk:expr) => {
        cfg_if::cfg_if! {
            if #[cfg(debug_assertions)] {
                $blk
            }
        }
    };
}

pub use when_debug;

/// debug 用的, 用于在 log 之间快速判定两个 buf 的内容是否相等
pub fn exam_hash(buf: &[u8]) -> usize {
    let mut h: usize = 5381;
    for c in buf {
        h = h.wrapping_mul(33).wrapping_add(*c as usize);
    }
    h
}
