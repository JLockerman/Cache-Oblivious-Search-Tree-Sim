use clap::{Parser, ValueEnum};
use sim::simalloc::{self, Simalloc};
use std::{cell::RefCell, str::FromStr};

use rand::{SeedableRng, prelude::*};
use rand_pcg::Pcg64Mcg;

mod btree;
#[cfg(test)]
mod parse_tests;
mod trees_of_small_height;
mod veb;

fn main() {
    use TestSpec::*;

    let Args {
        caches,
        test,
        num_values,
        order,
    } = Args::parse();

    let rng = Pcg64Mcg::from_rng(&mut rand::rng());
    let values = || -> Box<dyn Iterator<Item = u64>> {
        match order {
            TestOrder::Asc => Box::new((0..num_values).map(|v| v as u64)),
            TestOrder::Rng => {
                Box::new(rng.clone().random_iter().take(num_values))
            }
        }
    };

    let cache_spec = caches.specs.iter().map(|s| simalloc::CacheSpec {
        num_blocks: s.num_lines,
        block_size: s.line_size,
    });

    let print_stats = |sim: &RefCell<Simalloc>| {
        for (i, stat) in sim.borrow().stats().enumerate() {
            println!("L{i}:");
            println!("   {:>9} accesses", stat.accesses);
            println!("   {:>9} hits ", stat.hits);
            println!("   {:>9} misses ", stat.accesses - stat.hits);
            println!(
                "  {:>9.2}% hit rate ",
                ((stat.hits as f64 / stat.accesses as f64) * 100.0)
            );
            println!(
                "   {:>9.3} accesses / write",
                stat.accesses as f64 / num_values as f64
            );
            println!(
                "   {:>9.3} hits / write ",
                stat.hits as f64 / num_values as f64
            );
            println!(
                "   {:>9.3} misses / write ",
                (stat.accesses - stat.hits) as f64 / num_values as f64
            );
        }
    };

    if let VEB | All = test {
        // println!(
        //     "max height = {}",
        //     num_values.next_power_of_two().trailing_zeros()
        // );
        let sim = Simalloc::new(cache_spec.clone());
        let vals = values();
        let mut tree = trees_of_small_height::Tree::<u64>::in_sim(sim.clone());
        for val in vals {
            tree.insert(val);
        }

        print_stats(&sim);
    }

    if let BTree4k | All = test {
        let sim = Simalloc::new(cache_spec.clone());
        let vals = values();
        let mut tree = btree::BTree::<u64>::with_node_size_in_sim(1 << 12, sim.clone());
        for val in vals {
            tree.insert(val);
        }
        print_stats(&sim);
    }

    if let Interleaved | All = test {
        todo!()
    }
}

#[derive(Parser, Debug)]
pub struct Args {
    /// Cache specifications in format "size:lines,size:lines,..."
    /// Size can include units like K, KiB, M, MiB, etc.
    pub caches: CacheSpecs,

    /// Test type to run
    #[arg(value_enum)]
    pub test: TestSpec,

    pub num_values: usize,

    #[arg(value_enum)]
    pub order: TestOrder,
}

#[derive(Debug, Clone)]
pub struct CacheSpecs {
    pub specs: Vec<CacheSpec>,
}

#[derive(Debug, Clone)]
pub struct CacheSpec {
    pub line_size: usize,
    pub num_lines: usize,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestSpec {
    VEB,
    BTree4k,
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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
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

// Helper method to access the specs vector directly
impl Args {
    pub fn cache_specs(&self) -> &Vec<CacheSpec> {
        &self.caches.specs
    }
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
