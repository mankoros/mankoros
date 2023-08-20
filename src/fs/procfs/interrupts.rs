use alloc::collections::BTreeMap;

pub static mut PROC_FS_IRQ_CNT: BTreeMap<usize, usize> = BTreeMap::new();
