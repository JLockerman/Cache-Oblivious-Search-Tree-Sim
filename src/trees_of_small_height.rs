use std::{cell::RefCell, fmt::Debug, mem::MaybeUninit, rc::Rc};

use crate::veb;

// use rand::{RngCore, SeedableRng};
// use rand_pcg::Pcg64Mcg;
use sim::{
    lru_cache::Cache,
    simalloc::{SBox, SSlice, SSliceMut, Simalloc},
};

pub struct Tree<K> {
    tree: SBox<[Option<(usize, K)>]>,
    scratch: SBox<[MaybeUninit<K>]>,
}

pub fn sim_get<T>(
    base_addr: usize,
    // center around lg(n) for an even hit rate
    mut num_elements_until_match: usize,
    num_elements: usize,
    sim: &mut sim::hierarchy::Hierarchy,
    global_rng: &mut impl rand::Rng,
) -> Option<usize> {
    let mut ancestors = [0; 64];
    // match self.index(&mut ancestors, value) {
    //     Err(_) => return None,
    //     Ok((i, _)) => self.tree.pin_at(i).as_ref().map(|(_, v)| v),
    // }
    use std::cmp::Ordering::*;

    // let num_elements = self.tree.slice(..).len();
    // let table = self.table();
    let table = table(num_elements);

    let mut pos = 0;
    let mut bfs_pos = 1;
    let mut depth = 0;
    while pos < num_elements {
        // let Some((_, pivot)) = self.tree.pin_at(pos).as_ref() else {
        //     return Err((pos, depth, bfs_pos));
        // };
        sim.access_addr(
            base_addr + pos * size_of::<Option<T>>(),
            size_of::<Option<T>>(),
        );
        // let mut node_rng =
        //     Pcg64Mcg::seed_from_u64((base_addr + pos * size_of::<Option<T>>()) as u64);
        // 50% fill rate
        // if pos > num_elements / 2 && global_rng.next_u32() % 2 == 0 {
        //     return None
        // }

        let cmp = if num_elements_until_match == 0 {
            Equal
        } else if global_rng.next_u32() % 2 == 1 {
            Greater
        } else {
            Less
        };
        num_elements_until_match = num_elements_until_match.saturating_sub(1);
        // bfs_pos = match value.cmp(pivot) {
        bfs_pos = match cmp {
            // Equal => return Ok((pos, depth)),
            Equal => return Some(pos),
            Less => 2 * bfs_pos,
            Greater => 2 * bfs_pos + 1,
        };
        depth += 1;
        pos = match veb_index(bfs_pos, &mut ancestors, depth, table) {
            Some(p) => p,
            None => break,
        };
        ancestors[depth as usize] = pos;
    }
    // println!("{neum} {depth}");
    None
}

impl<K: Debug> Debug for Tree<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.tree.fmt(f)
    }
}

impl<K: Debug + Ord> Tree<K> {
    pub fn in_sim(sim: Rc<RefCell<Simalloc>>) -> Self {
        Self {
            tree: SBox::default_in_sim(sim.clone()),
            scratch: SBox::default_in_sim(sim),
        }
    }

    #[allow(dead_code)]
    pub fn cap(&self) -> usize {
        self.tree.slice(..).len()
    }

    #[allow(dead_code)]
    pub fn get(&self, value: &K) -> Option<&K> {
        let mut ancestors = [0; 64];
        match self.index(&mut ancestors, value) {
            Err(_) => return None,
            Ok((i, _)) => self.tree.pin_at(i).as_ref().map(|(_, v)| v),
        }
    }

    pub fn insert(&mut self, value: K) {
        if self.tree.slice(..).is_empty() {
            self.tree =
                SBox::unsize_in_sim([Some((1, value)), None, None], self.tree.sim().clone());
            self.scratch = SBox::unsize_in_sim(
                [
                    MaybeUninit::uninit(),
                    MaybeUninit::uninit(),
                    MaybeUninit::uninit(),
                ],
                self.scratch.sim().clone(),
            );
            return;
        }

        let mut ancestors = [0; 64];
        let pos = self.index(&mut ancestors, &value);
        match pos {
            Ok((i, depth)) => {
                **self.tree.pin_mut_at(i) = (1, value).into();
                for j in (0..depth).rev() {
                    self.tree.pin_mut_at(j as usize).as_mut().unwrap().0 += 1;
                }
            }
            // TODO switch to ternary enum
            Err((i, depth, _)) if i < self.tree.slice(..).len() => {
                **self.tree.pin_mut_at(i) = (1, value).into();
                for j in (0..depth).rev() {
                    let ancestor = ancestors[j as usize];
                    self.tree.pin_mut_at(ancestor).as_mut().unwrap().0 += 1;
                }
            }
            Err((_, depth, bfs_pos)) => {
                self.rebalance_and_insert(&mut ancestors, depth, value, bfs_pos)
            }
        }
    }

    fn rebalance_and_insert(
        &mut self,
        ancestors: &mut [usize; 64],
        depth: u32,
        value: K,
        bfs_at: usize,
    ) {
        let t_0 = 0.9;
        let t_h = 0.75;
        let mut current_max_density = t_0;
        let mut current_height = 0.0;
        // cap = 2^h-1
        // cap + 1 = 2^h
        // lg(cap + 1) = h
        let max_height = (self.tree.slice(..).len() + 1).ilog2() as f64;
        let mut pow2 = 2;
        let mut bfs_pos = bfs_at >> 1;
        for depth in (0..depth).rev() {
            let capacity = pow2 - 1;
            let size = self
                .tree
                .pin_at(ancestors[depth as usize])
                .as_ref()
                .unwrap()
                .0
                + 1;
            // TODO if lg(other size) < log(this size) + 1 just rotate?
            if size < (capacity as f64 * current_max_density).floor() as usize {
                // dbg!();
                // println!(
                //   "{depth}, {size}, {}",
                //   (capacity as f64 * current_max_density).floor() as usize
                // );
                let table = self.table();
                let scratch = extract(
                    table,
                    self.tree.slice_mut(..),
                    self.scratch.slice_mut(..),
                    ancestors,
                    depth,
                    bfs_pos,
                    value,
                    bfs_at,
                );
                // println!("{ancestors:?}");
                layout(
                    table,
                    scratch.slice(..),
                    depth,
                    bfs_pos,
                    ancestors,
                    self.tree.slice_mut(..),
                );
                for j in (0..depth).rev() {
                    let ancestor = ancestors[j as usize];
                    self.tree.pin_mut_at(ancestor).as_mut().unwrap().0 += 1;
                }
                return;
            }

            current_max_density = t_h + (t_0 - t_h) * (max_height - current_height) / max_height;
            current_height += 1.0;
            pow2 <<= 1;
            bfs_pos >>= 1;
        }

        let new_len = ((self.tree.slice(..).len() + 1) << 1) - 1;
        self.scratch = SBox::new_uninit_in_sim(new_len, self.scratch.sim().clone());
        let scratch = extract(
            self.table(),
            self.tree.slice_mut(..),
            self.scratch.slice_mut(..),
            ancestors,
            0,
            1,
            value,
            bfs_at,
        );
        self.tree = unsafe {
            let mut t: SBox<[MaybeUninit<Option<_>>]> =
                SBox::new_uninit_in_sim(new_len, self.tree.sim().clone());
            let mut slice = t.slice_mut(..);
            for i in 0..slice.len() {
                slice.pin_mut_at(i).write(None);
            }
            SBox::<[MaybeUninit<_>]>::assume_init(t)
        };
        layout(
            table(self.tree.slice(..).len()),
            scratch.slice(..),
            0,
            1,
            ancestors,
            self.tree.slice_mut(..),
        );
        self.scratch =
            SBox::new_uninit_in_sim(self.tree.slice(..).len(), self.scratch.sim().clone());
    }

    fn index(
        &self,
        ancestors: &mut [usize; 64],
        value: &K,
    ) -> Result<(usize, u32), (usize, u32, usize)> {
        use std::cmp::Ordering::*;

        let num_elements = self.tree.slice(..).len();
        if num_elements == 0 {
            return Err((0, 0, 0));
        }

        let table = self.table();

        let mut pos = 0;
        let mut bfs_pos = 1;
        let mut depth = 0;
        while pos < num_elements {
            let Some((_, pivot)) = self.tree.pin_at(pos).as_ref() else {
                return Err((pos, depth, bfs_pos));
            };
            bfs_pos = match value.cmp(pivot) {
                Equal => return Ok((pos, depth)),
                Less => 2 * bfs_pos,
                Greater => 2 * bfs_pos + 1,
            };
            depth += 1;
            pos = match veb_index(bfs_pos, ancestors, depth, table) {
                Some(p) => p,
                None => break,
            };
            ancestors[depth as usize] = pos;
        }

        Err((num_elements, depth, bfs_pos))
    }

    #[inline]
    fn table(&self) -> &'static [veb::TreeInfo] {
        table(self.tree.slice(..).len())
    }
}

#[inline]
fn table(num_elements: usize) -> &'static [veb::TreeInfo] {
    if num_elements == 1 {
        //TODO
        return veb::constants::LOOKUP[1];
    }
    let height = if num_elements % 2 == 0 {
        (num_elements + 1).next_power_of_two().trailing_zeros()
    } else {
        num_elements.next_power_of_two().trailing_zeros()
    };
    let table = veb::constants::LOOKUP[height as usize];
    assert_eq!(table.len(), height as usize);
    table
}

fn extract<'s, K: Debug + Ord>(
    table: &[veb::TreeInfo],
    tree: SSliceMut<'_, Option<(usize, K)>>,
    scratch: SSliceMut<'s, MaybeUninit<K>>,
    ancestors: &mut [usize; 64],
    depth: u32,
    bfs_pos: usize,
    value: K,
    bfs_at: usize,
) -> SSliceMut<'s, MaybeUninit<K>> {
    fn gather<K: Debug + Ord>(
        table: &[veb::TreeInfo],
        ancestors: &mut [usize; 64],
        mut tree: SSliceMut<'_, Option<(usize, K)>>,
        mut scratch: SSliceMut<'_, MaybeUninit<K>>,
        pre_offset: usize,
        pos: usize,
        bfs_pos: usize,
        depth: u32,
        value: &mut Option<K>,
        bfs_at: usize,
    ) -> usize {
        if depth as usize >= table.len() {
            return 0;
        }
        if tree.pin_at(pos).is_none() {
            return 0;
        }

        ancestors[depth as usize] = pos;
        let mut offset = pre_offset;
        let bfs_left = 2 * bfs_pos;
        if bfs_left == bfs_at {
            scratch.pin_mut_at(offset).write(value.take().unwrap());
            offset += 1;
        }

        if let Some(left) = veb_index(bfs_left, ancestors, depth + 1, table) {
            debug_assert!(left < tree.len());
            debug_assert!(left > pos, "{left} > {pos}");
            offset += gather(
                table,
                &mut *ancestors,
                tree.slice_mut(..),
                scratch.slice_mut(..),
                offset,
                left,
                2 * bfs_pos,
                depth + 1,
                value,
                bfs_at,
            );
        }

        let (_, pivot) = tree.pin_mut_at(pos).take().unwrap();
        scratch.pin_mut_at(offset).write(pivot);
        offset += 1;

        let bfs_right = 2 * bfs_pos + 1;
        if bfs_right == bfs_at {
            scratch.pin_mut_at(offset).write(value.take().unwrap());
            offset += 1;
        }

        if let Some(right) = veb_index(bfs_right, &mut *ancestors, depth + 1, table) {
            debug_assert!(right < tree.len());
            debug_assert!(right > pos, "{right} > {pos}");
            offset += gather(
                table,
                ancestors,
                tree,
                scratch.slice_mut(..),
                offset,
                right,
                2 * bfs_pos + 1,
                depth + 1,
                value,
                bfs_at,
            );
        }

        offset - pre_offset
    }

    debug_assert_eq!(ancestors[0], 0);

    let pos = ancestors[depth as usize];
    let num_elements = tree.pin_at(pos).as_ref().unwrap().0 + 1;
    let mut scratch = scratch.subrange(..num_elements);
    let mut val = Some(value);

    let len = gather(
        table,
        ancestors,
        tree,
        scratch.slice_mut(..),
        0,
        pos,
        bfs_pos,
        depth,
        &mut val,
        bfs_at,
    );
    assert_eq!(len, num_elements);

    return scratch;
}

fn layout<K: Debug>(
    table: &[veb::TreeInfo],
    elements: SSlice<'_, MaybeUninit<K>>,
    depth: u32,
    bfs_pos: usize,
    ancestors: &mut [usize; 64],
    mut out: SSliceMut<Option<(usize, K)>>,
) -> usize {
    if elements.is_empty() {
        return 0;
    }
    let pos = veb_index(bfs_pos, ancestors, depth, table).unwrap();
    ancestors[depth as usize] = pos;
    let root = elements.len() / 2;

    let mut size = 1;

    size += layout(
        table,
        elements.slice(..root),
        depth + 1,
        2 * bfs_pos,
        ancestors,
        out.slice_mut(..),
    );

    if root + 1 < elements.len() {
        size += layout(
            table,
            elements.slice((root + 1)..),
            depth + 1,
            2 * bfs_pos + 1,
            ancestors,
            out.slice_mut(..),
        );
    }

    let old = out
        .pin_mut_at(pos)
        .replace((size, unsafe { elements.pin_at(root).assume_init_read() }));
    assert!(old.is_none(), "{old:?}");
    return size;
}

#[inline]
fn veb_index(
    bfs_pos: usize,
    ancestors: &mut [usize; 64],
    depth: u32,
    table: &[veb::TreeInfo],
) -> Option<usize> {
    if depth as usize >= table.len() {
        return None;
    }
    // using the formula from
    // "Cache Oblivious Search Trees via Binary Trees of Small Height"
    //    Pos[d] = Pos[D[d]] + T[d] + (i and T [d]) * B[d]
    // translating to local names
    // pos = ancestors[top_tree_root_depth[d]]   (offset of the subtree)
    //     + top_tree_size[d]                    (offset of the children)
    //     + (bfs_pos & top_tree_size[d])        (child number)
    //       * bottom_tree_size[d]               (child size)
    debug_assert!(bfs_pos >= 1, "{bfs_pos} >= 1");
    let elem = &table[depth as usize];
    let pos = ancestors[elem.top_tree_root_depth as usize]
        + elem.top_tree_size as usize
        + (bfs_pos & elem.top_tree_size as usize) * elem.bottom_tree_size as usize;
    pos.into()
}

// #[cfg(test)]
// mod test {
//     use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

//     use super::Tree;

//     use proptest::property_test;

//     #[test]
//     fn count_to_3() {
//         let mut a = Tree::default();
//         println!("{:2}: {a:?}", 0);
//         for i in 1..=3 {
//             a.insert(i);
//             println!("{:2}: {a:?}", i);
//             for j in 1..=i {
//                 assert!(a.get(&j).is_some(), "{j}");
//             }
//         }
//     }

//     #[test]
//     fn count_to_10() {
//         let mut a = Tree::default();
//         println!("{:2}: {a:?}", 0);
//         for i in 0..11 {
//             a.insert(i);
//             println!("{:2}: {a:?}", i);
//             for j in 0..=i {
//                 assert!(a.get(&j).is_some(), "{j}");
//             }
//         }
//     }

//     #[test]
//     fn spiral_to_10() {
//         let mut a = Tree::default();
//         println!("{:2}: {a:?}", 0);
//         for i in [5, 4, 6, 3, 7, 2, 8, 1, 9, 0, 10] {
//             a.insert(i);
//             println!("{:2}: {a:?}", i);
//         }
//     }

//     #[test]
//     fn count_to_terminal_width() {
//         let mut a = Tree::default();
//         println!("{:2}: {a:?}", 0);
//         for i in 0..(80 / 3) {
//             a.insert(i);
//             println!("{:2}: {a:?}", i);
//             for j in 0..=i {
//                 assert!(a.get(&j).is_some(), "{j}");
//             }
//         }
//     }

//     #[test]
//     fn count_to_100() {
//         let mut a = Tree::default();
//         for i in 0..100 {
//             a.insert(i);
//             for j in 0..=i {
//                 assert!(a.get(&j).is_some(), "{j}");
//             }
//         }
//         // println!("{a:?}");
//     }

//     // #[property_test]
//     // fn quick(vals: Vec<u16>) {
//     //   let mut base = HashSet::new();
//     //   let mut test = Tree::default();
//     //   for val in vals {
//     //     base.insert(val);
//     //     test.insert(val);
//     //     for v in &base {
//     //       assert!(test.get(v).is_some());
//     //     }
//     //   }
//     // }

//     #[test]
//     fn exhaust() {
//         for i in 0..=16 {
//             let values: Vec<_> = (0..((1 << i) - 1)).collect();
//             // let mut a = Tree::with_capacity(values.len());
//             let mut a = Tree::default();
//             for &value in &values {
//                 // println!("i: {i}");
//                 // println!("{a:?}");
//                 a.insert(value);
//             }
//             // if i == 5 {
//             //   println!("aa {a:?}");
//             // }
//             // println!("({}, {})", values.len(), a.mem_cap());
//             for value in &values {
//                 catch_unwind(AssertUnwindSafe(|| {
//                     let v = a
//                         .get(&value)
//                         .unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
//                     assert_eq!(v, value);
//                 }))
//                 .unwrap_or_else(|e| {
//                     println!(
//                         "when searching for {value}\nin with tree len {}: {a:?}",
//                         values.len()
//                     );
//                     resume_unwind(e)
//                 });
//             }
//         }
//     }

//     #[test]
//     fn exhaust2() {
//         for _ in 0..=16 {
//             // let mut a = Tree::with_capacity(values.len());
//             let len = 17;
//             let mut a = Tree::default();
//             for value in (0..((1 << len) - 1)).rev() {
//                 // println!("i: {i}");
//                 // println!("{a:?}");
//                 a.insert(value);
//             }
//             // if i == 5 {
//             //   println!("aa {a:?}");
//             // }
//             // println!("({}, {})", values.len(), a.mem_cap());
//             for value in 0..((1 << len) - 1) {
//                 catch_unwind(AssertUnwindSafe(|| {
//                     let v = a
//                         .get(&value)
//                         .unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
//                     assert_eq!(v, &value);
//                 }))
//                 .unwrap_or_else(|e| {
//                     println!("when searching for {value}\nin with tree len {len}: {a:?}",);
//                     resume_unwind(e)
//                 });
//             }
//         }
//     }

//     #[test]
//     fn exhaust3() {
//         for _ in 0..=16 {
//             // let mut a = Tree::with_capacity(values.len());
//             let len = 18;
//             let mut a = Tree::default();
//             for value in 0..((1 << len) - 1) {
//                 // println!("i: {i}");
//                 // println!("{a:?}");
//                 a.insert(value);
//             }
//             // if i == 5 {
//             //   println!("aa {a:?}");
//             // }
//             // println!("({}, {})", values.len(), a.mem_cap());
//             for value in 0..((1 << len) - 1) {
//                 catch_unwind(AssertUnwindSafe(|| {
//                     let v = a
//                         .get(&value)
//                         .unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
//                     assert_eq!(v, &value);
//                 }))
//                 .unwrap_or_else(|e| {
//                     println!("when searching for {value}\nin with tree len {len}: {a:?}",);
//                     resume_unwind(e)
//                 });
//             }
//         }
//     }
// }
