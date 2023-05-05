use core::ops::Range;

use alloc::collections::BTreeMap;

struct Node<U, V> {
    pub end: U,
    pub value: V,
}

/// [start, end) -> value
///
/// 保证区间不重合 否则panic
///
/// 禁止区间长度为0
pub struct RangeMap<U: Ord + Copy, V>(BTreeMap<U, Node<U, V>>);

impl<U: Ord + Copy, V> RangeMap<U, V> {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn try_insert(&mut self, Range { start, end }: Range<U>, value: V) -> Result<&mut V, V> {
        debug_assert!(start < end);
        if let Some((_xstart, Node { end: xend, .. })) = self.0.range(..end).next_back() {
            if *xend > start {
                return Err(value);
            }
        }
        let node = self.0.try_insert(start, Node { end, value }).ok().unwrap();
        Ok(&mut node.value)
    }
    /// 找到 start <= key < end 的区间
    pub fn get(&self, key: U) -> Option<(Range<U>, &V)> {
        let (&start, Node { end, value }) = self.0.range(..=key).next_back()?;
        if *end > key {
            return Some((start..*end, value));
        }
        None
    }
    /// 找到 start <= key < end 的区间
    pub fn get_mut(&mut self, key: U) -> Option<(Range<U>, &mut V)> {
        let (&start, Node { end, value }) = self.0.range_mut(..=key).next_back()?;
        if *end > key {
            return Some((start..*end, value));
        }
        None
    }
    /// 在 [start, end) 中找到一个长至少为 size 的空闲区间
    /// 
    /// 这个空闲区间将会以某个地址 a 为起点, 以 offset(a, n) 为终点,
    /// offset 函数用于提供一个将 U 和 usize 相加并返回 U 的方法
    pub fn find_free_range(
        &self,
        Range { mut start, end }: Range<U>,
        size: usize,
        mut offset: impl FnMut(U, usize) -> U,
    ) -> Option<Range<U>> {
        if offset(start, size) > end {
            return None;
        }
        if let Some((_, node)) = self.0.range(..start).next_back() {
            debug_assert!(offset(node.end, size) <= end);
            start = start.max(node.end);
        }
        for (&n_start, node) in self.0.range(start..end) {
            let xend = offset(node.end, size);
            if xend > end {
                return None;
            }
            if xend <= n_start {
                break;
            }
            start = node.end;
        }
        debug_assert!(offset(start, size) <= end);
        Some(start..offset(start, size))
    }
    /// Check whether range is free
    ///
    /// if range is free, return Ok(())
    ///
    /// if start >= end, return Err(())
    pub fn range_is_free(&self, Range { start, end }: Range<U>) -> Result<(), ()> {
        if start >= end {
            return Err(());
        }
        if let Some((_, node)) = self.0.range(..start).next_back() {
            if node.end > start {
                return Err(());
            }
        }
        if self.0.range(start..end).next().is_some() {
            return Err(());
        }
        Ok(())
    }
    /// range 处于返回值对应的 range 内
    pub fn range_contain(&self, range: Range<U>) -> Option<&V> {
        let (_, Node { end, value }) = self.0.range(..=range.start).next_back()?;
        if *end >= range.end {
            return Some(value);
        }
        None
    }
    /// range 处于返回值对应的 range 内
    pub fn range_contain_mut(&mut self, range: Range<U>) -> Option<&mut V> {
        let (_, Node { end, value }) = self.0.range_mut(..=range.start).next_back()?;
        if *end >= range.end {
            return Some(value);
        }
        None
    }
    /// range 完全匹配返回值所在范围
    pub fn range_match(&self, range: Range<U>) -> Option<&V> {
        let (start, Node { end, value }) = self.0.range(..=range.start).next_back()?;
        if *start == range.start && *end == range.end {
            return Some(value);
        }
        None
    }
    pub fn force_remove_one(&mut self, Range { start, end }: Range<U>) -> V {
        let Node { end: n_end, value } = self.0.remove(&start).unwrap();
        assert!(n_end == end);
        value
    }
    /// split_l: take the left side of the range
    ///
    /// split_r: take the right side of the range
    pub fn remove(
        &mut self,
        Range { start, end }: Range<U>,
        mut split_l: impl FnMut(&mut V, U, Range<U>) -> V,
        mut split_r: impl FnMut(&mut V, U, Range<U>) -> V,
        mut release: impl FnMut(V, Range<U>),
    ) {
        if start >= end {
            return;
        }
        //  aaaaaaa  aaaaa
        //    bbb       bbbb
        //  aa---aa  aaa--
        //  ^^       ^^^
        // The left side will stay
        if let Some((&n_start, node)) = self.0.range_mut(..start).next_back() {
            let n_end = node.end;
            if start < n_end {
                node.end = start;
                let mut v_m = split_r(&mut node.value, start, n_start..n_end);
                if end < n_end {
                    let v_r = split_r(&mut v_m, end, start..n_end);
                    release(v_m, start..end);
                    let value = Node {
                        end: n_end,
                        value: v_r,
                    };
                    self.0.try_insert(end, value).ok().unwrap();
                } else {
                    release(v_m, start..n_end);
                }
            }
        }
        //    aaaaaa
        //  bbbbb
        //    ---aaa
        //       ^^^
        // The right side will stay
        if let Some((&n_start, node)) = self.0.range_mut(..end).next_back() {
            if end < node.end {
                let cut = split_l(&mut node.value, end, n_start..node.end);
                release(cut, n_start..end);
                let node = self.0.remove(&n_start).unwrap();
                self.0.try_insert(end, node).ok().unwrap();
            }
        }
        //   aa aa aaa
        //  bbbbbbbbbbb
        //   -- -- ---
        while let Some((&n_start, _node)) = self.0.range(start..end).next() {
            let Node { end, value } = self.0.remove(&n_start).unwrap();
            release(value, n_start..end);
        }
    }
    pub fn replace(
        &mut self,
        r @ Range { start, end }: Range<U>,
        value: V,
        split_l: impl FnMut(&mut V, U, Range<U>) -> V,
        split_r: impl FnMut(&mut V, U, Range<U>) -> V,
        release: impl FnMut(V, Range<U>),
    ) {
        self.remove(r, split_l, split_r, release);
        self.0.try_insert(start, Node { end, value }).ok().unwrap();
    }
    /// 位置必须位于某个段中间, 否则panic
    pub fn split_at(&mut self, p: U, split_r: impl FnOnce(&mut V, U, Range<U>) -> V) {
        let (&start, Node { end, value }) = self.0.range_mut(..p).next_back().unwrap();
        let xend = *end;
        debug_assert!(p < xend);
        *end = p;
        let node = Node {
            end: xend,
            value: split_r(value, p, start..xend),
        };
        self.0.try_insert(p, node).ok().unwrap();
    }
    /// 将一个段切成两半
    pub fn split_at_maybe(&mut self, p: U, split_r: impl FnOnce(&mut V, U, Range<U>) -> V) {
        let (&start, Node { end, value }) = match self.0.range_mut(..p).next_back() {
            Some(v) => v,
            None => return,
        };
        let xend = *end;
        if xend <= p {
            return;
        }
        *end = p;
        let node = Node {
            end: xend,
            value: split_r(value, p, start..xend),
        };
        self.0.try_insert(p, node).ok().unwrap();
    }
    /// 按顺序调用三个函数
    ///
    /// 位置必须位于某个段中间, 否则panic
    pub fn split_at_run(
        &mut self,
        p: U,
        split_r: impl FnOnce(&mut V, U, Range<U>) -> V,
        l_run: impl FnOnce(&mut V, Range<U>),
        r_run: impl FnOnce(&mut V, Range<U>),
    ) {
        let (&start, Node { end, value }) = self.0.range_mut(..p).next_back().unwrap();
        let xend = *end;
        debug_assert!(p < xend);
        *end = p;
        let mut xvalue = split_r(value, p, start..xend);
        l_run(value, start..p);
        r_run(&mut xvalue, p..xend);
        let node = Node {
            end: xend,
            value: xvalue,
        };
        self.0.try_insert(p, node).ok().unwrap();
    }
    pub fn clear(&mut self, mut release: impl FnMut(V, Range<U>)) {
        core::mem::take(&mut self.0)
            .into_iter()
            .for_each(|(n_start, node)| release(node.value, n_start..node.end));
    }
    /// 如果条件对变量返回true, 这个段将从容器中被移除
    pub fn clear_if(
        &mut self,
        mut condition: impl FnMut(&V, Range<U>) -> bool,
        mut release: impl FnMut(V, Range<U>),
    ) {
        self.0
            .drain_filter(|&n_start, v| condition(&v.value, n_start..v.end))
            .for_each(|(n_start, node)| release(node.value, n_start..node.end))
    }
    /// f return (A, B)
    ///
    /// if A is Some will set current into A, else do nothing.
    ///
    /// B will insert to new range_map.
    pub fn fork(&mut self, mut f: impl FnMut(&V) -> V) -> Self {
        // use crate::tools::container::{never_clone_linked_list::NeverCloneLinkedList, Stack};
        let mut map = RangeMap::new();
        for (&a, Node { end: b, value: v }) in self.0.iter() {
            let node = Node {
                end: *b,
                value: f(v),
            };
            map.0.try_insert(a, node).ok().unwrap();
        }
        map
    }
    /// 必须存在 range 对应的 node
    pub fn merge(&mut self, Range { start, end }: Range<U>, mut f: impl FnMut(&V, &V) -> bool) {
        let cur = self.0.get(&start).unwrap();
        assert!(cur.end == end);
        let cur = if let Some(nxt) = self.0.get(&end) {
            if f(&cur.value, &nxt.value) {
                let nxt_end = nxt.end;
                self.0.remove(&end).unwrap();
                let cur = self.0.get_mut(&start).unwrap();
                cur.end = nxt_end;
                unsafe { &*(&*cur as *const _) }
            } else {
                cur
            }
        } else {
            cur
        };

        if let Some((&s, n)) = self.0.range(..start).next_back() {
            if n.end == start && f(&n.value, &cur.value) {
                self.0.get_mut(&s).unwrap().end = cur.end;
                self.0.remove(&start).unwrap();
            }
        }
    }

    /// 向后 (虚拟地址增大方向) 扩展一个从 start 开始的段, 这个段必须存在. 扩展成功返回 Ok, 否则 Err
    pub fn extend_back(&mut self, start: U, new_end: U) -> Result<(), ()> {
        self.range_is_free(start..new_end)?;
        let node = self.0.get_mut(&start).unwrap();
        node.end = new_end;
        Ok(())
    }

    /// 向后 (虚拟地址增大方向) 减少一个从 start 开始的段, 这个段必须存在. 
    /// 
    /// 减少成功 (长度为 0 时删除该段) 返回 Ok, 越界或超出长度返回 Err
    pub fn reduce_back(&mut self, start: U, new_end: U) -> Result<(), ()> {
        let node = self.0.get_mut(&start).unwrap();
        if start <= new_end && new_end < node.end {
            if start == new_end {
                self.0.remove(&start).unwrap();
            } else {
                node.end = new_end;
            }
            Ok(())
        } else {
            Err(())
        }
    }


    pub fn iter(&self) -> impl Iterator<Item = (Range<U>, &V)> {
        self.0.iter().map(|(&s, n)| {
            let r = s..n.end;
            (r, &n.value)
        })
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Range<U>, &mut V)> {
        self.0.iter_mut().map(|(&s, n)| {
            let r = s..n.end;
            (r, &mut n.value)
        })
    }
    /// return start in r
    pub fn range(&self, r: Range<U>) -> impl Iterator<Item = (Range<U>, &V)> {
        self.0.range(r).map(|(&s, n)| {
            let r = s..n.end;
            (r, &n.value)
        })
    }
    /// return start in r
    pub fn range_mut(&mut self, r: Range<U>) -> impl Iterator<Item = (Range<U>, &mut V)> {
        self.0.range_mut(r).map(|(&s, n)| {
            let r = s..n.end;
            (r, &mut n.value)
        })
    }
}
