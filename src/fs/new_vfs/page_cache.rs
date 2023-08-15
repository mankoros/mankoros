use super::{
    sync_attr_file::SyncAttrFile,
    top::{MmapKind, VfsFile},
    underlying::ConcreteFile,
};
use crate::{
    consts::PAGE_SIZE,
    executor::block_on,
    impl_vfs_default_non_dir,
    memory::{
        address::{PhysAddr, PhysAddr4K, VirtAddr},
        frame::alloc_frame,
    },
    sync::SleepLock,
    tools::errors::{dyn_future, ASysResult, SysError, SysResult},
};
use alloc::collections::BTreeMap;
use core::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicU32},
};

pub struct PageCacheFile<F: ConcreteFile> {
    mgr: SleepLock<PageManager<F>>,
    pub(super) file: SyncAttrFile<F>,
}

impl<F: ConcreteFile> PageCacheFile<F> {
    pub fn new(file: SyncAttrFile<F>) -> Self {
        Self {
            mgr: SleepLock::new(PageManager::new()),
            file,
        }
    }
}

impl<F: ConcreteFile> VfsFile for PageCacheFile<F> {
    fn attr(&self) -> ASysResult<super::VfsFileAttr> {
        dyn_future(async {
            let mut attr = self.file.attr().await;
            let mgr = self.mgr.lock().await;
            let last = mgr.cached_pages.last_key_value();

            if let Some((begin_offset, page_cache)) = last {
                let end_offset = begin_offset + page_cache.len();
                attr.byte_size = attr.byte_size.max(end_offset);
            }

            Ok(attr)
        })
    }

    fn set_time(&self, time: [usize; 3]) -> ASysResult {
        dyn_future(async move { self.file.set_time(time).await })
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            log::debug!(
                "PageCacheFile::read_at: offset={}, buf.len={}",
                offset,
                buf.len()
            );
            let mut mgr = self.mgr.lock().await;
            mgr.perpare_range(&self.file, offset, buf.len()).await?;
            Ok(mgr.cached_read(offset, buf))
        })
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            log::debug!(
                "PageCacheFile::write_at: offset={}, buf.len={}",
                offset,
                buf.len()
            );
            let page_addr = PhysAddr::from(offset).floor().bits();
            let mut mgr = self.mgr.lock().await;
            mgr.perpare_range(&self.file, page_addr, 1).await?;
            mgr.cached_write(offset, buf);
            Ok(buf.len())
        })
    }

    fn get_page(&self, offset: usize, kind: MmapKind) -> ASysResult<PhysAddr4K> {
        if kind != MmapKind::Private {
            panic!("SyncPageCacheFile::get_page: only support private mapping")
        }
        dyn_future(async move {
            let addr = self.mgr.lock().await.get_page(&self.file, offset).await?;
            let new_page = alloc_frame().ok_or(SysError::ENOMEM)?;
            unsafe { new_page.as_mut_page_slice().copy_from_slice(addr.as_page_slice()) };
            Ok(new_page)
        })
    }

    fn truncate(&self, length: usize) -> ASysResult {
        dyn_future(async move { self.file.truncate(length).await })
    }

    fn poll_ready(
        &self,
        offset: usize,
        len: usize,
        _kind: super::top::PollKind,
    ) -> ASysResult<usize> {
        dyn_future(
            async move { self.mgr.lock().await.perpare_range(&self.file, offset, len).await },
        )
    }

    fn poll_read(&self, offset: usize, buf: &mut [u8]) -> usize {
        let mgr = block_on(self.mgr.lock());
        mgr.cached_read(offset, buf)
    }

    fn poll_write(&self, offset: usize, buf: &[u8]) -> usize {
        let mut mgr = block_on(self.mgr.lock());
        mgr.cached_write(offset, buf);
        buf.len()
    }

    impl_vfs_default_non_dir!(SyncPageCacheFile);
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}

// 直接在最外层上大锁好了
// TODO: 更好的页缓存
struct PageManager<F: ConcreteFile> {
    cached_pages: BTreeMap<usize, CachedPage>,
    _phantom: PhantomData<F>,
}

impl<F: ConcreteFile> PageManager<F> {
    pub fn new() -> Self {
        Self {
            cached_pages: BTreeMap::new(),
            _phantom: PhantomData,
        }
    }

    pub async fn perpare_range(
        &mut self,
        file: &SyncAttrFile<F>,
        offset: usize,
        len: usize,
    ) -> SysResult<usize> {
        let begin = VirtAddr::from(offset).floor().bits();
        let end = VirtAddr::from(offset + len).ceil().bits();

        let mut total_len = 0;
        for page_begin in (begin..end).step_by(PAGE_SIZE) {
            if !self.cached_pages.contains_key(&{ page_begin }) {
                let page = CachedPage::alloc()?;
                let len = file.lock().await.read_page_at(page_begin, page.for_read()).await?;
                page.set_len(len);
                total_len += len;
                self.cached_pages.insert(page_begin, page);

                // 读到文件尾了
                if len != PAGE_SIZE {
                    break;
                }
            }
        }

        Ok(total_len)
    }

    pub async fn get_page(
        &mut self,
        file: &SyncAttrFile<F>,
        offset: usize,
    ) -> SysResult<PhysAddr4K> {
        let page_addr = PhysAddr::from(offset).floor().bits();
        self.perpare_range(file, page_addr, 1).await?;
        let page = self.cached_pages.get(&page_addr).unwrap();
        let new_page = alloc_frame().ok_or(SysError::ENOMEM)?;
        unsafe { new_page.as_mut_page_slice().copy_from_slice(page.as_slice()) }
        Ok(new_page)
    }

    /// 从缓存中读取数据, 返回读取的长度.
    /// 要求文件 [offset, offset+len) 范围内的内容都已经在缓存中了,
    /// 缓存中找不到的页就当是文件没有了.
    pub fn cached_read(&self, offset: usize, buf: &mut [u8]) -> usize {
        let mut total_len = 0; // 读取的总长度
        let mut target_buf = buf; // 目标区域, 会随着读取逐渐向后取
        let mut page; // 缓存页
        let mut page_buf; // 缓存页的有效区域, 会一页页向后寻找并取得
        let mut page_addr = PhysAddr::from(offset).floor().bits();

        // 调整开头, 因为 offset 一般而言不会对齐到页, 所以我们要向前找到最近的页地址并尝试寻找该页
        // 随后裁剪该页的有效区域的前面的不会被读取的部分作为 page_buf
        page = match self.cached_pages.get(&page_addr) {
            Some(page) => page,
            None => return total_len,
        };

        if (offset - page_addr) >= page.len() {
            // 如果第一页就是文件末尾, 就直接返回
            return total_len;
        }
        page_buf = &page.as_slice()[offset - page_addr..];
        if page_buf.is_empty() {
            return total_len;
        }
        page_addr += PAGE_SIZE;

        loop {
            // 此后依次拷贝 buf, 并更新 buf 的指向
            // 主要需要考虑两个 buf 的长度的关系
            let page_buf_len = page_buf.len();
            let target_buf_len = target_buf.len();
            if target_buf_len > page_buf_len {
                // 如果目标 buf 还比当前页长, 就直接读完当前页
                target_buf[..page_buf_len].copy_from_slice(page_buf);
                total_len += page_buf_len;

                // 读到文件尾就直接返回
                if page.len() != PAGE_SIZE {
                    return total_len;
                }

                // 否则继续读下一页
                target_buf = &mut target_buf[page_buf_len..];
                page = match self.cached_pages.get(&page_addr) {
                    Some(page) => page,
                    // 如果没有下一页了, 那也是读到文件尾了
                    None => return total_len,
                };
                page_buf = page.as_slice();
                page_addr += PAGE_SIZE;
            } else {
                // 如果目标 buf 比当前页短或恰好等于, 就读完目标 buf 并返回
                target_buf.copy_from_slice(&page_buf[..target_buf_len]);
                total_len += target_buf_len;
                return total_len;
            }
        }
    }

    fn get_or_alloc(&mut self, idx: usize) -> &CachedPage {
        self.cached_pages.entry(idx).or_insert_with(|| CachedPage::alloc().unwrap())
    }

    /// 写入数据到缓存中, 必定能全部写入
    /// 要求文件 offset 所在的页范围中的内容必须已经在缓存中或本来就不存在, 以避免使用 async 读
    pub fn cached_write(&mut self, offset: usize, buf: &[u8]) {
        let mut total_offset = offset; // 写入的总偏移
        let mut target_buf = buf; // 目标区域, 会随着写入逐渐向后取
        let mut page_buf;
        let mut page_addr = PhysAddr::from(offset).floor().bits();

        // 不用管什么有效长度了, 写入的话写就是了
        page_buf = self.get_or_alloc(page_addr);
        page_addr += PAGE_SIZE;

        loop {
            let target_len = target_buf.len();
            if target_len > PAGE_SIZE {
                page_buf.set_len(PAGE_SIZE);
                page_buf.as_mut_slice().copy_from_slice(&target_buf[..PAGE_SIZE]);

                total_offset += PAGE_SIZE;
                target_buf = &target_buf[PAGE_SIZE..];
                page_buf = self.get_or_alloc(page_addr);
                page_addr += PAGE_SIZE;
            } else {
                let curr_buf_addr = page_addr - PAGE_SIZE;
                let curr_buf_offset = total_offset - curr_buf_addr;
                let curr_buf_new_len = curr_buf_offset + target_len;
                if page_buf.len() < curr_buf_new_len {
                    // TODO: 找一个更靠谱的方式更新文件长度
                    page_buf.set_len(curr_buf_new_len);
                }
                page_buf.as_mut_slice()[curr_buf_offset..curr_buf_new_len]
                    .copy_from_slice(target_buf);
                return;
            }
        }
    }
}

struct CachedPage {
    phys_addr: PhysAddr4K,
    // 有效长度, 大部分页内偏移量不会超过 u32
    // (不会有 4GiB 的页吧, 不会吧不会吧)
    // 所以为了节省内存, 上一个 u32, 刚好卡住对齐要求
    effective_len: AtomicU32,
    is_dirty: AtomicBool,
}

impl CachedPage {
    pub fn alloc() -> SysResult<Self> {
        let phys_addr = alloc_frame().ok_or(SysError::ENOMEM)?;
        Ok(Self::new(phys_addr))
    }

    pub fn new(phys_addr: PhysAddr4K) -> Self {
        Self {
            is_dirty: AtomicBool::new(false),
            effective_len: AtomicU32::new(PAGE_SIZE as u32),
            phys_addr,
        }
    }

    pub fn addr(&self) -> PhysAddr4K {
        self.phys_addr
    }

    pub fn for_read(&self) -> &mut [u8] {
        unsafe { self.phys_addr.as_mut_page_slice() }
    }

    pub fn set_len(&self, len: usize) {
        self.effective_len.store(len as u32, core::sync::atomic::Ordering::Relaxed);
    }
    pub fn len(&self) -> usize {
        self.effective_len.load(core::sync::atomic::Ordering::Relaxed) as usize
    }
    pub fn is_tail(&self) -> bool {
        self.len() != PAGE_SIZE
    }

    pub fn mark_dirty(&self) {
        self.is_dirty.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(core::sync::atomic::Ordering::Relaxed)
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { self.phys_addr.as_slice(self.len()) }
    }
    pub fn as_mut_slice(&self) -> &mut [u8] {
        self.mark_dirty();
        unsafe { self.phys_addr.as_mut_slice(self.len()) }
    }
}
