use crate::{memory::address::PhysPageNum, sync::SpinNoIrqLock};
use alloc::collections::BTreeMap;
use core::{
    cell::SyncUnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};
use log::debug;

static SHARED_FRAME_MANAGER: SpinNoIrqLock<SyncUnsafeCell<SharedFrameManager>> =
    SpinNoIrqLock::new(SyncUnsafeCell::new(SharedFrameManager::new()));

pub fn with_shared_frame_mgr(f: impl FnOnce(&mut SharedFrameManager)) {
    let sfmgr = SHARED_FRAME_MANAGER.lock(here!());
    f(unsafe { &mut *sfmgr.get() });
}

pub struct SharedFrameManager {
    map: BTreeMap<PhysPageNum, SharedFrameInfo>,
}

impl SharedFrameManager {
    const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    pub fn add_ref(&mut self, page: PhysPageNum) {
        let info = self
            .map
            .entry(page)
            .and_modify(|info| {
                info.increase();
            })
            .or_insert_with(SharedFrameInfo::new);
        debug!("add_ref: {:x}, ({:x})", page, info.get());
    }

    pub fn remove_ref(&mut self, page: PhysPageNum) {
        debug!("remove_ref: {:x}", page);
        let info = self.map.get_mut(&page).expect("remove_ref: page not found");
        debug!("remove_ref: {:x}, ({:x})", page, info.get() - 1);

        if info.decrease() {
            debug!("remove_ref: {:x}, removed from manager", page);
            self.map.remove(&page);
        }
    }

    pub fn is_shared(&self, page: PhysPageNum) -> bool {
        self.map.contains_key(&page)
    }

    pub fn is_unique(&self, page: PhysPageNum) -> bool {
        !self.is_shared(page)
    }
}

struct SharedFrameInfo {
    counter: AtomicUsize,
}

impl SharedFrameInfo {
    fn new() -> Self {
        Self {
            counter: AtomicUsize::new(2),
        }
    }

    fn get(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }

    fn increase(&self) {
        debug_assert!(self.is_shared());
        self.counter.fetch_add(1, Ordering::SeqCst);
    }

    /// 返回减少后是否为 is_unique
    fn decrease(&self) -> bool {
        debug_assert!(self.is_shared());
        self.counter.fetch_sub(1, Ordering::SeqCst) == 2
    }

    fn is_unique(&self) -> bool {
        self.counter.load(Ordering::SeqCst) == 1
    }

    fn is_shared(&self) -> bool {
        self.counter.load(Ordering::SeqCst) > 1
    }
}
