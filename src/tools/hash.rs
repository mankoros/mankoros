use alloc::{boxed::Box, vec::Vec};
use crate::sync::SpinNoIrqLock;
use core::{pin::Pin, sync::atomic::Ordering, mem::MaybeUninit};
use super::arena::{ObjPool, Ptr, MAtomicPtr};


pub trait HashKey {
    fn hash(&self) -> usize;
    fn equal(&self, other: &Self) -> bool;
}

pub trait HashValue {
    type Key: HashKey;
    fn key(&self) -> &Self::Key;
}

struct ListNode<V: HashValue> {
    value: V,
    next: Ptr<ListNode<V>>,
}

impl<V: HashValue> ListNode<V> {
    fn new(value: V) -> Self {
        Self {
            value,
            next: Ptr::null(),
        }
    }
}

pub struct HashTable<const BUCKET_CNT: usize, V: HashValue> {
    nodes: SpinNoIrqLock<ObjPool<ListNode<V>>>,
    buckets: [MAtomicPtr<ListNode<V>>; BUCKET_CNT],
}

impl<const BC: usize, V: HashValue> HashTable<BC, V> {
    fn key_to_bucket(&self, key: &V::Key) -> &MAtomicPtr<ListNode<V>> {
        let bid = key.hash() % BC;
        &self.buckets[bid]
    }

    pub fn new() -> Self {
        Self {
            nodes: SpinNoIrqLock::new(ObjPool::new()),
            buckets: array_init::array_init(|_| MAtomicPtr::new(Ptr::null())),
        }
    }

    pub fn get<'a>(&'a self, key: &V::Key) -> Option<&'a mut V> {
        let bucket = self.key_to_bucket(key);

        let mut np = bucket.load(Ordering::SeqCst);
        while !np.is_null() {
            let node = np.as_mut();
            if node.value.key().equal(key) {
                return Some(&mut node.value);
            }
            np = node.next;
        }
        None
    }

    pub fn put(&self, value: V) {
        let bucket = self.key_to_bucket(value.key());
        let node = self.nodes.lock(here!()).put(ListNode::new(value));
        node.as_mut().next = bucket.swap(node, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        for bucket in self.buckets.iter() {
            bucket.store(Ptr::null(), Ordering::SeqCst);
        }
        self.nodes.lock(here!()).free();
    }
}

