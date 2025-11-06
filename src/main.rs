use clap::{Parser, Subcommand, ValueEnum};
use sim::{
    hierarchy::Hierarchy,
    simalloc::{self, Simalloc},
};
use std::str::FromStr;

use rand::{SeedableRng, prelude::*};
use rand_pcg::Pcg64Mcg;

mod btree;
#[cfg(test)]
mod parse_tests;
mod trees_of_small_height;
mod veb;
// mod hierarchical;

fn main() {
    let Args { test } = Args::parse();

    match test {
        Test::Get {
            caches,
            datastructure,
            num_values,
            interleaving,
        } => {
            let cache_spec = caches.specs.iter().map(|s| simalloc::CacheSpec {
                num_blocks: s.num_lines,
                block_size: s.line_size,
                inclusive: s.inclusive,
            });
            get_test(&datastructure, num_values, interleaving, cache_spec)
        }
        Test::Write {
            caches,
            datastructure,
            num_values,
            order,
            interleaving,
        } => {
            let rng = Pcg64Mcg::from_os_rng();
            match order {
                TestOrder::Asc => println!("ASC"),
                TestOrder::Rng => println!("RNG"),
            }
            let values = || -> Box<dyn Iterator<Item = u64>> {
                match order {
                    TestOrder::Asc => Box::new((0..num_values).map(|v| v as u64)),
                    TestOrder::Rng => Box::new(rng.clone().random_iter().take(num_values)),
                }
            };
            let cache_spec = caches.specs.iter().map(|s| simalloc::CacheSpec {
                num_blocks: s.num_lines,
                block_size: s.line_size,
                inclusive: s.inclusive,
            });
            insert_test(
                &datastructure,
                num_values,
                interleaving,
                values,
                cache_spec,
                false,
            )
        }
        Test::WR {
            caches,
            datastructure,
            num_values,
            order,
            interleaving,
        } => {
            let rng = Pcg64Mcg::from_os_rng();
            match order {
                TestOrder::Asc => println!("ASC"),
                TestOrder::Rng => println!("RNG"),
            }
            let values = || -> Box<dyn Iterator<Item = u64>> {
                match order {
                    TestOrder::Asc => Box::new((0..num_values).map(|v| v as u64)),
                    TestOrder::Rng => Box::new(rng.clone().random_iter().take(num_values)),
                }
            };
            let cache_spec = caches.specs.iter().map(|s| simalloc::CacheSpec {
                num_blocks: s.num_lines,
                block_size: s.line_size,
                inclusive: s.inclusive,
            });
            insert_test(
                &datastructure,
                num_values,
                interleaving,
                values,
                cache_spec,
                true,
            )
        }
    }
}

fn get_test(
    datastructure: &[TreeSpec],
    num_values: usize,
    interleaving: Option<usize>,
    cache_spec: impl Iterator<Item = simalloc::CacheSpec> + Clone,
) {
    use TreeSpec::*;

    // ≈1 trillion element dataset size
    let set_size = 1usize << 40;
    let average_found_search_length = set_size.ilog2();

    let element_rng = Pcg64Mcg::from_os_rng();
    let tree_rng = Pcg64Mcg::from_os_rng();

    let num_trees = interleaving.unwrap_or(1);
    if datastructure.contains(&VEB) || datastructure.contains(&All) {
        println!("VEB");
        let mut hier = Hierarchy::new(cache_spec.clone());
        let mut element_rng = element_rng.clone();
        let mut tree_rng = tree_rng.clone();
        let tree_size = if set_size.is_power_of_two() {
            (set_size + 1).next_power_of_two()
        } else {
            set_size.next_power_of_two()
        } - 1;
        let tree_bytes = set_size * size_of::<Option<u64>>();
        let roots: Vec<_> = (0..num_trees)
            .map(|_| {
                tree_rng.random_range(0..1 << (64 - 1 - tree_bytes.ilog2())) << tree_bytes.ilog2()
            })
            .collect();
        let mut hits = 0;
        for _ in 0..num_values {
            for &base_addr in &roots {
                let num_elements_until_match =
                    element_rng.random_range(0..2 * average_found_search_length) as usize;
                let hit = trees_of_small_height::sim_get::<u64>(
                    base_addr,
                    num_elements_until_match,
                    tree_size,
                    &mut hier,
                    &mut tree_rng,
                );
                hits += hit.is_some() as usize;
            }
        }
        print_stats(num_values * num_trees, Some(hits), hier.stats());
    }

    if datastructure.contains(&BTree4k) || datastructure.contains(&All) {
        println!("B-Tree");
        let mut hier = Hierarchy::new(cache_spec.clone());
        let mut element_rng = element_rng.clone();
        let mut tree_rng = tree_rng.clone();
        let node_size = 1 << 12;
        let node_cap = btree::num_elements_for_node_size::<u64>(node_size);
        println!("cap: {node_cap}");
        let roots: Vec<_> = (0..num_trees)
            .map(|_| {
                tree_rng.random_range(0..1 << (64 - 1 - node_size.ilog2())) << node_size.ilog2()
            })
            .collect();

        let height = ((set_size + 1) / 2).ilog(node_cap / 2).try_into().unwrap();
        let mut hits = 0;
        for _ in 0..num_values {
            for &base_addr in &roots {
                let num_elements_until_match =
                    element_rng.random_range(0..2 * average_found_search_length) as usize;
                let hit = btree::sim_get::<u64>(
                    base_addr,
                    0,
                    height,
                    num_elements_until_match,
                    node_cap,
                    node_size,
                    &mut hier,
                    &mut tree_rng,
                );
                hits += hit.is_some() as usize;
            }
        }
        print_stats(num_values * num_trees, Some(hits), hier.stats());
    }

    // if let Interleaved | All = datastructure {
    //     todo!()
    // }
}

fn insert_test(
    datastructure: &[TreeSpec],
    num_values: usize,
    interleaving: Option<usize>,
    values: impl Fn() -> Box<dyn Iterator<Item = u64>>,
    cache_spec: impl Iterator<Item = simalloc::CacheSpec> + Clone,
    also_read: bool,
) {
    use TreeSpec::*;
    let num_trees = interleaving.unwrap_or(1);

    if datastructure.contains(&VEB) || datastructure.contains(&All) {
        println!("VEB");
        // println!(
        //     "max height = {}",
        //     num_values.next_power_of_two().trailing_zeros()
        // );
        let sim = Simalloc::new(cache_spec.clone());
        let vals = values();

        let mut trees: Vec<_> = (0..num_trees)
            .map(|_| trees_of_small_height::Tree::<u64>::in_sim(sim.clone()))
            .collect();
        for val in vals {
            for tree in &mut trees {
                tree.insert(val);
            }
        }

        println!("{}B", trees[0].num_bytes());
        print_stats(num_values * num_trees, None, sim.borrow().stats());

        if also_read {
            println!("VEB READ");
            sim.borrow_mut().reset();

            let vals = values();
            for val in vals {
                for tree in &trees {
                    assert_eq!(tree.get(&val), Some(&val));
                }
            }

            print_stats(num_values * num_trees, None, sim.borrow().stats());
        }
    }

    if datastructure.contains(&BTree4k) || datastructure.contains(&All) {
        print!("B-Tree ");
        let sim = Simalloc::new(cache_spec.clone());
        let vals = values();
        let mut trees: Vec<_> = (0..num_trees)
            .map(|_| btree::BTree::<u64>::with_node_size_in_sim(1 << 12, sim.clone()))
            .collect();

        for val in vals {
            for tree in &mut trees {
                tree.insert(val);
            }
        }
        print_stats(num_values * num_trees, None, sim.borrow().stats());

        if also_read {
            println!("B-Tree READ");
            sim.borrow_mut().reset();

            let vals = values();
            for val in vals {
                for tree in &trees {
                    assert_eq!(tree.get(&val), Some(&val), "{}", tree.output_dot());
                }
            }

            print_stats(num_values * num_trees, None, sim.borrow().stats());
        }
    }

    if datastructure.contains(&BTree2) || datastructure.contains(&All) {
        let sz = btree::node_size_for_num_elements::<u64>(2);
        println!("B-Tree {sz}");
        let sim = Simalloc::new(cache_spec.clone());
        let vals = values();
        let mut trees: Vec<_> = (0..num_trees)
            .map(|_| btree::BTree::<u64>::with_node_size_in_sim(sz, sim.clone()))
            .collect();

        for val in vals {
            for tree in &mut trees {
                tree.insert(val);
            }
        }
        print_stats(num_values * num_trees, None, sim.borrow().stats());

        if also_read {
            println!("B-Tree READ");
            sim.borrow_mut().reset();

            let vals = values();
            for val in vals {
                for tree in &trees {
                    assert_eq!(tree.get(&val), Some(&val), "{}", tree.output_dot());
                }
            }

            print_stats(num_values * num_trees, None, sim.borrow().stats());
        }
    }

    // if let Interleaved | All = datastructure {
    //     todo!()
    // }
}

fn print_stats(
    num_values: usize,
    found: Option<usize>,
    stats: impl Iterator<Item = simalloc::CacheStats>,
) {
    if let Some(found) = found {
        println!(
            "   {:>9.2}% found ",
            (found as f64 / num_values as f64) * 100.0
        );
    }
    for (i, stat) in stats.enumerate() {
        println!(
            "L{i}: {}B x {}{}",
            stat.blocksize,
            stat.num_blocks,
            if stat.inclusive { "" } else { " *" }
        );
        println!("   {:>9} accesses", stat.accesses);
        println!("   {:>9} hits ", stat.hits);
        println!("   {:>9} misses ", stat.accesses - stat.hits);
        println!(
            "  {:>9.2}% hit rate ",
            ((stat.hits as f64 / stat.accesses as f64) * 100.0)
        );
        println!(
            "   {:>9.3} accesses / op",
            stat.accesses as f64 / num_values as f64
        );
        println!(
            "   {:>9.3} hits / op ",
            stat.hits as f64 / num_values as f64
        );
        println!(
            "   {:>9.3} misses / op ",
            (stat.accesses - stat.hits) as f64 / num_values as f64
        );
    }
}

#[derive(Parser, Debug)]
pub struct Args {
    #[command(subcommand)]
    test: Test,
}

#[derive(Subcommand, Debug)]
pub enum Test {
    /// Test gets to the simulated trees with simulated caches.
    Get {
        /// Cache specifications in format "size:lines,size:lines,..."
        /// Size can include units like K, KiB, M, MiB, etc.
        caches: CacheSpecs,

        num_values: usize,

        interleaving: Option<usize>,

        /// Data structure to test
        #[arg(value_enum)]
        datastructure: Vec<TreeSpec>,
    },

    /// Test writes to the actual trees with simulated caches.
    Write {
        /// Cache specifications in format "size:lines,size:lines,..."
        /// Size can include units like K, KiB, M, MiB, etc.
        caches: CacheSpecs,

        num_values: usize,

        #[arg(value_enum)]
        order: TestOrder,

        #[arg(short = 'i')]
        interleaving: Option<usize>,

        /// Data structure to test
        #[arg(value_enum)]
        datastructure: Vec<TreeSpec>,
    },

    WR {
        /// Cache specifications in format "size:lines,size:lines,..."
        /// Size can include units like K, KiB, M, MiB, etc.
        caches: CacheSpecs,

        num_values: usize,

        #[arg(value_enum)]
        order: TestOrder,

        #[arg(short = 'i')]
        interleaving: Option<usize>,

        /// Data structure to test
        #[arg(value_enum)]
        datastructure: Vec<TreeSpec>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestSpec {
    Get,
    Write,
}

#[derive(Debug, Clone)]
pub struct CacheSpecs {
    pub specs: Vec<CacheSpec>,
}

#[derive(Debug, Clone)]
pub struct CacheSpec {
    pub line_size: usize,
    pub num_lines: usize,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum TreeSpec {
    VEB,
    BTree4k,
    BTree2,
    Interleaved,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestOrder {
    Asc,
    Rng,
}

impl FromStr for CacheSpecs {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let specs = s
            .split(',')
            .map(|spec| spec.trim().parse::<CacheSpec>())
            .collect::<Result<Vec<_>, _>>()?;

        if specs.is_empty() {
            return Err("At least one cache specification is required".to_string());
        }

        Ok(CacheSpecs { specs })
    }
}

impl FromStr for CacheSpec {
    type Err = String;

    fn from_str(mut s: &str) -> Result<Self, Self::Err> {
        let mut inclusive = true;
        if let Some(t) = s.strip_suffix("n") {
            s = t;
            inclusive = false;
        }
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err(format!(
                "Invalid cache spec format: '{}'. Expected 'size:lines'",
                s
            ));
        }

        let line_size = parse_size(parts[0].trim())?;
        let num_lines = parts[1]
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("Invalid number of lines: '{}'", parts[1]))?;

        Ok(CacheSpec {
            line_size,
            num_lines,
            inclusive,
        })
    }
}

// impl FromStr for TestSpec {
//     type Err = String;

//     fn from_str(s: &str) -> Result<Self, Self::Err> {
//         match s.to_lowercase().as_str() {
//             "veb" => Ok(TestSpec::VEB),
//             "btree" => Ok(TestSpec::BTree),
//             "interleaved" => Ok(TestSpec::Interleaved),
//             "all" => Ok(TestSpec::All),
//             _ => Err(format!(
//                 "Invalid test spec: '{}'. Expected 'veb', 'btree', or 'both'",
//                 s
//             )),
//         }
//     }
// }

fn parse_size(s: &str) -> Result<usize, String> {
    let s = s.trim();
    let bytes = s.as_bytes();

    // Find the unit suffix and corresponding multiplier using slice patterns
    let (number_bytes, multiplier) = match bytes {
        [num_bytes @ .., b'K' | b'k', b'i', b'B'] => (num_bytes, 1 << 10),
        [num_bytes @ .., b'M' | b'm', b'i', b'B'] => (num_bytes, 1 << 20),
        [num_bytes @ .., b'G' | b'g', b'i', b'B'] => (num_bytes, 1 << 30),
        [num_bytes @ .., b'K' | b'k', b'B'] => (num_bytes, 1000),
        [num_bytes @ .., b'M' | b'm', b'B'] => (num_bytes, 1_000_000),
        [num_bytes @ .., b'G' | b'g', b'B'] => (num_bytes, 1_000_000_000),
        [num_bytes @ .., b'B'] => (num_bytes, 1),
        [num_bytes @ .., b'K' | b'k'] => (num_bytes, 1 << 10),
        [num_bytes @ .., b'M' | b'm'] => (num_bytes, 1 << 20),
        [num_bytes @ .., b'G' | b'g'] => (num_bytes, 1 << 30),
        num_bytes => (num_bytes, 1), // No suffix
    };

    let number_part = std::str::from_utf8(number_bytes)
        .map_err(|_| "Invalid UTF-8 in number part".to_string())?;

    let num = number_part
        .parse::<usize>()
        .map_err(|_| format!("Invalid number: '{}'", number_part))?;

    Ok(num * multiplier)
}

// trait Values: Iterator<Item = u64> {
//     fn duplicate(&self) -> Box<dyn Values>;
// }
// impl<T> Values for T
// where
//     T: 'static + Clone + Iterator<Item = u64>,
// {
//     fn duplicate(&self) -> Box<dyn Values> {
//         Box::new(self.clone())
//     }
// }

// trait Tree<T> {
//     fn do_insert(&mut self, val: T);
//     fn do_get(&self, val: &T) -> Option<usize>;
// }

// impl<T> Tree<T> for trees_of_small_height::Tree<T>
// where
//     T: Ord + std::fmt::Debug,
// {
//     fn do_insert(&mut self, val: T) {
//         self.insert(val);
//     }

//     fn do_get(&self, val: &T) -> Option<usize> {
//         self.get(val).map(|r| (r as *const T).addr())
//     }
// }

// impl<T> Tree<T> for btree::BTree<T>
// where
//     T: Ord,
// {
//     fn do_insert(&mut self, val: T) {
//         self.insert(val);
//     }

//     fn do_get(&self, val: &T) -> Option<usize> {
//         self.get(val).map(|r| (r as *const T).addr())
//     }
// }
