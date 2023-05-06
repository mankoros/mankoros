use crate::memory::address::{VirtAddr, VirtPageNum};

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct VirtAddrRange {
    begin: VirtAddr,
    end: VirtAddr,
}

pub struct VARangeVPNIter {
    range: VirtAddrRange,
    curr: VirtPageNum,
}

impl Iterator for VARangeVPNIter {
    type Item = VirtPageNum;
    fn next(&mut self) -> Option<Self::Item> {
        if self.curr < self.range.end().into() {
            let ret = self.curr;
            self.curr = self.curr + 1;
            Some(ret)
        } else {
            None
        }
    }
}

impl VirtAddrRange {
    /// Grow the range towards higher address
    pub fn grow_high(&mut self, size: usize) {
        self.end += size;
    }
    /// Grow the range towards lower address
    pub fn grow_low(&mut self, size: usize) {
        self.begin -= size;
    }
    /// Shrink the range from the higher end
    pub fn shrink_high(&mut self, size: usize) {
        self.end -= size;
    }
    /// Shrink the range from the lower end
    pub fn shrink_low(&mut self, size: usize) {
        self.begin += size;
    }

    /// Left Inclusive, Right Exclusive range
    pub fn new_lire(begin: VirtAddr, end: VirtAddr) -> Self {
        debug_assert!(begin <= end);
        Self { begin, end }
    }

    pub fn new_beg_size(begin: VirtAddr, size: usize) -> Self {
        Self {
            begin,
            end: begin + size,
        }
    }

    pub fn begin(&self) -> VirtAddr {
        self.begin
    }

    pub fn end(&self) -> VirtAddr {
        self.end
    }

    pub fn size(&self) -> usize {
        self.end.0 - self.begin.0
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        self.begin <= addr && addr < self.end
    }

    pub fn empty(&self) -> bool {
        self.begin == self.end
    }

    pub fn vpn_iter(&self) -> VARangeVPNIter {
        VARangeVPNIter {
            range: self.clone(),
            curr: self.begin.into(),
        }
    }

    pub fn from_begin(&self, vpn: VirtPageNum) -> usize {
        let vaddr: VirtAddr = vpn.into();
        vaddr - self.begin
    }
}