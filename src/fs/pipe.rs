//! 管道实现
//!
//! 相当于两个文件，其中一个只读，一个只可写，但指向同一片内存。
//! Pipe 的读写可能会触发进程切换。
//! 目前的实现中，Pipe位于内核堆
//! Adapted from Maturin OS.

use alloc::{boxed::Box, sync::Arc};
use ringbuffer::{AllocRingBuffer, RingBuffer, RingBufferRead, RingBufferWrite};

use super::new_vfs::{top::VfsFile, DeviceIDCollection, VfsFileAttr};
use crate::{
    consts,
    executor::util_futures::yield_now,
    here, impl_vfs_default_non_dir,
    sync::SpinNoIrqLock,
    tools::errors::{dyn_future, ASysResult, SysError},
};
use core::cmp::min;

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
        Arc::strong_count(&self.data) < 2
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let only_one = Arc::strong_count(&self.data) == 1;
        let has_data = !self.data.lock(here!()).is_empty();
        if only_one && has_data {
            panic!("Pipe is not empty when dropped, very wrong");
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

            if self.is_hang_up() {
                // don't allow writing to a closed pipe
                log::info!("write to a closed pipe");
                return Ok(0);
            }

            loop {
                let mut data = self.data.lock(here!());
                if data.len() + buf.len() <= data.capacity() {
                    for i in 0..buf.len() {
                        data.enqueue(buf[i]);
                    }
                    return Ok(buf.len());
                } else {
                    // wait for next round
                    drop(data);
                    yield_now().await;
                }
            }
        })
    }

    fn read_at<'a>(&'a self, _offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        Box::pin(async move {
            // Check if the pipe is readable
            if !self.is_read {
                return Err(SysError::EPERM);
            }

            loop {
                let mut data = self.data.lock(here!());
                log::debug!(
                    "pipe read loop: data_len: {}, buf_len: {}, ref_cnt: {}",
                    data.len(),
                    buf.len(),
                    Arc::strong_count(&self.data)
                );

                if data.len() >= buf.len() {
                    for i in 0..buf.len() {
                        buf[i] = data.dequeue().unwrap();
                    }
                    return Ok(buf.len());
                } else if self.is_hang_up() {
                    // return leftover data
                    // must save len first here
                    let read_len = data.len();
                    for i in 0..data.len() {
                        buf[i] = data.dequeue().unwrap();
                    }
                    return Ok(read_len);
                } else {
                    // wait for next round
                    drop(data);
                    yield_now().await;
                    continue;
                }
            }
        })
    }

    fn get_page(
        &self,
        _offset: usize,
        _kind: super::new_vfs::top::MmapKind,
    ) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Should never get page for a pipe")
    }

    fn poll_ready(
        &self,
        _offset: usize,
        len: usize,
        kind: super::new_vfs::top::PollKind,
    ) -> ASysResult<usize> {
        dyn_future(async move {
            let poll_is_read = kind == super::new_vfs::top::PollKind::Read;
            if poll_is_read != self.is_read {
                return Err(SysError::EPERM);
            }

            if poll_is_read {
                loop {
                    let data = self.data.lock(here!());
                    if data.len() >= len {
                        break Ok(len);
                    } else if self.is_hang_up() {
                        break Ok(data.len());
                    } else {
                        drop(data);
                        yield_now().await;
                    }
                }
            } else {
                loop {
                    if self.is_hang_up() {
                        // don't allow writing to a closed pipe
                        return Ok(0);
                    }

                    let data = self.data.lock(here!());
                    if data.capacity() - data.len() >= len {
                        break Ok(len);
                    } else {
                        drop(data);
                        yield_now().await;
                    }
                }
            }
        })
    }

    fn poll_read(&self, _offset: usize, buf: &mut [u8]) -> usize {
        debug_assert!(self.is_read);
        let mut data = self.data.lock(here!());
        let len = min(data.len(), buf.len());
        for i in 0..len {
            buf[i] = data.dequeue().expect("Just checked for len, should not fail");
        }
        len
    }

    fn poll_write(&self, _offset: usize, buf: &[u8]) -> usize {
        debug_assert!(!self.is_read);
        let mut data = self.data.lock(here!());
        let len = min(data.capacity() - data.len(), buf.len());
        for i in 0..len {
            data.push(buf[i]);
        }
        len
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

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
