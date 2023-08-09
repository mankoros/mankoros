use super::new_vfs::{
    top::{PollKind, VfsFile, VfsFileRef},
    DeviceIDCollection, VfsFileAttr,
};
use crate::{
    consts::MAX_PIPE_SIZE,
    here, impl_vfs_default_non_dir,
    sync::SpinNoIrqLock,
    tools::errors::{dyn_future, ASysResult, SysError, SysResult},
};
use alloc::sync::Arc;
use core::{
    pin::Pin,
    task::{Context, Poll, Waker},
};
use futures::Future;
use ringbuffer::{AllocRingBuffer, RingBuffer, RingBufferRead, RingBufferWrite};

fn pipe_attr() -> VfsFileAttr {
    VfsFileAttr {
        kind: super::new_vfs::VfsFileKind::Pipe,
        device_id: DeviceIDCollection::PIPE_FS_ID,
        self_device_id: 0,
        byte_size: 0,
        block_count: 0,
        access_time: 0,
        modify_time: 0,
        create_time: 0,
    }
}

macro_rules! impl_vfs_default_pipe {
    ($id:ident) => {
        fn attr(&self) -> ASysResult<VfsFileAttr> {
            dyn_future(async { Ok(pipe_attr()) })
        }
        fn as_any(&self) -> &dyn core::any::Any {
            self
        }

        fn get_page(
            &self,
            _offset: usize,
            _kind: super::new_vfs::top::MmapKind,
        ) -> ASysResult<crate::memory::address::PhysAddr4K> {
            unimplemented!(concat!(stringify!($id), "::get_page"))
        }
        fn truncate(&self, _length: usize) -> ASysResult {
            unimplemented!(concat!(stringify!($id), "::truncate"))
        }

        fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
            dyn_future(async move {
                self.poll_ready(offset, buf.len(), PollKind::Read).await?;
                Ok(self.poll_read(offset, buf))
            })
        }

        fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
            dyn_future(async move {
                self.poll_ready(offset, buf.len(), PollKind::Write).await?;
                Ok(self.poll_write(offset, buf))
            })
        }
    };
}

pub struct Pipe {
    is_closed: bool,
    buf: AllocRingBuffer<u8>,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
}

impl Pipe {
    fn new() -> Self {
        Self {
            is_closed: false,
            buf: AllocRingBuffer::with_capacity(MAX_PIPE_SIZE),
            read_waker: None,
            write_waker: None,
        }
    }
    pub fn new_pipe() -> (VfsFileRef, VfsFileRef) {
        let pipe = Arc::new(SpinNoIrqLock::new(Pipe::new()));
        let read_end = VfsFileRef::new(PipeReadEnd { pipe: pipe.clone() });
        let write_end = VfsFileRef::new(PipeWriteEnd { pipe });
        (read_end, write_end)
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        if !self.is_closed {
            panic!("Pipe is dropped without being closed");
        }
        if !self.buf.is_empty() {
            log::debug!("Pipe is dropped without being empty");
        }
    }
}

pub struct PipeReadEnd {
    pipe: Arc<SpinNoIrqLock<Pipe>>,
}

impl VfsFile for PipeReadEnd {
    impl_vfs_default_non_dir!(PipeReadEnd);
    impl_vfs_default_pipe!(PipeReadEnd);

    fn poll_ready(&self, _offset: usize, _len: usize, kind: PollKind) -> ASysResult<usize> {
        dyn_future(async move {
            match kind {
                PollKind::Read => PipeReadPollFuture::new(self.pipe.clone()).await,
                PollKind::Write => Err(SysError::EPERM),
            }
        })
    }

    fn poll_read(&self, _offset: usize, buf: &mut [u8]) -> usize {
        let mut pipe = self.pipe.lock(here!());
        debug_assert_ne!(pipe.buf.len(), 0);
        if buf.len() == 0 {
            // ensure we at least read one byte
            return 0;
        }

        let len = Ord::min(pipe.buf.len(), buf.len());
        for i in 0..len {
            buf[i] = pipe.buf.dequeue().expect("Just checked for len, should not fail");
        }

        if let Some(waker) = pipe.write_waker.take() {
            waker.wake();
        }

        len
    }

    fn poll_write(&self, _offset: usize, _buf: &[u8]) -> usize {
        unimplemented!("PipeReadEnd::poll_write")
    }
}

impl Drop for PipeReadEnd {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock(here!());
        pipe.is_closed = true;
        if let Some(waker) = pipe.write_waker.take() {
            waker.wake();
        }
    }
}

struct PipeReadPollFuture {
    pipe: Arc<SpinNoIrqLock<Pipe>>,
}
impl PipeReadPollFuture {
    fn new(pipe: Arc<SpinNoIrqLock<Pipe>>) -> Self {
        Self { pipe }
    }
}
impl Future for PipeReadPollFuture {
    type Output = SysResult<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let mut pipe = this.pipe.lock(here!());

        if pipe.buf.len() != 0 {
            Poll::Ready(Ok(pipe.buf.len()))
        } else {
            if pipe.is_closed {
                Poll::Ready(Err(SysError::EPIPE))
            } else {
                // debug_assert!(pipe.read_waker.is_none());
                if !pipe.read_waker.is_none() {
                    // 考虑一次带 timeout 的 pselect.
                    // 如果是 timeout 或者是其他 fd 成功返回了, 它就会在这里留下一个没有的 waker.
                    // 所以在设新 waker 时, 这里存在一个旧 waker 是可能的情况.
                    // 我们姑且直接替换掉旧 waker.
                    // 但是这样在真有多个进程一起读管道的时候可能会把某些进程彻底丢掉,
                    // 让调度器再也找不到那个进程. 这种情况还不知道应该如何处理.
                    log::debug!("PipeWrite: dropping old waker");
                }
                pipe.read_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

struct PipeWriteEnd {
    pipe: Arc<SpinNoIrqLock<Pipe>>,
}

impl VfsFile for PipeWriteEnd {
    impl_vfs_default_non_dir!(PipeWriteEnd);
    impl_vfs_default_pipe!(PipeWriteEnd);

    fn poll_ready(&self, _offset: usize, _len: usize, kind: PollKind) -> ASysResult<usize> {
        dyn_future(async move {
            match kind {
                PollKind::Read => Err(SysError::EPERM),
                PollKind::Write => PipeWritePollFuture::new(self.pipe.clone()).await,
            }
        })
    }

    fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
        unimplemented!("PipeWriteEnd::poll_read")
    }

    fn poll_write(&self, _offset: usize, buf: &[u8]) -> usize {
        let mut pipe = self.pipe.lock(here!());
        let space_left = pipe.buf.capacity() - pipe.buf.len();

        debug_assert_ne!(space_left, 0);
        if buf.len() == 0 {
            // ensure we at least write one byte
            return 0;
        }

        let len = Ord::min(space_left, buf.len());
        for i in 0..len {
            pipe.buf.push(buf[i]);
        }

        if let Some(waker) = pipe.read_waker.take() {
            waker.wake();
        }

        len
    }
}

impl Drop for PipeWriteEnd {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock(here!());
        pipe.is_closed = true;
        if let Some(waker) = pipe.read_waker.take() {
            waker.wake();
        }
    }
}

struct PipeWritePollFuture {
    pipe: Arc<SpinNoIrqLock<Pipe>>,
}
impl PipeWritePollFuture {
    fn new(pipe: Arc<SpinNoIrqLock<Pipe>>) -> Self {
        Self { pipe }
    }
}
impl Future for PipeWritePollFuture {
    type Output = SysResult<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let mut pipe = this.pipe.lock(here!());
        let space_left = pipe.buf.capacity() - pipe.buf.len();

        if pipe.is_closed {
            Poll::Ready(Err(SysError::EPIPE))
        } else {
            if space_left != 0 {
                Poll::Ready(Ok(pipe.buf.capacity() - pipe.buf.len()))
            } else {
                // debug_assert!(pipe.write_waker.is_none());
                if !pipe.write_waker.is_none() {
                    log::debug!("PipeWrite: dropping old waker");
                }
                pipe.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
