use crate::{
    consts::PAGE_SIZE,
    memory::{address::PhysAddr4K, frame::alloc_frame},
    process::pid::Pid,
    sync::SpinNoIrqLock,
    tools::errors::{SysError, SysResult},
};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

static GLOBAL_SHM_MGR: ShmManager = ShmManager::new();
pub fn global_shm_mgr() -> &'static ShmManager {
    &GLOBAL_SHM_MGR
}

pub type ShmKey = usize;
pub type ShmId = usize;

pub struct ShmManager {
    shms: SpinNoIrqLock<BTreeMap<ShmKey, Arc<Shm>>>,
}

impl ShmManager {
    pub const fn new() -> Self {
        Self {
            shms: SpinNoIrqLock::new(BTreeMap::new()),
        }
    }

    pub fn get(&self, key: ShmKey) -> Option<Arc<Shm>> {
        self.shms.lock(here!()).get(&key).map(Arc::clone)
    }
    pub fn remove(&self, key: ShmKey) -> Option<Arc<Shm>> {
        self.shms.lock(here!()).remove(&key)
    }
    pub fn create(&self, key: Option<ShmKey>, size: usize, creator: Pid) -> SysResult<Arc<Shm>> {
        let shm = Shm::alloc(size, creator)?;
        if let Some(key) = key {
            self.shms.lock(here!()).insert(key, shm.clone());
        }
        Ok(shm)
    }
}

pub struct Shm {
    attach_cnt: AtomicUsize,
    last_operater: SpinNoIrqLock<Pid>,
    size: usize,
    creater: Pid,
    frames: Vec<PhysAddr4K>,
}

impl Shm {
    pub fn alloc(size: usize, creater: Pid) -> SysResult<Arc<Self>> {
        debug_assert!(size % PAGE_SIZE == 0);
        let mut frames = Vec::new();
        for _ in 0..(size / PAGE_SIZE) {
            frames.push(alloc_frame().ok_or(SysError::ENOMEM)?);
        }
        Ok(Arc::new(Self {
            attach_cnt: AtomicUsize::new(0),
            last_operater: SpinNoIrqLock::new(creater),
            size,
            creater,
            frames,
        }))
    }

    pub fn size(&self) -> usize {
        self.size
    }
    pub fn creater(&self) -> Pid {
        self.creater
    }
    pub fn last_operater(&self) -> Pid {
        *(self.last_operater.lock(here!()))
    }
    pub fn attach_cnt(self: &Arc<Self>) -> usize {
        self.attach_cnt.load(Ordering::Relaxed)
    }

    pub fn attach(&self, pid: Pid) -> impl Iterator<Item = &PhysAddr4K> {
        self.attach_cnt.fetch_add(1, Ordering::Relaxed);
        *(self.last_operater.lock(here!())) = pid;
        self.frames.iter()
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        for frame in self.frames.iter() {
            frame.page_num().decrease_and_must_dealloc();
        }
    }
}
