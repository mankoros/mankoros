pub mod arena;
pub mod errors;
pub mod handler_pool;
pub mod hash;
pub mod pointers;
pub mod sync_ptr;
pub mod with_dirty;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Either<L, R> {
    pub fn new_left(l: L) -> Self {
        Self::Left(l)
    }
    pub fn new_right(r: R) -> Self {
        Self::Right(r)
    }
    pub fn is_left(&self) -> bool {
        matches!(self, Self::Left(_))
    }
    pub fn is_right(&self) -> bool {
        matches!(self, Self::Right(_))
    }
    pub fn left(&self) -> Option<&L> {
        match self {
            Self::Left(l) => Some(l),
            _ => None,
        }
    }
    pub fn right(&self) -> Option<&R> {
        match self {
            Self::Right(r) => Some(r),
            _ => None,
        }
    }
    pub fn left_mut(&mut self) -> Option<&mut L> {
        match self {
            Self::Left(l) => Some(l),
            _ => None,
        }
    }
    pub fn right_mut(&mut self) -> Option<&mut R> {
        match self {
            Self::Right(r) => Some(r),
            _ => None,
        }
    }
}
