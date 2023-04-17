// Copyright (c) 2023 EastonMan <me@eastonman.com>
// Copyright (c) 2020 rCore

//! Mutex
//! Adapted from https://github.com/rcore-os/rCore/blob/master/kernel/src/sync/mutex.rs

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use riscv::register::sstatus;

/// A mutual exclusion primitive useful for protecting shared data
///
///
#[derive(Debug)]
pub struct Mutex<T: ?Sized, S: MutexSupport> {
    locked: AtomicBool,
    support: MaybeUninit<S>,
    support_init: AtomicU8, // For core-safe init, 0 = uninitialized, 1 = initializing, 2 = initialized
    hart_id: UnsafeCell<usize>, // Current hart id holding the mutex
    data: UnsafeCell<T>,
}

// Same unsafe impls as `std::sync::Mutex`
// TODO: EastonMan: not sure what this is use for, just copied it from rCore
unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for Mutex<T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for Mutex<T, S> {}

impl<T, S: MutexSupport> Mutex<T, S> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            support: MaybeUninit::uninit(),
            support_init: AtomicU8::new(0), // Uninitialized
            hart_id: UnsafeCell::new(0),    // No one is holding the mutex
            data: UnsafeCell::new(data),
        }
    }

    /// Consumes the mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // Compiler ensures that `self` is only used here
        let Mutex { data, .. } = self;
        data.into_inner()
    }
}

impl<T: ?Sized, S: MutexSupport> Mutex<T, S> {
    fn obtain_lock(&self, place: &str) {
        // Swap true in if old is false
        // on success load in-order, store relaxed
        // on failure relaxed
        while self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            != Ok(false)
        {
            let mut try_count = 0;
            // Wait until the lock looks unlocked before retrying
            while self.locked.load(Ordering::Relaxed) {
                // Release the CPU
                unsafe { &*self.support.as_ptr() }.cpu_relax();
                try_count += 1;
                if try_count == 0x100000 {
                    // Dead lock detected!
                    panic!("dead lock detected! place: {}", place);
                }
            }
        }
        // TODO: implement a riscv hart_id read
        let hart_id = 0;
        unsafe { self.hart_id.get().write(hart_id) };
    }

    /// Locks the spinlock and returns a guard.
    ///
    /// The returned value may be dereferenced for data access
    /// and the lock will be dropped when the guard falls out of scope.
    ///
    /// ```
    /// let mylock = Mutex::new(0);
    /// {
    ///     let mut data = mylock.lock();
    ///     // The lock is now locked and the data can be accessed
    ///     *data += 1;
    ///     // The lock is implicitly dropped
    /// }
    ///
    /// ```
    pub fn lock(&self, place: &str) -> MutexGuard<T, S> {
        let support_guard = S::before_lock();

        // Ensure support is initialized
        self.ensure_support();

        self.obtain_lock(place);
        MutexGuard {
            mutex: self,
            support_guard,
        }
    }

    pub fn ensure_support(&self) {
        let initialization = self.support_init.load(Ordering::Relaxed);
        if initialization == 2 {
            return;
        };
        if initialization == 1
            || self.support_init.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                != Ok(0)
        {
            // Wait for another thread to initialize
            while self.support_init.load(Ordering::Acquire) == 1 {
                core::hint::spin_loop();
            }
        } else {
            // My turn to initialize
            (unsafe { core::ptr::write(self.support.as_ptr() as *mut _, S::new()) });
            self.support_init.store(2, Ordering::Release);
        }
    }
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
pub struct MutexGuard<'a, T: ?Sized + 'a, S: MutexSupport + 'a> {
    pub(super) mutex: &'a Mutex<T, S>,
    support_guard: S::GuardData,
}
impl<'a, T: ?Sized, S: MutexSupport> Deref for MutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for MutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for MutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
        unsafe { &*self.mutex.support.as_ptr() }.after_unlock();
    }
}

/// Low-level support for mutex
pub trait MutexSupport {
    type GuardData;
    fn new() -> Self;
    /// Called when failing to acquire the lock
    fn cpu_relax(&self);
    /// Called before lock() & try_lock()
    fn before_lock() -> Self::GuardData;
    /// Called when MutexGuard dropping
    fn after_unlock(&self);
}

/// Spin & no-interrupt lock
#[derive(Debug)]
pub struct SpinNoIrq;

#[inline]
pub unsafe fn enable_sie() {
    sstatus::set_sie();
}

pub unsafe fn restore_sie(sie_before: bool) {
    if sie_before {
        enable_sie()
    }
}

pub unsafe fn disable_and_store_sie() -> bool {
    let e = sstatus::read().sie();
    sstatus::clear_sie();
    e
}

/// Contains sie before disable interrupt, will auto restore it when dropping
pub struct FlagsGuard(bool);

impl Drop for FlagsGuard {
    fn drop(&mut self) {
        unsafe { restore_sie(self.0) };
    }
}

impl FlagsGuard {
    pub fn no_irq_region() -> Self {
        Self(unsafe { disable_and_store_sie() })
    }
}

impl MutexSupport for SpinNoIrq {
    type GuardData = FlagsGuard;
    fn new() -> Self {
        SpinNoIrq
    }
    fn cpu_relax(&self) {
        // No relax, continue spinning
        core::hint::spin_loop();
    }
    fn before_lock() -> Self::GuardData {
        FlagsGuard(unsafe { disable_and_store_sie() })
    }
    fn after_unlock(&self) {}
}
