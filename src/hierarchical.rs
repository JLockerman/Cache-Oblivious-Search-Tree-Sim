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

pub(crate) struct TreeInfo {
    pub top_tree_size: u64,      // T[d]
    pub bottom_tree_size: u64,   // B[d]
    pub top_tree_root_depth: u8, // D[d]
}

// Original
// Pos[d] = Pos[top_tree_root_depth[d]]  // offset of the subtree
//        + top_tree_size[d]             // offset of the children
//        + (i & top_tree_size[d])       // child number
//          * bottom_tree_size[d]        // child size
//
//
// B-Tree
// b-tree-pos = (node_size + 1) * node_idx + node_size * child (+1)?
// Pos[d] = Pos[top_tree_root_depth[d]]  // offset of the subtree
//        + top_tree_size[d]             // offset of the children
//        + (child_num[d])               // child number
//          * bottom_tree_size[d]        // child size
#[inline]
pub(crate) fn hindex(
  hpos: usize,
  ancestors: &mut [usize; 64],
  depth: u32,
  table: &[TreeInfo],
) -> usize {
  let elem = &table[depth as usize];
  ancestors[elem.top_tree_root_depth as usize]
    + elem.top_tree_size as usize
    + (hpos & elem.top_tree_size as usize) * elem.bottom_tree_size as usize
}

fn make_tree_info(height: u8, node_size: usize) -> Vec<TreeInfo> {
    // tree is at depth top_tree_depth, has height
    // top_tree_height = div_ceil(height)
    // idx = depth of bottom_tree = top_tree_depth + top_tree_height
    fn make_info_for_subtrees(
        top: bool,
        height: usize,
        top_tree_depth: u8,
        node_size: usize,
        tree_info: &mut [TreeInfo],
    ) {
        if height < 1 {
            dbg!(top, height);
            return;
        }

        let top_tree_height = if height == 1 { 0 } else { height.div_ceil(2) };
        let bottom_tree_height = height - top_tree_height;
        let num_children = node_size + 1;
        let num_nodes_for_height =
            |height| (num_children.pow(height as u32) - 1) / (num_children - 1);
        let top_tree_size = num_nodes_for_height(top_tree_height) * node_size;
        let bottom_tree_size = num_nodes_for_height(bottom_tree_height) * node_size;
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
        tree_info[top_tree_depth as usize + top_tree_height] = TreeInfo {
            top_tree_size: top_tree_size as u64,
            bottom_tree_size: bottom_tree_size as u64,
            top_tree_root_depth: top_tree_depth,
        };
        // }

        if height > 2 {
            make_info_for_subtrees(true, top_tree_height, top_tree_depth, node_size, tree_info);
        }

        if bottom_tree_height > 1 {
            make_info_for_subtrees(
                false,
                bottom_tree_height,
                top_tree_depth + top_tree_height as u8,
                node_size,
                tree_info,
            );
        }
    }

    if height == 0 {
        return vec![];
    }

    let mut tree_info: Vec<_> = (0..height)
        .map(|_| TreeInfo {
            top_tree_size: 0,
            bottom_tree_size: 0,
            top_tree_root_depth: 0,
        })
        .collect();
    tree_info[0] = TreeInfo {
        top_tree_size: 0,
        bottom_tree_size: ((((node_size + 1).pow(height as u32) - 1) / ((node_size + 1) - 1))
            * node_size) as u64,
        top_tree_root_depth: 0,
    };
    make_info_for_subtrees(false, height as usize, 0, node_size, &mut tree_info);
    tree_info
}
