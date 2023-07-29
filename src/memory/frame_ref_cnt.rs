use crate::{
    consts::{
        device::{MAX_PHYSICAL_MEMORY, PHYMEM_START},
        PAGE_SIZE,
    },
    memory::{address::PhysPageNum, frame::dealloc_frame},
};
use alloc::alloc::alloc;
use core::{alloc::Layout, intrinsics};

static mut FRAME_REF_CNT_PTR: *mut u32 = 0 as _;

/// need to be called after device tree parse and kernel memory management
pub fn init_frame_ref_cnt() {
    let physis_memory_size = unsafe { MAX_PHYSICAL_MEMORY - PHYMEM_START };
    let frame_ref_cnt_size = physis_memory_size / PAGE_SIZE;

    let frame_ref_cnt_memory = unsafe {
        let layout = Layout::array::<u32>(frame_ref_cnt_size).unwrap();
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("frame_ref_cnt_memory alloc failed");
        }
        ptr.write_bytes(0, layout.size());
        ptr as *mut u32
    };

    unsafe {
        FRAME_REF_CNT_PTR = frame_ref_cnt_memory;
    }
}

pub fn is_frame_ref_cnt_inited() -> bool {
    unsafe { FRAME_REF_CNT_PTR != 0 as _ }
}

impl PhysPageNum {
    fn get_ref_cnt_ptr(self) -> *mut u32 {
        cfg_if::cfg_if! {
            if #[cfg(debug_assertions)] {
                let max_ppn = unsafe { MAX_PHYSICAL_MEMORY / PAGE_SIZE };
                if self.bits() >= max_ppn {
                    panic!("get_ref_cnt_ptr: ppn out of range");
                }
            }
        }
        unsafe { FRAME_REF_CNT_PTR.add(self.bits()) }
    }

    pub fn get_ref_cnt(self) -> u32 {
        unsafe { intrinsics::atomic_load_seqcst(self.get_ref_cnt_ptr()) }
    }
    fn set_ref_cnt(self, value: u32) {
        unsafe {
            let ptr = self.get_ref_cnt_ptr();
            intrinsics::atomic_store_seqcst(ptr, value);
        }
    }
    /// return previous value
    #[inline(always)]
    fn add_ref_cnt(self, offset: i32) -> u32 {
        if offset > 0 {
            unsafe {
                let ptr = self.get_ref_cnt_ptr();
                intrinsics::atomic_xadd_seqcst(ptr, offset as u32)
            }
        } else if offset < 0 {
            unsafe {
                let ptr = self.get_ref_cnt_ptr();
                intrinsics::atomic_xsub_seqcst(ptr, (-offset) as u32)
            }
        } else {
            self.get_ref_cnt()
        }
    }

    pub fn is_free(self) -> bool {
        self.get_ref_cnt() == 0
    }
    pub fn is_unique(self) -> bool {
        self.get_ref_cnt() == 1
    }
    pub fn is_shared(self) -> bool {
        self.get_ref_cnt() > 1
    }

    pub fn increase(self) {
        self.add_ref_cnt(1);
    }
    pub fn decrease(self) {
        debug_assert!(self.get_ref_cnt() != 0);
        self.add_ref_cnt(-1);
    }
    pub fn decrease_and_try_dealloc(self) {
        // if previous value is 1, then we can dealloc this frame
        if self.add_ref_cnt(-1) == 1 {
            // Fill the page with zeroes when in debug mode
            cfg_if::cfg_if! {
                if #[cfg(debug_assertions)] {
                    unsafe { self.addr().as_mut_page_slice().fill(0) };
                    log::debug!("dealloc_frame {:?} by ref count == 0", self.addr())
                }
            }
            dealloc_frame(self.addr());
        }
    }
    pub fn decrease_and_must_dealloc(self) {
        let prev = self.add_ref_cnt(-1);
        if prev != 1 {
            panic!(
                "decrease_and_must_dealloc: ref_cnt != 1, ref_cnt = {}",
                prev
            );
        }
        dealloc_frame(self.addr());
    }
}
