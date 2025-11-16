use crate::lru_cache::{Access, Cache, LruCache};

pub struct Hierarchy {
    level: Vec<LruCache>,
    inclusive: Vec<bool>,
}

pub struct CacheSpec {
    pub num_blocks: usize,
    pub block_size: usize,
    pub inclusive: bool,
}

pub struct CacheStats {
    pub num_blocks: usize,
    pub blocksize: usize,
    pub accesses: u64,
    pub hits: u64,
    pub inclusive: bool,
}

impl Hierarchy {
    pub fn new(sizes: impl IntoIterator<Item = CacheSpec>) -> Self {
        let (level, inclusive) = sizes
            .into_iter()
            .map(|spec| {
                (
                    LruCache::new(spec.num_blocks, spec.block_size),
                    spec.inclusive,
                )
            })
            .unzip();
        Self { level, inclusive }
    }

    pub fn stats(&self) -> impl Iterator<Item = CacheStats> {
        self.level.iter().enumerate().map(|(i, l)| CacheStats {
            num_blocks: l.num_blocks(),
            blocksize: l.blocksize(),
            accesses: l.accs(),
            hits: l.hits(),
            inclusive: self.inclusive[i],
        })
    }

    pub fn reset(&mut self) {
        for level in &mut self.level {
            level.reset()
        }
    }
}

impl Cache for Hierarchy {
    fn access_or_pin(&mut self, block: usize, pin: bool) -> Access {
        use Access::{Hit, Miss};
        let mut res = Miss;
        for i in 0..self.level.len() {
            let access = self.level[i].access_or_pin(block, pin);
            if let Hit = access {
                if !self.inclusive[i]
                    || (i + 1 < self.level.len()
                        && self.level[i].blocksize() > self.level[i + 1].blocksize())
                {
                    return Hit;
                }
                res = Hit
            }
        }
        res
    }

    fn access_or_pin_addr(&mut self, addr: usize, num_bytes: usize, mut pin: bool) -> Access {
        use Access::{Hit, Miss};
        let mut res = Miss;
        for i in 0..self.level.len() {
            let access = self.level[i].access_or_pin_addr(addr, num_bytes, pin);
            if let Hit = access {
                if !self.inclusive[i] {
                    return Hit;
                }
                res = Hit;
            }
            if !self.inclusive[i] {
                pin = false
            }
        }
        res
    }

    fn unpin(&mut self, block: usize) {
        for i in 0..self.level.len() {
            self.level[i].unpin(block);
        }
    }

    fn unpin_addr(&mut self, addr: usize, num_bytes: usize) {
        for i in 0..self.level.len() {
            self.level[i].unpin_addr(addr, num_bytes);
        }
    }

    fn hits(&self) -> u64 {
        self.level.iter().map(|c| c.hits()).sum()
    }

    fn accs(&self) -> u64 {
        self.level[0].accs()
    }
}
