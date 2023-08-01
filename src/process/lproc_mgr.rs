use super::{lproc::LightProcess, pid::Pid};
use crate::sync::SpinNoIrqLock;
use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec::Vec,
};

pub struct GlobalLProcManager {
    map: SpinNoIrqLock<BTreeMap<Pid, Weak<LightProcess>>>,
}

static MGR: GlobalLProcManager = GlobalLProcManager::new();

impl GlobalLProcManager {
    pub const fn new() -> Self {
        Self {
            map: SpinNoIrqLock::new(BTreeMap::new()),
        }
    }

    pub fn get(pid: Pid) -> Option<Arc<LightProcess>> {
        let mut map = MGR.map.lock(here!());
        let result = map.get(&pid)?.upgrade();
        if result.is_none() {
            map.remove(&pid);
        }
        result
    }

    pub fn put(lproc: &Arc<LightProcess>) {
        MGR.map.lock(here!()).insert(lproc.id(), Arc::downgrade(lproc));
    }

    pub fn all() -> Vec<(Pid, Arc<LightProcess>)> {
        let map = MGR.map.lock(here!());
        let mut result = Vec::new();
        for (pid, weak) in map.iter() {
            if let Some(lproc) = weak.upgrade() {
                result.push((*pid, lproc));
            }
        }
        result
    }
}
