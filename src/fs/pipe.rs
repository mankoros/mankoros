//! 管道实现
//!
//! 相当于两个文件，其中一个只读，一个只可写，但指向同一片内存。
//! Pipe 的读写可能会触发进程切换。
//! 目前的实现中，Pipe位于内核堆
//! Adapted from Maturin OS.

use alloc::{boxed::Box, sync::Arc};
use ringbuffer::{AllocRingBuffer, RingBuffer, RingBufferRead, RingBufferWrite};

use crate::{
    axerrno::AxError, consts, executor::util_futures::yield_now, here, impl_vfs_non_dir_default,
    sync::SpinNoIrqLock,
};

use super::vfs::{
    filesystem::VfsNode,
    node::{VfsNodeAttr, VfsNodePermission, VfsNodeType},
    AVfsResult, VfsResult,
};

/// 管道本体，每次创建两份，一个是读端，一个是写端
pub struct Pipe {
    /// 标记是否是读的一端
    is_read: bool,
    /// 管道内保存的数据
    /// 只有所有持有管道的 Arc 被 Drop 时，才会释放其中的 PipeBuffer 的空间
    data: Arc<SpinNoIrqLock<AllocRingBuffer<u8>>>,
}

impl Pipe {
    /// 新建一个管道，返回两端
    pub fn new_pipe() -> (Self, Self) {
        let buf = Arc::new(SpinNoIrqLock::new(AllocRingBuffer::with_capacity(
            consts::MAX_PIPE_SIZE,
        )));
        (
            Self {
                is_read: true,
                data: buf.clone(),
            },
            Self {
                is_read: false,
                data: buf,
            },
        )
    }
}

impl VfsNode for Pipe {
    impl_vfs_non_dir_default! {}

    fn write_at<'a>(&'a self, _offset: u64, buf: &'a [u8]) -> AVfsResult<usize> {
        Box::pin(async move {
            // Check if the pipe is writable
            if self.is_read {
                return Err(AxError::Unsupported);
            }

            let buf_len = buf.len();

            let mut data = loop {
                // Acquire the lock
                let data = self.data.lock(here!());
                // Check if the buffer is enough
                if data.capacity() - data.len() >= buf_len {
                    break data;
                }
                // Release the lock
                drop(data);
                // Wait for next round
                yield_now().await;
            };
            for b in buf.iter() {
                data.push(*b);
            }
            // Auto release lock
            Ok(buf_len)
        })
    }

    fn fsync(&self) -> VfsResult {
        crate::ax_err!(Unsupported)
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        crate::ax_err!(Unsupported)
    }
    fn read_at<'a>(&'a self, _offset: u64, buf: &'a mut [u8]) -> AVfsResult<usize> {
        Box::pin(async move {
            // Check if the pipe is readable
            if !self.is_read {
                return Err(AxError::Unsupported);
            }
            let buf_len = buf.len();
            let mut data = loop {
                // Acquire the lock
                let data = self.data.lock(here!());
                // Check if the buffer is enough
                if data.len() >= buf_len {
                    break data;
                }
                // Release the lock
                drop(data);
                // Wait for next round
                yield_now().await;
            };
            for i in 0..buf_len {
                buf[i] = data.dequeue().expect("Just checked for len, should not fail");
            }
            Ok(buf_len)
        })
    }
    /// 文件属性
    fn stat(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            VfsNodePermission::all(),
            VfsNodeType::CharDevice,
            0,
            0,
        ))
    }
}
