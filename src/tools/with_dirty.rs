use core::{cell::SyncUnsafeCell, sync::atomic::AtomicBool};

pub struct WithDirty<T> {
    inner: SyncUnsafeCell<T>,
    dirty: AtomicBool,
}

impl<T> WithDirty<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: SyncUnsafeCell::new(inner),
            dirty: AtomicBool::new(false),
        }
    }

    pub fn as_ref(&self) -> &T {
        unsafe { &*self.inner.get() }
    }
    pub fn as_mut(&self) -> &mut T {
        self.dirty.store(true, core::sync::atomic::Ordering::Relaxed);
        unsafe { &mut *self.inner.get() }
    }

    pub fn dirty(&self) -> bool {
        self.dirty.load(core::sync::atomic::Ordering::Relaxed)
    }
    pub fn clear(&self) {
        self.dirty.store(false, core::sync::atomic::Ordering::Relaxed);
    }
}

impl<T> Drop for WithDirty<T> {
    fn drop(&mut self) {
        if self.dirty() {
            log::warn!("Dropping a dirty WithDirty, may cause data loss");
        }
    }
}
