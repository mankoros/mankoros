use core::ops::Deref;

struct SyncMutPtr<T>(*mut T);

impl<T> Deref for SyncMutPtr<T> {
    type Target = *mut T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> SyncMutPtr<T> {
    pub fn new_ptr(ptr: *mut T) -> Self {
        Self(ptr)
    }
    pub fn new_usize(ptr: usize) -> Self {
        Self(ptr as *mut T)
    }

    pub fn get(&self) -> *mut T {
        self.0
    }
    pub fn add(&self, byte_offset: usize) -> Self {
        Self(unsafe { self.0.add(byte_offset) })
    }
}

unsafe impl<T> Sync for SyncMutPtr<T> {}
