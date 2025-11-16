use crate::lru_ll::{List, NodeElem};

use std::collections::HashMap;

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

pub trait Cache {
    fn access_addr(&mut self, addr: usize, num_bytes: usize) -> Access;

    fn access(&mut self, block: usize) -> Access;
    fn hits(&self) -> usize;
    fn accs(&self) -> usize;

    fn run_to_end(&mut self, blocks: impl Iterator<Item = usize>) {
        for block in blocks {
            self.access(block);
        }
    }

    fn misses(&self) -> usize {
        self.accs() - self.hits()
    }

    fn hit_rate(&self) -> f64 {
        (self.hits() as f64) / (self.accs() as f64)
    }

    fn miss_rate(&self) -> f64 {
        (self.misses() as f64) / (self.accs() as f64)
    }
}

type LruNode = NodeElem<usize>;

struct LruCacheEntry {
    lru_node: LruNode,
}

pub struct LruCache {
    cache: HashMap<usize, LruCacheEntry>,
    lru: List<usize>,
    max_blocks: usize,
    block_size: usize,
    accs: usize,
    hits: usize,
}

impl LruCache {
    pub fn new(max_blocks: usize, block_size: usize) -> LruCache {
        LruCache {
            cache: HashMap::new(),
            lru: List::new(),
            max_blocks,
            block_size,
            accs: 0,
            hits: 0,
        }
    }
}

impl Cache for LruCache {
    fn access_addr(&mut self, addr: usize, num_bytes: usize) -> Access {
        let block = (addr / self.block_size) * self.block_size;
        let mut access = Access::Hit;
        let num_blocks = num_bytes.div_ceil(self.block_size);
        for off in 0..num_blocks {
            let info = self.access(block + off);
            access = access & info;
        }
        access
    }

    fn access(&mut self, block_num: usize) -> Access {
        self.accs += 1;
        if let Some(entry) = self.cache.get(&block_num) {
            self.hits += 1;
            self.lru.move_to_front(entry.lru_node.clone());
            Access::Hit
        } else if self.lru.len() == self.max_blocks {
            let victim_node = self.lru.pop_back_node().unwrap();
            let victim_block = *victim_node.borrow().value();
            let entry = self.cache.remove(&victim_block).unwrap();
            assert_eq!(entry.lru_node.as_ptr(), victim_node.as_ptr());
            *victim_node.borrow_mut().value_mut() = block_num;
            self.cache.insert(block_num, entry);
            self.lru.push_front_node(victim_node);
            Access::Miss
        } else {
            self.lru.push_front(block_num);
            let lru_node = self.lru.peek_front_node().unwrap();
            self.cache.insert(block_num, LruCacheEntry { lru_node });
            Access::Miss
        }
    }

    fn hits(&self) -> usize {
        self.hits
    }

    fn accs(&self) -> usize {
        self.accs
    }
}
