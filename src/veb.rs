use std::{fmt::Debug, mem::MaybeUninit};

pub(crate) mod constants;

pub fn make_veb_in<K: Clone + Debug>(elements: &[K], out: &mut [Option<K>]) {
  fn layout<K: Clone + Debug>(
    table: &[TreeInfo],
    elements: &[K],
    depth: u32,
    bfs_pos: usize,
    ancestors: &mut [usize; 64],
    out: &mut [Option<K>],
  ) {
    if elements.is_empty() {
      return;
    }
    // using the formula from
    // "Cache Oblivious Search Trees via Binary Trees of Small Height"
    //    Pos[d] = Pos[D[d]] + T[d] + (i and T [d]) * B[d]
    // translating to local names
    // pos = ancestors[top_tree_root_depth[d]]   (offset of the subtree)
    //     + top_tree_size[d]                    (offset of the children)
    //     + (bfs_pos & top_tree_size[d])        (child number)
    //       * bottom_tree_size[d]               (child size)
    let pos = veb_index(bfs_pos, ancestors, depth, table);
    ancestors[depth as usize] = pos;
    let root = elements.len() / 2;

    let old = out[pos].replace(elements[root].clone());
    assert!(old.is_none(), "{old:?}");

    layout(
      table,
      &elements[..root],
      depth + 1,
      2 * bfs_pos,
      ancestors,
      out,
    );
    if root + 1 < elements.len() {
      layout(
        table,
        &elements[(root + 1)..],
        depth + 1,
        2 * bfs_pos + 1,
        ancestors,
        out,
      );
    }
  }

  // height is log_2(num_elements)
  let num_elements = elements.len();
  if num_elements == 0 {
    return;
  }

  if num_elements == 1 {
    return out[0] = elements[0].clone().into();
  }

  let smallest_fitting_pow2 = if num_elements % 2 == 0 {
    (num_elements + 1).next_power_of_two()
  } else {
    num_elements.next_power_of_two()
  };
  let height = smallest_fitting_pow2.trailing_zeros();
  let mut ancestors = [0; 64];
  // assert_eq!(out.len(), smallest_fitting_pow2 - 1);
  layout(
    constants::LOOKUP[height as usize],
    elements,
    0,
    1,
    &mut ancestors,
    out,
  );
}

pub fn make_veb<K: Debug>(mut elements: Vec<K>) -> Vec<Option<K>> {
  fn layout<K: Debug>(
    table: &[TreeInfo],
    elements: &mut [MaybeUninit<K>],
    depth: u32,
    bfs_pos: usize,
    ancestors: &mut [usize; 64],
    out: &mut [Option<K>],
  ) {
    if elements.is_empty() {
      return;
    }
    // using the formula from
    // "Cache Oblivious Search Trees via Binary Trees of Small Height"
    //    Pos[d] = Pos[D[d]] + T[d] + (i and T [d]) * B[d]
    // translating to local names
    // pos = ancestors[top_tree_root_depth[d]]   (offset of the subtree)
    //     + top_tree_size[d]                    (offset of the children)
    //     + (bfs_pos & top_tree_size[d])        (child number)
    //       * bottom_tree_size[d]               (child size)
    let pos = veb_index(bfs_pos, ancestors, depth, table);
    dbg!(pos, bfs_pos, depth, &ancestors, table);
    // dbg!(pos, bfs_pos);
    ancestors[depth as usize] = pos;
    let root = elements.len() / 2;

    let old = unsafe { out[pos].replace(elements[root].assume_init_read()) };
    assert!(old.is_none(), "{old:?}");

    layout(
      table,
      &mut elements[..root],
      depth + 1,
      2 * bfs_pos,
      ancestors,
      out,
    );
    if root + 1 < elements.len() {
      layout(
        table,
        &mut elements[(root + 1)..],
        depth + 1,
        2 * bfs_pos + 1,
        ancestors,
        out,
      );
    }
  }

  // height is log_2(num_elements)
  let num_elements = elements.len();
  if num_elements == 0 {
    return vec![];
  }

  if num_elements == 1 {
    return elements.into_iter().map(|v| Some(v)).collect();
  }

  let smallest_fitting_pow2 = if num_elements % 2 == 0 {
    (num_elements + 1).next_power_of_two()
  } else {
    num_elements.next_power_of_two()
  };
  let height = smallest_fitting_pow2.trailing_zeros();
  // dbg!(num_elements, num_elements.next_power_of_two(), height);
  let elements = unsafe {
    elements.set_len(0);
    &mut elements.spare_capacity_mut()[0..num_elements]
  };
  let mut ancestors = [0; 64];
  let mut output = Vec::new();
  output.resize_with(smallest_fitting_pow2 - 1, || None);
  layout(
    constants::LOOKUP[height as usize],
    elements,
    0,
    1,
    &mut ancestors,
    &mut output,
  );
  output
}

pub fn search<K: Ord>(veb_tree: &[Option<K>], value: &K) -> Option<usize> {
  use std::cmp::Ordering::*;

  let num_elements = veb_tree.len();
  if num_elements == 0 {
    return None;
  }

  let height = if num_elements % 2 == 0 {
    (num_elements + 1).next_power_of_two().trailing_zeros()
  } else {
    num_elements.next_power_of_two().trailing_zeros()
  };
  let mut ancestors = [0; 64];
  let table = constants::LOOKUP[height as usize];
  assert_eq!(table.len(), height as usize);

  let mut pos = 0;
  let mut bfs_pos = 1;
  let mut depth = 0;
  while pos < num_elements {
    let Some(pivot) = &veb_tree[pos] else {
      return None;
    };
    bfs_pos = match value.cmp(pivot) {
      Equal => return pos.into(),
      Less => 2 * bfs_pos,
      Greater => 2 * bfs_pos + 1,
    };
    depth += 1;

    pos = veb_index(bfs_pos, &mut ancestors, depth, table);
    ancestors[depth as usize] = pos;
  }

  None
}

#[derive(Debug)]
pub(crate) struct TreeInfo {
  pub top_tree_size: u64,      // T[d]
  pub bottom_tree_size: u64,   // B[d]
  pub top_tree_root_depth: u8, // D[d]
}

#[inline]
pub(crate) fn veb_index(
  bfs_pos: usize,
  ancestors: &mut [usize; 64],
  depth: u32,
  table: &[TreeInfo],
) -> usize {
  let elem = &table[depth as usize];
  ancestors[elem.top_tree_root_depth as usize]
    + elem.top_tree_size as usize
    + (bfs_pos & elem.top_tree_size as usize) * elem.bottom_tree_size as usize
}

#[cfg(test)]
mod test {
  use std::collections::BTreeSet;
  use std::panic::{catch_unwind, resume_unwind};
  use std::{
    cmp,
    collections::{HashMap, HashSet},
  };

  use proptest::property_test;

  use super::*;

  #[test]
  fn count_to_3() {
    let values: Vec<_> = vec![1, 2, 3];
    let a = make_veb(values.clone());
    println!("{a:?}");
    for value in values {
      catch_unwind(|| {
        let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
        assert_eq!(a[i].unwrap(), value);
      })
      .unwrap_or_else(|e| {
        println!(
          "when searching for {value}\nin with tree len {}: {a:?}",
          a.len()
        );
        resume_unwind(e)
      });
    }
  }

  #[test]
  fn count_to_10() {
    let values: Vec<_> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let a = make_veb(values.clone());
    println!("{a:?}");
    for value in values {
      catch_unwind(|| {
        let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
        assert_eq!(a[i].unwrap(), value);
      })
      .unwrap_or_else(|e| {
        println!(
          "when searching for {value}\nin with tree len {}: {a:?}",
          a.len()
        );
        resume_unwind(e)
      });
    }
  }

  #[test]
  fn count_to_8() {
    let values: Vec<_> = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let a = make_veb(values.clone());
    println!("{a:?}");
    for value in values {
      catch_unwind(|| {
        let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
        assert_eq!(a[i].unwrap(), value);
      })
      .unwrap_or_else(|e| {
        println!(
          "when searching for {value}\nin with tree len {}: {a:?}",
          a.len()
        );
        resume_unwind(e)
      });
    }
  }

  #[test]
  fn count_to_8_in() {
    let values: Vec<_> = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let mut a = vec![None; 15];
    make_veb_in(&values, &mut a);
    println!("{a:?}");
    for value in values {
      catch_unwind(|| {
        let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
        assert_eq!(a[i].unwrap(), value);
      })
      .unwrap_or_else(|e| {
        println!(
          "when searching for {value}\nin with tree len {}: {a:?}",
          a.len()
        );
        resume_unwind(e)
      });
    }
  }

  #[property_test]
  fn prop(data: BTreeSet<u32>) {
    let values: Vec<_> = data.into_iter().collect();
    let a = make_veb(values.clone());
    for value in values {
      catch_unwind(|| {
        let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
        assert_eq!(a[i].unwrap(), value);
      })
      .unwrap_or_else(|e| {
        println!(
          "when searching for {value}\nin with tree len {}: {a:?}",
          a.len()
        );
        resume_unwind(e)
      });
    }
  }

  #[test]
  fn prop_repro() {
    let values: Vec<_> = vec![
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
      26, 27, 28, 29, 30, 31,
    ];
    let a = make_veb(values.clone());
    for value in values {
      catch_unwind(|| {
        let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
        assert_eq!(a[i].unwrap(), value);
      })
      .unwrap_or_else(|e| {
        println!(
          "when searching for {value}\nin with tree len {}: {a:?}",
          a.len()
        );
        resume_unwind(e)
      });
    }
  }

  #[test]
  fn exhaust() {
    for i in 0..=16 {
      let values: Vec<_> = (0..((1 << i) - 1)).collect();
      let a = simple_make_veb(&values);
      for value in values {
        catch_unwind(|| {
          let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
          assert_eq!(a[i].unwrap(), value);
        })
        .unwrap_or_else(|e| {
          println!(
            "when searching for {value}\nin with tree len {}: {a:?}",
            a.len()
          );
          resume_unwind(e)
        });
      }
    }
  }

  #[test]
  fn exhaust_t() {
    for i in 0..=16 {
      let values: Vec<_> = (0..((1 << i) - 1)).collect();
      let a = make_veb(values.clone());
      for value in values {
        catch_unwind(|| {
          let i = search(&a, &value).unwrap_or_else(|| panic!("could not find {value} in {a:?}"));
          assert_eq!(a[i].unwrap(), value);
        })
        .unwrap_or_else(|e| {
          println!(
            "when searching for {value}\nin with tree len {}: {a:?}",
            a.len()
          );
          resume_unwind(e)
        });
      }
    }
  }

  #[test]
  fn exhaust_build() {
    for i in 0..=16 {
      let values: Vec<_> = (0..((1 << i) - 1)).collect();
      let a = simple_make_veb(&values);
      catch_unwind(|| {
        let b = make_veb(values.clone());
        assert_eq!(a, b, "{a:2?}, {b:2?}");
      })
      .unwrap_or_else(|e| {
        println!("with tree len {}, data {values:?}", a.len());
        resume_unwind(e)
      });
    }
  }

  fn simple_make_veb<K: PartialOrd + Copy + Debug>(elements: &[K]) -> Vec<Option<K>> {
    if elements.is_empty() {
      return vec![];
    }
    assert!(elements.is_sorted());
    assert_eq!(elements.len() % 2, 1, "{}", elements.len());
    let (height, tree) = make_tree(elements);
    match tree {
      None => vec![],
      Some(Node {
        value,
        height: 1,
        left: None,
        right: None,
      }) => {
        vec![value.into()]
      }
      Some(tree) => slv(tree, height),
    }
  }

  fn slv<K: PartialOrd + Copy + Debug>(tree: Node<K>, height: u8) -> Vec<Option<K>> {
    if let Node {
      value,
      height: _,
      left: None,
      right: None,
    } = tree
    {
      assert_eq!(height, 1, "{value:?}");
      return vec![value.into()];
    }

    let top_tree_size = height.div_ceil(2);
    let (top_tree, bottom_trees) = split(tree, top_tree_size);
    let mut top = slv(top_tree, top_tree_size);
    for bottom in bottom_trees {
      top.extend(slv(bottom, height - top_tree_size));
    }
    top
  }

  fn split<K: PartialOrd + Copy>(mut tree: Node<K>, h0: u8) -> (Node<K>, Vec<Node<K>>) {
    fn take<K: PartialOrd + Copy>(node: &mut Node<K>, d: u8, h0: u8, subtrees: &mut Vec<Node<K>>) {
      let next = d + 1;
      if next > h0 {
        if let Some(left) = node.left.take() {
          assert!(left.value < node.value);
          subtrees.push(*left);
        }
        if let Some(right) = node.right.take() {
          assert!(right.value > node.value);
          subtrees.push(*right);
        }
      } else {
        if let Some(left) = node.left.as_mut() {
          assert!(left.value < node.value);
          take(left, next, h0, subtrees);
        }
        if let Some(right) = node.right.as_mut() {
          assert!(right.value > node.value);
          take(right, next, h0, subtrees);
        }
      }
    }

    let mut subtrees = vec![];
    take(&mut tree, 1, h0, &mut subtrees);
    (tree, subtrees)
  }

  #[derive(Debug)]
  struct Node<K> {
    value: K,
    height: u8,
    left: Option<Box<Node<K>>>,
    right: Option<Box<Node<K>>>,
  }

  fn make_tree<K: PartialOrd + Copy + Debug>(elements: &[K]) -> (u8, Option<Node<K>>) {
    use std::cmp::max;
    match elements {
      [] => (0, None),
      [e] => (
        1,
        Node {
          value: *e,
          height: 1,
          left: None,
          right: None,
        }
        .into(),
      ),
      es @ [..] => {
        let mid = es.len() / 2;
        let value = es[mid];
        let (left_height, left) = make_tree(&es[0..mid]);
        let (right_height, right) = make_tree(&es[mid + 1..]);
        let (left, right) = (left.map(Box::new), right.map(Box::new));
        let height = max(left_height, right_height) + 1;
        (
          height,
          Node {
            value,
            height,
            left,
            right,
          }
          .into(),
        )
      }
    }
  }

  #[test]
  fn make_table() {
    for h in 0..64 {
      // for h in 0..4 {
      catch_unwind(|| println!("&{:?},", make_tree_info2(h))).unwrap_or_else(|e| {
        println!("when creating table of size {h}");
        resume_unwind(e)
      });
    }
  }

  fn make_tree_info2(height: u8) -> Vec<(u64, u64, u8)> {
    // tree is at depth top_tree_depth, has height
    // top_tree_height = div_ceil(height)
    // idx = depth of bottom_tree = top_tree_depth + top_tree_height
    fn make_info_for_subtrees(
      top: bool,
      height: usize,
      top_tree_depth: u8,
      tree_info: &mut [(u64, u64, u8)],
    ) {
      if height < 1 {
        dbg!(top, height);
        return;
      }

      let top_tree_height = if height == 1 { 0 } else { height.div_ceil(2) };
      let bottom_tree_height = height - top_tree_height;
      let top_tree_size = (1 << top_tree_height) - 1;
      let bottom_tree_size = (1 << bottom_tree_height) - 1;
      let depth = top_tree_depth as usize + top_tree_height;
      dbg!(
        top,
        height,
        depth,
        top_tree_depth,
        top_tree_height,
        top_tree_depth as usize + top_tree_height,
        top_tree_size,
        bottom_tree_size,
      );

      // if !seen {
      tree_info[top_tree_depth as usize + top_tree_height] =
        (top_tree_size, bottom_tree_size, top_tree_depth);
      // }

      if height > 2 {
        make_info_for_subtrees(true, top_tree_height, top_tree_depth, tree_info);
      }

      if bottom_tree_height > 1 {
        make_info_for_subtrees(
          false,
          bottom_tree_height,
          top_tree_depth + top_tree_height as u8,
          tree_info,
        );
      }
    }

    if height == 0 {
      return vec![];
    }

    let mut tree_info = vec![(0, 0, 0); height as usize];
    tree_info[0] = (0, (1 << height) - 1, 0);
    make_info_for_subtrees(false, height as usize, 0, &mut tree_info);
    tree_info
  }

  //

  #[test]
  fn c_stats() {
    let mut values: HashMap<_, usize> = HashMap::with_capacity(2146);
    for t in constants::LOOKUP {
      for &TreeInfo {
        top_tree_size,
        bottom_tree_size,
        top_tree_root_depth,
      } in t
      {
        *values
          .entry((top_tree_size, bottom_tree_size, top_tree_root_depth))
          .or_default() += 1
      }
    }

    let mut max = 0;
    let mut min = usize::MAX;
    let mut sum = 0;
    let mut count = 0;
    let mut values: Vec<_> = values
      .into_iter()
      .inspect(|(_, v)| {
        max = cmp::max(*v, max);
        min = cmp::min(*v, min);
        sum += v;
        count += 1;
      })
      .collect();
    values.sort_unstable();
    println!("max {max}");
    println!("min {min}");
    println!("avg {}", sum as f64 / count as f64);
    println!("count {count}");
    // for (k, v) in values {
    //   println!("{k:?}: {:#<v$}", "");
    // }
  }

  #[test]
  fn c_stats3() {
    let mut covering: HashMap<_, Vec<_>> = HashMap::with_capacity(64);
    let mut max_covered_by: HashMap<_, _> = HashMap::with_capacity(64);
    let mut mapping: HashMap<_, Vec<_>> = HashMap::with_capacity(2146);
    for (i, t) in constants::LOOKUP.into_iter().enumerate().skip(0).rev() {
      let mut potential: HashSet<usize> = (0..64).collect();
      for (
        j,
        &TreeInfo {
          top_tree_size,
          bottom_tree_size,
          top_tree_root_depth,
        },
      ) in t.into_iter().enumerate().rev()
      {
        let has = mapping
          .entry((top_tree_size, bottom_tree_size, top_tree_root_depth))
          .or_default();

        let other = has.iter().map(|(i, _)| i).copied().collect();
        potential = potential.intersection(&other).copied().collect();

        has.push((i, j))
      }
      for q in &potential {
        covering.entry(*q).or_default().push(i);
      }
      let mut p: Vec<_> = potential.into_iter().collect();
      p.sort_unstable();
      max_covered_by.insert(i, p.last().copied());
    }

    {
      let mut elements: Vec<_> = covering.into_iter().collect();
      elements.sort_unstable();
      println!("{elements:?}");
    }
    println!("");
    {
      let mut elements: Vec<_> = max_covered_by.into_iter().collect();
      elements.sort_unstable();
      println!("{elements:?}");
    }
  }
}
