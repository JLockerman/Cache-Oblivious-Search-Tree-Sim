use std::cell::Cell;

use intrusive_collections::{LinkedList, LinkedListLink, UnsafeRef, intrusive_adapter};

use hashbrown::HashMap;

pub trait Cache {
    fn pin_addr(&mut self, addr: usize, num_bytes: usize) -> Access {
        self.access_or_pin_addr(addr, num_bytes, true)
    }

    fn unpin_addr(&mut self, addr: usize, num_bytes: usize);

    fn unpin(&mut self, block: usize);

    fn access_addr(&mut self, addr: usize, num_bytes: usize) -> Access {
        self.access_or_pin_addr(addr, num_bytes, false)
    }

    fn access(&mut self, block: usize) -> Access {
        self.access_or_pin(block, false)
    }

    fn access_or_pin_addr(&mut self, addr: usize, num_bytes: usize, pin: bool) -> Access;

    fn access_or_pin(&mut self, block: usize, pin: bool) -> Access;

    fn hits(&self) -> u64;
    fn accs(&self) -> u64;

    fn run_to_end(&mut self, blocks: impl Iterator<Item = usize>) {
        for block in blocks {
            self.access(block);
        }
    }

    fn misses(&self) -> u64 {
        self.accs() - self.hits()
    }

    fn hit_rate(&self) -> f64 {
        (self.hits() as f64) / (self.accs() as f64)
    }

    fn miss_rate(&self) -> f64 {
        (self.misses() as f64) / (self.accs() as f64)
    }
}

pub enum Access {
    Miss,
    Hit,
}

impl std::ops::BitAnd for Access {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        use Access::*;
        match (self, rhs) {
            (Hit, Hit) => Hit,
            (Hit, Miss) => Miss,
            (Miss, Hit) => Miss,
            (Miss, Miss) => Miss,
        }
    }
}

struct LruNode {
    link: LinkedListLink,
    block_id: Cell<usize>,
    pinned: Cell<usize>,
}

intrusive_adapter!(LruCacheEntry = UnsafeRef<LruNode>: LruNode { link: LinkedListLink });

pub struct LruCache {
    cache: HashMap<usize, *const LruNode>,
    lru: LinkedList<LruCacheEntry>,
    pinned: LinkedList<LruCacheEntry>,
    lru_nodes: Box<[LruNode]>,
    num_pinned: usize,
    block_size: usize,
    accs: u64,
    hits: u64,
}

impl LruCache {
    pub fn new(max_blocks: usize, block_size: usize) -> LruCache {
        let lru_nodes = (0..max_blocks)
            .map(|_| LruNode {
                link: LinkedListLink::new(),
                block_id: Cell::new(0),
                pinned: Cell::new(0),
            })
            .collect::<Box<[_]>>();
        let mut cache = LruCache {
            cache: HashMap::with_capacity(max_blocks),
            lru: LinkedList::new(LruCacheEntry::new()),
            pinned: LinkedList::new(LruCacheEntry::new()),
            lru_nodes,
            num_pinned: 0,
            block_size,
            accs: 0,
            hits: 0,
        };
        for node in &cache.lru_nodes {
            cache.lru.push_back(unsafe { UnsafeRef::from_raw(node) });
        }
        cache
    }

    pub fn blocksize(&self) -> usize {
        self.block_size
    }

    pub fn num_blocks(&self) -> usize {
        self.lru_nodes.len()
    }

    pub fn reset(&mut self) {
        self.accs = 0;
        self.hits = 0;
        self.num_pinned = 0;
        for node in self.pinned.take() {
            self.lru.push_back(node);
        }
        self.cache.clear();
    }
}

impl Cache for LruCache {
    fn access_or_pin_addr(&mut self, addr: usize, num_bytes: usize, pin: bool) -> Access {
        let mut access = Access::Hit;
        for block in span_to_blocks(self.block_size, addr, num_bytes) {
            let info = self.access_or_pin(block, pin);
            access = access & info;
        }
        access
    }

    fn access_or_pin(&mut self, block_num: usize, pin: bool) -> Access {
        self.accs += 1;
        if let Some(entry) = self.cache.get(&block_num) {
            self.hits += 1;
            if unsafe { (**entry).pinned.get() == 0 } {
                let val = unsafe { self.lru.cursor_mut_from_ptr(*entry) }
                    .remove()
                    .unwrap();
                self.lru.push_front(val);
            } else {
                unsafe { (**entry).pinned.update(|c| c + 1) };
            }

            return Access::Hit;
        }

        let victim_node = self.lru.pop_back().unwrap_or_else(|| {
            panic!(
                "no unpinned victim nodes available ({} pinned)",
                self.num_pinned
            )
        });
        let victim_block = victim_node.block_id.get();
        self.cache.remove(&victim_block);
        victim_node.block_id.set(block_num);
        debug_assert!(victim_node.pinned.get() == 0);
        victim_node.pinned.set(pin as _);
        self.cache
            .insert(block_num, UnsafeRef::into_raw(victim_node.clone()));
        if pin {
            self.pinned.push_front(victim_node);
            self.num_pinned += 1;
        } else {
            self.lru.push_front(victim_node);
        }
        Access::Miss

        //  else if self.len == self.max_blocks {
        //     let victim_node = self.lru.pop_back().unwrap_or_else(|| {
        //         panic!(
        //             "no unpinned victim nodes available ({} pinned)",
        //             self.num_pinned
        //         )
        //     });
        //     let victim_block = victim_node.block_id.get();
        //     let entry = self.cache.remove(&victim_block).unwrap();
        //     debug_assert_eq!(entry, victim_node.as_ref() as *const _);
        //     victim_node.block_id.set(block_num);
        //     debug_assert!(victim_node.pinned.get() == 0);
        //     victim_node.pinned.set(1);
        //     self.cache.insert(block_num, entry);
        //     if pin {
        //         self.pinned.push_front(victim_node);
        //         self.num_pinned += 1;
        //     } else {
        //         self.lru.push_front(victim_node);
        //     }
        //     Access::Miss
        // } else {
        //     self.len += 1;
        //     let node = UnsafeRef::from_box(Box::new(LruNode {
        //         link: LinkedListLink::new(),
        //         block_id: block_num.into(),
        //         pinned: Cell::new(pin as _),
        //     }));
        //     #[cfg(debug_assertions)]
        //     let p = node.as_ref() as *const _;
        //     let lru_node = if pin {
        //         self.num_pinned += 1;
        //         self.pinned.push_front(node);
        //         self.pinned.front().clone_pointer().unwrap()
        //     } else {
        //         self.lru.push_front(node);
        //         self.lru.front().clone_pointer().unwrap()
        //     };
        //     #[cfg(debug_assertions)]
        //     debug_assert_eq!(p, lru_node.as_ref() as *const _);
        //     self.cache.insert(block_num, UnsafeRef::into_raw(lru_node));

        //     Access::Miss
        // }
    }

    fn unpin_addr(&mut self, addr: usize, num_bytes: usize) {
        let block = (addr / self.block_size) * self.block_size;
        let num_blocks = num_bytes.div_ceil(self.block_size);
        for off in 0..num_blocks {
            self.unpin(block + off);
        }
    }

    fn unpin(&mut self, block: usize) {
        let Some(entry) = self.cache.get(&block) else {
            return;
        };
        if unsafe { (**entry).pinned.get() == 0 } {
            return;
        }
        unsafe {
            (**entry).pinned.update(|c| c - 1);
        }
        if unsafe { (**entry).pinned.get() > 0 } {
            return;
        }
        let Some(val) = unsafe { self.pinned.cursor_mut_from_ptr(*entry) }.remove() else {
            return;
        };
        debug_assert!(val.pinned.get() == 0);
        self.lru.push_front(val);
        self.num_pinned -= 1;
    }

    fn hits(&self) -> u64 {
        self.hits
    }

    fn accs(&self) -> u64 {
        self.accs
    }
}

pub fn span_to_blocks(
    block_size: usize,
    addr: usize,
    num_bytes: usize,
) -> impl Iterator<Item = usize> {
    assert!(num_bytes > 0);
    assert!(block_size > 0);
    let start = (addr / block_size) * block_size;
    let end = addr + num_bytes;
    (start..end).step_by(block_size)
}

#[cfg(test)]
mod test {
    use crate::lru_cache::span_to_blocks;

    #[test]
    fn test_addrs() {
        let blocks = |block_size, addr, num_bytes| -> Vec<_> {
            span_to_blocks(block_size, addr, num_bytes).collect()
        };

        assert_eq!(blocks(4096, 1, 1), [0]);
        assert_eq!(blocks(4096, 1, 4097), [0, 4096]);
        assert_eq!(blocks(4096, 4096, 4097), [4096, 4096 * 2]);
        assert_eq!(blocks(4096, 2048, 1), [0]);
        assert_eq!(blocks(4096, 2048, 4096 * 2 + 2), [0, 4096, 4096 * 2]);
    }
}
