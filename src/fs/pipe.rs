//! 管道实现
//!
//! 相当于两个文件，其中一个只读，一个只可写，但指向同一片内存。
//! Pipe 的读写可能会触发进程切换。
//! 目前的实现中，Pipe位于内核堆
//! Adapted from Maturin OS.

use alloc::{boxed::Box, sync::Arc};
use ringbuffer::{AllocRingBuffer, RingBuffer, RingBufferRead, RingBufferWrite};

use crate::{
    consts, executor::util_futures::yield_now, here,
    sync::SpinNoIrqLock, impl_vfs_default_non_dir, tools::errors::{ASysResult, dyn_future, SysError},
};
use super::new_vfs::{top::VfsFile, VfsFileAttr, DeviceIDCollection};


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

    fn is_hang_up(&self) -> bool {
        if self.is_read {
            self.data.lock(here!()).is_empty() && Arc::strong_count(&self.data) < 2
        } else {
            Arc::strong_count(&self.data) < 2
        }
    }
}

impl VfsFile for Pipe {
    impl_vfs_default_non_dir!(Pipe);

    fn write_at<'a>(&'a self, _offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        Box::pin(async move {
            // Check if the pipe is writable
            if self.is_read {
                return Err(SysError::EPERM);
            }
            // Check if the pipe is hang up
            if self.is_hang_up() {
                return Ok(0);
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

    fn read_at<'a>(&'a self, _offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        Box::pin(async move {
            // Check if the pipe is readable
            if !self.is_read {
                return Err(SysError::EPERM);
            }
            // Check if the pipe is hang up
            if self.is_hang_up() {
                return Ok(0);
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

    fn get_page(&self, _offset: usize, _kind: super::new_vfs::top::MmapKind) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Should never get page for a pipe")
    }

    /// 文件属性
    fn attr(&self) -> ASysResult<VfsFileAttr> {
        dyn_future(async {
            Ok(VfsFileAttr {
                kind: super::new_vfs::VfsFileKind::Pipe,
                device_id: DeviceIDCollection::PIPE_FS_ID,
                self_device_id: 0,
                byte_size: 0,
                block_count: 0,
                access_time: 0,
                modify_time: 0,
                create_time: 0,
            })
        })
    }
}
