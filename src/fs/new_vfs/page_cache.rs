use super::{
    sync_attr_cache::SyncAttrCacheFile,
    top::{MmapKind, VfsFile},
    underlying::ConcreteFile,
};
use crate::{
    consts::PAGE_SIZE,
    impl_vfs_default_non_dir,
    memory::{
        address::{PhysAddr, PhysAddr4K},
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

pub struct SyncPageCacheFile<F: ConcreteFile> {
    mgr: SleepLock<PageManager<F>>,
    file: SyncAttrCacheFile<F>,
}

impl<F: ConcreteFile> SyncPageCacheFile<F> {
    pub fn new(file: SyncAttrCacheFile<F>) -> Self {
        Self {
            mgr: SleepLock::new(PageManager::new()),
            file,
        }
    }
}

impl<F: ConcreteFile> VfsFile for SyncPageCacheFile<F> {
    fn attr(&self) -> ASysResult<super::VfsFileAttr> {
        dyn_future(async { Ok(self.file.with_attr_read(|attr| attr.clone())) })
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            let mut mgr = self.mgr.lock().await;
            mgr.perpare_range(&self.file, offset, buf.len()).await?;
            Ok(mgr.cached_read(offset, buf))
        })
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            let page_addr = PhysAddr::from(offset).round_down().bits();
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
            Ok(addr)
        })
    }

    impl_vfs_default_non_dir!(SyncPageCacheFile);
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
        file: &SyncAttrCacheFile<F>,
        offset: usize,
        len: usize,
    ) -> SysResult<()> {
        let begin = PhysAddr::from(offset).round_down().bits();
        let end = PhysAddr::from(offset + len).round_up().bits();

        for page_begin in (begin..end).step_by(PAGE_SIZE) {
            if !self.cached_pages.contains_key(&(page_begin as usize)) {
                let page = CachedPage::alloc()?;
                let len = file.lock().await.read_at(page_begin, page.for_read()).await?;
                page.set_len(len);
                self.cached_pages.insert(page_begin, page);

                // 读到文件尾了
                if len != PAGE_SIZE {
                    break;
                }
            }
        }

        Ok(())
    }

    pub async fn get_page(
        &mut self,
        file: &SyncAttrCacheFile<F>,
        offset: usize,
    ) -> SysResult<PhysAddr4K> {
        let page_addr = PhysAddr::from(offset).round_down().bits();
        self.perpare_range(file, page_addr, 1).await?;
        let page = self.cached_pages.get(&page_addr).unwrap();
        Ok(page.addr())
    }

    /// 从缓存中读取数据, 返回读取的长度.
    /// 要求文件 [offset, offset+len) 范围内的内容都已经在缓存中了,
    /// 缓存中找不到的页就当是文件没有了.
    pub fn cached_read(&self, offset: usize, buf: &mut [u8]) -> usize {
        let mut total_len = 0; // 读取的总长度
        let mut target_buf = buf; // 目标区域, 会随着读取逐渐向后取
        let mut page_buf; // 缓存页的有效区域, 会一页页向后寻找并取得
        let mut page_addr = PhysAddr::from(offset).round_down().bits();

        // 调整开头, 因为 offset 一般而言不会对齐到页, 所以我们要向前找到最近的页地址并尝试寻找该页
        // 随后裁剪该页的有效区域的前面的不会被读取的部分作为 page_buf
        let page = match self.cached_pages.get(&page_addr) {
            Some(page) => page,
            None => return total_len,
        };
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
                if page_buf_len != PAGE_SIZE {
                    return total_len;
                }

                // 否则继续读下一页
                target_buf = &mut target_buf[page_buf_len..];
                page_buf = match self.cached_pages.get(&page_addr) {
                    Some(page) => page.as_slice(),
                    // 如果没有下一页了, 那也是读到文件尾了
                    None => return total_len,
                };
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
    pub fn cached_write(&mut self, offset: usize, mut buf: &[u8]) {
        let mut target_buf = buf; // 目标区域, 会随着写入逐渐向后取
        let mut page_buf;
        let mut page_addr = PhysAddr::from(offset).round_down().bits();

        // 不用管什么有效长度了, 写入的话写就是了
        page_buf = self.get_or_alloc(page_addr).as_mut_slice();
        page_addr += PAGE_SIZE;

        loop {
            let target_len = target_buf.len();
            if target_len > PAGE_SIZE {
                page_buf.copy_from_slice(&target_buf[..PAGE_SIZE]);
                target_buf = &target_buf[PAGE_SIZE..];
                page_buf = self.get_or_alloc(page_addr).as_mut_slice();
                page_addr += PAGE_SIZE;
            } else {
                page_buf[..target_len].copy_from_slice(target_buf);
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
            effective_len: AtomicU32::new(0),
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
