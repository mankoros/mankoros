use alloc::vec::Vec;

#[derive(Clone)]
pub struct UsizePool {
    next: usize,
    recycled: Vec<usize>,
}

impl UsizePool {
    pub const fn new(start: usize) -> Self {
        UsizePool {
            next: start,
            recycled: Vec::new(),
        }
    }

    pub fn get(&mut self) -> usize {
        if let Some(pid) = self.recycled.pop() {
            pid
        } else {
            let pid = self.next;
            self.next += 1;
            pid
        }
    }

    pub fn release(&mut self, pid: usize) {
        debug_assert!(!self.recycled.contains(&pid));
        self.recycled.push(pid);
    }
}
