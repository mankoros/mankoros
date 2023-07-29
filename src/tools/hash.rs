use super::arena::{MAtomicPtr, ObjPool, Ptr};
use crate::sync::SpinNoIrqLock;

use core::sync::atomic::Ordering;

pub trait HashKey {
    fn hash(&self) -> usize;
}

pub trait HashValue {
    type Key: HashKey;
    fn hash(&self) -> usize;
    fn equal(&self, other: &Self::Key) -> bool;
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
    #[inline(always)]
    fn key_to_bucket(&self, hash: usize) -> &MAtomicPtr<ListNode<V>> {
        &self.buckets[hash % BC]
    }

    pub fn new() -> Self {
        Self {
            nodes: SpinNoIrqLock::new(ObjPool::new()),
            buckets: array_init::array_init(|_| MAtomicPtr::new(Ptr::null())),
        }
    }

    pub fn get<'a>(&'a self, key: &V::Key) -> Option<&'a mut V> {
        let bucket = self.key_to_bucket(key.hash());

        let mut np = bucket.load(Ordering::SeqCst);
        while !np.is_null() {
            let node = np.as_mut();
            if node.value.equal(key) {
                return Some(&mut node.value);
            }
            np = node.next;
        }
        None
    }

    pub fn put(&self, value: V) {
        let bucket = self.key_to_bucket(value.hash());
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
