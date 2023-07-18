use alloc::{boxed::Box, vec::Vec};
use core::{cell::SyncUnsafeCell, fmt::Debug, pin::Pin, sync::atomic::AtomicUsize};

pub struct Ptr<T>(*mut T);

impl<T> Ptr<T> {
    pub fn new(ptr: *mut T) -> Self {
        Self(ptr)
    }
    pub fn null() -> Self {
        Self(core::ptr::null_mut())
    }
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
    pub fn as_ref<'a>(&self) -> &'a T {
        unsafe { &*self.0 }
    }
    pub fn as_mut<'a>(&self) -> &'a mut T {
        unsafe { &mut *self.0 }
    }
}

impl Ptr<u8> {
    pub fn as_slice(&self, len: usize) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.0, len) }
    }
    pub fn as_mut_slice(&mut self, len: usize) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.0, len) }
    }
}

unsafe impl<T> Send for Ptr<T> {}
unsafe impl<T> Sync for Ptr<T> {}

impl<T> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}
impl<T> Copy for Ptr<T> {}

impl<T> Debug for Ptr<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Ptr").field(&self.0).finish()
    }
}

impl<T> PartialEq for Ptr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for Ptr<T> {}
impl<T> PartialOrd for Ptr<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl<T> Ord for Ptr<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

pub struct MAtomicPtr<T> {
    ptr: AtomicUsize,
    _marker: core::marker::PhantomData<T>,
}

impl<T> MAtomicPtr<T> {
    pub fn new(ptr: Ptr<T>) -> Self {
        Self {
            ptr: AtomicUsize::new(ptr.0 as usize),
            _marker: core::marker::PhantomData,
        }
    }
    pub fn load(&self, order: core::sync::atomic::Ordering) -> Ptr<T> {
        Ptr(self.ptr.load(order) as *mut T)
    }
    pub fn store(&self, ptr: Ptr<T>, order: core::sync::atomic::Ordering) {
        self.ptr.store(ptr.0 as usize, order);
    }
    pub fn swap(&self, ptr: Ptr<T>, order: core::sync::atomic::Ordering) -> Ptr<T> {
        Ptr(self.ptr.swap(ptr.0 as usize, order) as *mut T)
    }
}

impl<T> Clone for MAtomicPtr<T> {
    fn clone(&self) -> Self {
        Self {
            ptr: AtomicUsize::new(self.ptr.load(core::sync::atomic::Ordering::Relaxed)),
            _marker: core::marker::PhantomData,
        }
    }
}

pub struct ObjPool<T> {
    objs: SyncUnsafeCell<Vec<Pin<Box<T>>>>,
}

impl<T> ObjPool<T> {
    pub const fn new() -> Self {
        Self {
            objs: SyncUnsafeCell::new(Vec::new()),
        }
    }
    pub fn put(&self, obj: T) -> Ptr<T> {
        let mut obj = Box::pin(obj);
        let ptr = Ptr::new(unsafe { obj.as_mut().get_unchecked_mut() as *mut _ });
        unsafe {
            (*self.objs.get()).push(obj);
        };
        ptr
    }
    pub fn free(&self) {
        unsafe {
            (*self.objs.get()).clear();
        }
    }
}

unsafe impl<T> Send for ObjPool<T> {}
unsafe impl<T> Sync for ObjPool<T> {}
