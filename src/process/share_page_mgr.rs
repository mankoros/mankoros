use alloc::{collections::BTreeMap, boxed::Box, format};
use crate::memory::address::PhysPageNum;
use core::{sync::atomic::{AtomicUsize, Ordering}, ptr::NonNull};

pub struct SharedPageManager {
    map: BTreeMap<PhysPageNum, SharedCounterPtr>
}

impl Drop for SharedPageManager {
    fn drop(&mut self) {
        panic!("Should not auto drop SharePageManager")
    }
}

impl SharedPageManager {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new()
        }
    }

    /// 获得一个对该页的引用计数器的指针 (引用计数++)
    pub fn clone(&mut self, ppn: PhysPageNum) -> SharedCounterPtr {
        self.map.get(&ppn)
            .expect(format!("SharePageManager: clone: ppn {:?} not exists", ppn).as_str())
            .clone()
    }

    /// 将新的页纳入管理, 会为它创建新的引用计数的指针, 其中一个放在自己这里, 另一个返回出去
    /// 一般返回的指针会直接给 insert_by 用
    pub fn insert_clone(&mut self, ppn: PhysPageNum) -> SharedCounterPtr {
        let (a, b) = SharedCounterPtr::new_dup();
        self.insert_by(ppn, a);
        b
    }

    /// 将对应的页的引用计数器的指针放入管理
    pub fn insert_by(&mut self, ppn: PhysPageNum, ptr: SharedCounterPtr) {
        self.map.try_insert(ppn, ptr)
            .expect(format!("SharePageManager: insert_by: ppn {:?} already exists", ppn).as_str());
    }

    pub fn remove(&mut self, ppn: PhysPageNum) {
        self.map.remove(&ppn)
            .expect(format!("SharePageManager: remove: ppn {:?} not exists", ppn).as_str())
            .consume();
    }

    pub fn is_shared(&self, ppn: PhysPageNum) -> bool {
        self.map.contains_key(&ppn)
    }
}

#[derive(Debug)]
pub struct SharedCounterPtr(NonNull<AtomicUsize>);

unsafe impl Send for SharedCounterPtr {}
unsafe impl Sync for SharedCounterPtr {}
impl Drop for SharedCounterPtr {
    fn drop(&mut self) {
        panic!("Should not auto drop SharedCounterPtr")
    }
}

impl SharedCounterPtr {
    fn alloc(init: usize) -> NonNull<AtomicUsize> {
        let ptr = Box::into_raw(Box::new(AtomicUsize::new(init)));
        unsafe { NonNull::new_unchecked(ptr) }
    }

    pub fn new() -> Self {
        Self(Self::alloc(1))
    }

    pub fn new_dup() -> (Self, Self) {
        let ptr = Self::alloc(2);
        (Self(ptr), Self(ptr))
    }

    /// 如果引用计数为 1, 则释放引用, 返回 true
    /// 否则, 引用计数减一, 返回 false
    pub fn consume(self) -> bool {
        let counter = unsafe { self.0.as_ref() };
        let n = counter.fetch_sub(1, Ordering::SeqCst);
        debug_assert_ne!(n, 0);

        let should_release = n == 1;
        if should_release {
            // 如果需要释放引用, 我们就再次构造一次 Box, 使其自然 Drop 掉, 然后释放
            unsafe { Box::from_raw(self.0.as_ptr()) };
        }
        core::mem::forget(self);

        should_release
    }

    pub fn increase(&self) -> usize {
        let counter = unsafe { self.0.as_ref() };
        counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn is_unique(&self) -> bool {
        let counter = unsafe { self.0.as_ref() };
        counter.load(Ordering::SeqCst) == 1
    }
}

impl Clone for SharedCounterPtr {
    fn clone(&self) -> Self {
        self.increase();
        Self(self.0)
    }
}

