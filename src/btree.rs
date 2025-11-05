use std::{cell::RefCell, marker::PhantomData, mem::MaybeUninit, ptr::NonNull, rc::Rc};

use sim::{
    lru_cache::Cache,
    simalloc::{SPtr, SRef, SRefMut, SSlice, SSliceMut, Simalloc},
};

pub struct BTree<T> {
    root: Option<NodeBox<T>>,
    max_elements: u16,
    sim: Rc<RefCell<Simalloc>>,
}

struct Node<T>(PhantomData<T>);

type NodePtr<T> = Option<NodeBox<T>>;

#[repr(transparent)]
struct NodeBox<T>(SPtr<Node<T>>);

struct Header<T> {
    num_elements: u16,
    max_elements: u16,
    node_size: u16,
    is_leaf: bool,
    _alignment: [T; 0],
}

// num_elements * size_of::<T>() + padding
//   + (num_elements + 1) * size_of::<NodePtr>() = 4096
//
// num_elements * size_of::<T>() + padding
//   + num_elements * size_of::<NodePtr>()
//   + size_of::<NodePtr>() = 4096
//
// num_elements * (size_of::T() + size_of::<NodePtr>())
//   size_of::<NodePtr>()
//   + padding = 4096
//
// num_elements = (4096 - size_of::<NodePtr>() - padding)
//                  / (size_of::T() + size_of::<NodePtr>())

pub const fn min_node_size<T>() -> usize {
    size_of::<Header<T>>()
        + 2 * size_of::<NodePtr<T>>()
        + align_of::<NodePtr<T>>().saturating_sub(align_of::<T>())
        + size_of::<T>()
}

pub const fn node_size_for_num_elements<T>(num_elements: usize) -> usize {
    let header_and_elems_size = size_of::<Header<T>>() + num_elements * size_of::<T>();
    let mut ptrs_offset = header_and_elems_size;
    if ptrs_offset % align_of::<NodePtr<T>>() > 0 {
        ptrs_offset += align_of::<NodePtr<T>>() - (ptrs_offset % align_of::<NodePtr<T>>());
    }
    ptrs_offset + (num_elements + 1) * size_of::<NodePtr<T>>()
}

pub const fn num_elements_for_node_size<T>(node_size: usize) -> usize {
    assert!(node_size >= min_node_size::<T>());
    let elems_and_ptrs_size = node_size - size_of::<Header<T>>() - size_of::<NodePtr<T>>();
    let mut max_elems = elems_and_ptrs_size / size_of::<T>() + size_of::<NodePtr<T>>();
    while node_size_for_num_elements::<T>(max_elems) > node_size {
        max_elems -= 1;
    }
    max_elems
    //   (node_size
    //     - size_of::<Header<T>>()
    //     - size_of::<NodePtr<T>>()
    //     - (align_of::<NodePtr<T>>().saturating_sub(align_of::<T>()))) // TODO better align?
    //     / (size_of::<T>() + size_of::<NodePtr<T>>())
}

pub fn sim_get<T>(
    base_addr: usize,
    depth: u8,
    height: u8,
    // center around lg(n) for an even hit rate
    mut num_elements_until_match: usize,
    node_cap: usize,
    page_size: usize,
    sim: &mut sim::hierarchy::Hierarchy,
    global_rng: &mut impl rand::Rng,
) -> Option<usize> {
    use rand::prelude::*;
    use rand_pcg::Pcg64Mcg;
    // println!("{base_addr:#x}");

    let ref_sim = std::cell::RefCell::new(sim);

    let mut node_rng = Pcg64Mcg::seed_from_u64(base_addr as u64);
    // Can it be smaller and maintain tree invariants?
    // let num_elements = node_rng.random_range(node_cap / 2..=node_cap);
    let num_elements = node_cap;

    let idx = global_rng.random_range(0..=num_elements);
    let gt = global_rng.next_u32() % 2 == 1;

    let sim_entries = |addr| {
        // self.entries_offset();
        let entries_offset = size_of::<Header<T>>();
        // let len = self.header().pin().num_elements;
        ref_sim.borrow_mut().access_addr(addr, size_of::<u16>());
        // TODO seed rng based on addr, use real cap
        let len = num_elements;
        // self.0
        //     .byte_add(entries_offset)
        //     .cast()
        //     .as_slice(len as usize)
        (addr + entries_offset, len)
    };

    let sim_ptrs = |addr| {
        // let len = self.header().pin().num_elements as usize + 1;
        ref_sim.borrow_mut().access_addr(addr, size_of::<u16>());
        let len = num_elements + 1;
        // let ptr_offset = self.ptrs_offset();
        //  let max_elements = self.header().pin().max_elements as usize;
        ref_sim
            .borrow_mut()
            .access_addr(addr + size_of::<u16>(), size_of::<u16>());
        let max_elements = node_cap;
        let mut offset = size_of::<Header<T>>() + size_of::<T>() * max_elements;
        if offset % align_of::<NodePtr<T>>() > 0 {
            offset += align_of::<NodePtr<T>>() - (offset % align_of::<NodePtr<T>>());
        }
        // self.0.byte_add(ptr_offset).cast().as_slice(len)
        (addr + offset, len)
    };

    let num_elements_until_match = &mut num_elements_until_match;
    let mut sim_find = |addr| {
        // let values = self.entries();
        let (values, mut size) = sim_entries(addr);
        // return values.binary_search(value);
        use std::{cmp::Ordering::*, hint};
        if size == 0 {
            return Err(0);
        }
        let mut base = 0usize;

        while size > 1 {
            let half = size / 2;
            let mid = base + half;

            // let cmp = f(&**self.pin_at(mid));
            ref_sim
                .borrow_mut()
                .access_addr(values + size_of::<T>() * mid, size_of::<T>());
            let cmp = mid.cmp(&idx);
            if mid != idx {
                *num_elements_until_match = num_elements_until_match.saturating_sub(1);
            }
            base = hint::select_unpredictable(cmp == Greater, base, mid);
            size -= half;
        }

        // let cmp = f(&**self.pin_at(base));
        ref_sim
            .borrow_mut()
            .access_addr(values + size_of::<T>() * base, size_of::<T>());
        let cmp = if *num_elements_until_match == 0 {
            Equal
        } else if gt {
            Greater
        } else {
            Less
        };
        // num_elements_until_match -= 1;
        // let cmp = base.cmp(&idx);
        if cmp == Equal {
            Ok(base)
        } else {
            let result = base + (cmp == Less) as usize;
            Err(result)
        }
    };

    match sim_find(base_addr) {
        Ok(i) => return Some(sim_entries(base_addr).0 + i * size_of::<T>()),
        Err(i) => {
            // let next = self.ptrs().into_value_at(i);
            let (next, _) = sim_ptrs(base_addr);
            ref_sim
                .borrow_mut()
                .access_addr(next + i * size_of::<NodePtr<T>>(), size_of::<NodePtr<T>>());
            // let Some(node) = &*next else { return None };
            if depth + 1 >= height {
                return None;
            }
            // node.get(value)
            node_rng.advance(i as _);
            let next_addr =
                node_rng.random_range(0..1 << (64 - 1 - page_size.ilog2())) << page_size.ilog2();
            sim_get::<T>(
                next_addr,
                depth + 1,
                height,
                *num_elements_until_match,
                node_cap,
                page_size,
                ref_sim.into_inner(),
                global_rng,
            )
        }
    }
}

impl<T> BTree<T> {
    pub fn with_node_size_in_sim(size: usize, sim: Rc<RefCell<Simalloc>>) -> Self {
        let num_elems = num_elements_for_node_size::<T>(size);
        assert!(node_size_for_num_elements::<T>(num_elems) <= size);
        assert!(num_elems <= u16::MAX as usize);
        println!("cap: {size} {num_elems}");
        Self::with_node_capacity_in_sim(num_elems as u16, sim)
    }

    pub fn with_node_capacity_in_sim(cap: u16, sim: Rc<RefCell<Simalloc>>) -> Self {
        Self {
            root: None,
            max_elements: cap,
            sim,
        }
    }

    #[allow(dead_code)]
    pub fn get(&self, value: &T) -> Option<&T>
    where
        T: Ord,
    {
        let Some(node) = &self.root else { return None };
        node.get(value)
    }

    pub fn insert(&mut self, value: T)
    where
        T: Ord,
    {
        let Some(root) = &mut self.root else {
            let mut root = NodeBox::with_capacity(self.max_elements, true, self.sim.clone());
            root.insert(value, &self.sim);
            self.root = Some(root);
            return;
        };

        let new = root.insert(value, &self.sim);
        let Some((new_pivot, new_sib)) = new else {
            return;
        };

        let new_root = NodeBox::root_with_capacity(
            self.max_elements,
            new_pivot,
            self.root.take().unwrap(),
            new_sib,
            self.sim.clone(),
        );
        self.root = Some(new_root);
    }
}

impl<T> NodeBox<T> {
    fn get(&self, value: &T) -> Option<&T>
    where
        T: Ord,
    {
        match self.find(value) {
            Ok(i) => return Some(&*self.entries().into_value_at(i)),
            Err(i) => {
                let next = self.ptrs().into_value_at(i);
                let Some(node) = &*next else { return None };
                node.get(value)
            }
        }
    }

    fn insert(&mut self, value: T, sim: &Rc<RefCell<Simalloc>>) -> Option<(T, NodeBox<T>)>
    where
        T: Ord,
    {
        let i = match self.find(&value) {
            Err(i) => i,
            Ok(i) => {
                self.entries_mut().pin_mut_at(i).write(value);
                return None;
            }
        };

        if self.header().pin().is_leaf {
            let Some((value, _)) = self.try_place_at(i, value, None) else {
                return None;
            };

            return self.split(i, value, None, sim).into();
        }

        let Some((new_pivot, new_child)) = self
            .ptrs_mut()
            .pin_mut_at(i)
            .as_mut()
            .unwrap()
            .insert(value, sim)
        else {
            return None;
        };

        let Some((new_pivot, new_child)) = self.try_place_at(i, new_pivot, Some(new_child)) else {
            return None;
        };

        self.split(i, new_pivot, new_child, sim).into()
    }

    fn try_place_at(&mut self, idx: usize, value: T, ptr: NodePtr<T>) -> Option<(T, NodePtr<T>)>
    where
        T: Ord,
    {
        let (max_elements, num_elements) = {
            let header = self.header();
            let header = header.pin();
            (header.max_elements as usize, header.num_elements as usize)
        };
        if num_elements >= max_elements || idx >= max_elements {
            return Some((value, ptr));
        }

        self.place_at(idx, value, ptr);
        None
    }

    fn place_at(&mut self, idx: usize, val: T, ptr_after_i: NodePtr<T>) {
        let (max_elements, num_elements) = {
            let header = self.header();
            let header = header.pin();
            (header.max_elements as usize, header.num_elements as usize)
        };
        assert!(idx < max_elements);
        assert!(num_elements < max_elements);
        assert!(
            idx <= num_elements,
            "{idx} <= {num_elements} ({max_elements})"
        );

        unsafe {
            let mut entries = self
                .0
                .byte_add_mut(self.entries_offset())
                .cast::<MaybeUninit<T>>()
                .as_slice_mut(num_elements + 1);
            entries.shift_insert(val, idx);

            if ptr_after_i.is_some() {
                let ptrs_len = num_elements + 1;
                let mut ptrs = self
                    .0
                    .byte_add_mut(self.ptrs_offset())
                    .cast::<MaybeUninit<NodePtr<T>>>()
                    .as_slice_mut(ptrs_len + 1);

                ptrs.shift_insert(ptr_after_i, idx + 1);
            }
        }

        self.header_mut().pin_mut().num_elements += 1;
    }

    fn split(
        &mut self,
        i: usize,
        value: T,
        ptr: Option<NodeBox<T>>,
        sim: &Rc<RefCell<Simalloc>>,
    ) -> (T, NodeBox<T>)
    where
        T: Ord,
    {
        let (max_elements, num_elements, is_leaf) = {
            let header = self.header();
            let header = header.pin();
            (
                header.max_elements,
                header.num_elements as usize,
                header.is_leaf,
            )
        };

        // TODO this isn't actually necessary, but it makes the code easier to write.
        let mut keys = Vec::with_capacity(num_elements + 1);
        let mut pointers = Vec::with_capacity(keys.capacity() + 1);
        unsafe {
            let mut entries = self.entries_mut();
            let keyspace = &mut keys.spare_capacity_mut()[..num_elements];
            assert_eq!(entries.len(), num_elements);
            for i in 0..num_elements {
                keyspace[i].write(entries.pin_mut_at(i).as_ptr().read());
            }
            // ptr::copy_nonoverlapping(
            //     self.entries_mut().as_mut_ptr() as *mut MaybeUninit<T>,
            //     keys.spare_capacity_mut().as_mut_ptr(),
            //     num_elements,
            // );
            keys.set_len(num_elements);
            // TODO set header.num_elements = 0?
        }

        unsafe {
            let mut ptrs = self.ptrs_mut().as_maybe_uninit();
            let ptrspace = &mut pointers.spare_capacity_mut()[..num_elements + 1];
            assert_eq!(ptrs.len(), num_elements + 1);
            for i in 0..num_elements + 1 {
                let mut p = ptrs.pin_mut_at(i);
                ptrspace[i].write(p.as_ptr().read());
                p.write(None);
            }
            // ptr::copy_nonoverlapping(
            //     self.ptrs_mut().as_mut_ptr() as *mut MaybeUninit<NodePtr<T>>,
            //     pointers.spare_capacity_mut().as_mut_ptr(),
            //     num_elements + 1,
            // );
            // ptr::write_bytes(self.ptrs_mut().as_mut_ptr(), 0, num_elements + 1);
            pointers.set_len(num_elements + 1);
            // TODO set header.num_elements = 0?
        }

        keys.insert(i, value);
        pointers.insert(i + 1, ptr);

        let mut new_sibling = NodeBox::<T>::with_capacity(max_elements, is_leaf, sim.clone());

        let median = if keys.len() % 2 == 0 {
            keys.len() / 2 + 1
        } else {
            keys.len() / 2
        };
        let for_new_sib = median + 1;
        let new_sib_len = keys.len() - for_new_sib;
        // unsafe {
        new_sibling.header_mut().pin_mut().num_elements = new_sib_len as u16;
        let mut new_sib_entries = new_sibling.entries_mut();
        debug_assert_eq!(keys[for_new_sib..].len(), new_sib_len);
        for (i, entry) in keys.drain(for_new_sib..).enumerate() {
            new_sib_entries.pin_mut_at(i).write(entry);
        }

        let mut new_sib_ptrs = new_sibling.ptrs_mut();
        debug_assert_eq!(pointers[for_new_sib..].len(), new_sib_len + 1);
        for (i, ptr) in pointers.drain(for_new_sib..).enumerate() {
            **new_sib_ptrs.pin_mut_at(i) = ptr;
        }
        // }

        let pivot = keys.pop().unwrap();

        // unsafe {
        self.header_mut().pin_mut().num_elements = median as u16;
        let mut entries = self.entries_mut();
        debug_assert_eq!(keys.len(), median);
        for (i, entry) in keys.drain(..).enumerate() {
            entries.pin_mut_at(i).write(entry);
        }

        let mut ptrs = self.ptrs_mut();
        debug_assert_eq!(pointers.len(), median + 1);
        for (i, ptr) in pointers.drain(..).enumerate() {
            **ptrs.pin_mut_at(i) = ptr;
        }
        // }

        (pivot, new_sibling)
    }

    fn entries_offset(&self) -> usize {
        size_of::<Header<T>>()
    }

    fn ptrs_offset(&self) -> usize {
        let max_elements = self.header().pin().max_elements as usize;
        let mut offset = size_of::<Header<T>>() + size_of::<T>() * max_elements;
        if offset % align_of::<NodePtr<T>>() > 0 {
            offset += align_of::<NodePtr<T>>() - (offset % align_of::<NodePtr<T>>());
        }
        offset
    }

    fn header_mut(&mut self) -> SRefMut<'_, Header<T>> {
        unsafe { self.0.byte_add_mut(0).cast() }
    }

    fn header(&self) -> SRef<'_, Header<T>> {
        unsafe { self.0.byte_add(0).cast() }
    }

    fn entries_mut(&mut self) -> SSliceMut<'_, MaybeUninit<T>> {
        unsafe {
            let entries_offset = self.entries_offset();
            let len = self.header().pin().num_elements;
            self.0
                .byte_add_mut(entries_offset)
                .cast()
                .as_slice_mut(len as usize)
        }
    }

    fn entries(&self) -> SSlice<'_, T> {
        unsafe {
            let entries_offset = self.entries_offset();
            let len = self.header().pin().num_elements;
            self.0
                .byte_add(entries_offset)
                .cast()
                .as_slice(len as usize)
        }
    }

    fn ptrs_mut(&mut self) -> SSliceMut<'_, NodePtr<T>> {
        unsafe {
            let len = self.header().pin().num_elements as usize + 1;
            let ptr_offset = self.ptrs_offset();
            self.0.byte_add_mut(ptr_offset).cast().as_slice_mut(len)
        }
    }

    fn ptrs(&self) -> SSlice<'_, NodePtr<T>> {
        unsafe {
            let len = self.header().pin().num_elements as usize + 1;
            let ptr_offset = self.ptrs_offset();
            self.0.byte_add(ptr_offset).cast().as_slice(len)
        }
    }

    fn find(&self, value: &T) -> Result<usize, usize>
    where
        T: Ord,
    {
        let values = self.entries();
        return values.binary_search(value);
    }
}

impl<T> NodeBox<T> {
    pub fn with_capacity(max_elements: u16, leaf: bool, sim: Rc<RefCell<Simalloc>>) -> Self {
        use std::{alloc, cmp::max};

        let size = node_size_for_num_elements::<T>(max_elements as usize);
        assert!(size <= u16::MAX as usize);

        let align = max(
            align_of::<NodePtr<T>>(),
            max(align_of::<T>(), align_of::<Header<T>>()),
        );

        let layout = alloc::Layout::from_size_align(size, align).unwrap();
        unsafe {
            let ptr = alloc::alloc_zeroed(layout);
            if ptr.is_null() {
                alloc::handle_alloc_error(layout)
            }

            let ptr = ptr.cast::<Node<T>>();
            let mut node = NodeBox(SPtr::new_in_sim(NonNull::new_unchecked(ptr), sim));

            {
                let mut header = node.header_mut();
                let mut header = header.pin_mut();
                header.node_size = size as u16;
                header.max_elements = max_elements;
                header.is_leaf = leaf;
            }

            node
        }
    }

    pub fn root_with_capacity(
        max_elements: u16,
        pivot: T,
        left: NodeBox<T>,
        right: NodeBox<T>,
        sim: Rc<RefCell<Simalloc>>,
    ) -> Self {
        let mut this = Self::with_capacity(max_elements, false, sim);
        unsafe {
            this.header_mut().pin_mut().num_elements = 1;
            this.entries_mut().pin_mut_at(0).write(pivot);

            let mut ptrs = this.ptrs_mut().as_maybe_uninit();
            ptrs.pin_mut_at(0).write(Some(left));
            ptrs.pin_mut_at(1).write(Some(right));
        }
        this
    }
}

impl<T> Drop for NodeBox<T> {
    fn drop(&mut self) {
        // deliberately bypasss the sim for dropping; we don't want to measure it.
        for p in self.ptrs_mut().into_inner() {
            p.take();
        }

        for e in self.entries_mut().into_inner() {
            unsafe { e.assume_init_drop() };
        }

        use std::alloc;
        let size = self.header().pin().node_size as usize;
        let layout = alloc::Layout::from_size_align(size, align_of::<NodePtr<T>>()).unwrap();
        unsafe {
            alloc::dealloc(self.0.into_inner_ptr().as_ptr().cast(), layout);
        }
    }
}

impl<T: std::fmt::Debug> BTree<T> {
    pub fn output_dot(&self) -> String {
        const TABLE_START: &str = "<TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\">";
        const TABLE_END: &str = "</TABLE>";

        use std::fmt::Write;

        let mut out = String::new();
        writeln!(
            out,
            "digraph {{\n  \
            node [shape=plaintext];\n  \
            rankdir=\"TB\";\n  \
            ranksep=\"0.02\";\n  \
            splines=polyline;\n"
        )
        .unwrap();

        let mut edges = String::new();

        // let mut num_nodes = 1;
        // let mut start = 0;
        let mut h = 0;
        // for h in 0..self.height {
        let mut queue = std::collections::VecDeque::new();
        let mut next = std::collections::VecDeque::new();
        if let Some(root) = &self.root {
            queue.push_back(root);
        }
        while !queue.is_empty() {
            // let mut c = 0;
            for current in queue.drain(..) {
                let ptr = current.header().into_ptr();
                writeln!(
                    out,
                    "n{ptr:?} [group=\"h{h}\",label=<\n\
              <TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\">"
                )
                .unwrap();
                writeln!(out, "    <TR><TD>").unwrap();
                for (i, v) in current.entries().into_inner().iter().enumerate() {
                    if i == 0 {
                        writeln!(out, "      {TABLE_START}\n        <TR>").unwrap();
                    }
                    writeln!(out, "          <TD PORT=\"p{i}\">{v:?}</TD>").unwrap();
                }
                writeln!(
                    out,
                    "          <TD PORT=\"p{}\"></TD>",
                    current.entries().len()
                )
                .unwrap();
                writeln!(out, "        </TR>\n      {TABLE_END}").unwrap();
                for (i, child) in current.ptrs().into_inner().iter().enumerate() {
                    let Some(child) = child else {
                        continue;
                    };
                    next.push_back(child);
                    writeln!(
                        edges,
                        "  n{ptr:?}:p{i}:s -> n{child:?}:n [weight=0.01]",
                        child = child.header().into_ptr()
                    )
                    .unwrap();
                }
                writeln!(out, "    </TD></TR>\n  {TABLE_END}\n>]\n").unwrap();
                // c += 1;
            }
            std::mem::swap(&mut queue, &mut next);
            h += 1;
        }

        out.push_str(&edges);

        writeln!(out, "}}").unwrap();

        out
    }
}

// #[cfg(test)]
// mod test {
//   use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

//   use super::*;

//   #[test]
//   fn node_size_for_num_u32s() {
//     assert_eq!(node_size_for_num_elements::<u32>(0), 16);
//     assert_eq!(node_size_for_num_elements::<u32>(1), 32);
//     assert_eq!(node_size_for_num_elements::<u32>(2), 40);
//     assert_eq!(node_size_for_num_elements::<u32>(3), 56);
//     assert_eq!(node_size_for_num_elements::<u32>(4), 64);
//     assert_eq!(node_size_for_num_elements::<u32>(5), 80);
//     assert_eq!(node_size_for_num_elements::<u32>(10), 136);
//     assert_eq!(node_size_for_num_elements::<u32>(20), 256);
//     assert_eq!(node_size_for_num_elements::<u32>(1000), 12016);
//   }

//   #[test]
//   fn u32_num_elemns_round_trips() {
//     let check = |i: usize| {
//       assert!(
//         num_elements_for_node_size::<u32>(node_size_for_num_elements::<u32>(i)) >= i,
//         "{} >= {i} @ {}",
//         num_elements_for_node_size::<u32>(node_size_for_num_elements::<u32>(i)),
//         node_size_for_num_elements::<u32>(i)
//       );
//     };
//     for i in 1..100 {
//       check(i);
//     }

//     for i in 1..100 {
//       check(i * 10)
//     }

//     for i in 1..100 {
//       check(i * 100)
//     }

//     for i in 1..100 {
//       check(i * 1000)
//     }
//   }

//   #[test]
//   fn u32_node_size_right_fits() {
//     for i in 5..32 {
//       let requested_size = 1 << i;
//       let num_elements = num_elements_for_node_size::<u32>(requested_size);
//       let min_size = node_size_for_num_elements::<u32>(num_elements);
//       assert!(requested_size >= min_size);
//       //   assert!(
//       //     node_size_for_num_elements::<u32>(num_elements + 1) > requested_size,
//       //     "{} > {} @ {num_elements}",
//       //     node_size_for_num_elements::<u32>(num_elements + 1),
//       //     requested_size
//       //   );
//     }
//   }

//   #[test]
//   fn u64_num_elemns_round_trips() {
//     for i in 1..100 {
//       assert_eq!(
//         num_elements_for_node_size::<u64>(node_size_for_num_elements::<u64>(i)),
//         i
//       );
//     }

//     for i in 1..100 {
//       assert_eq!(
//         num_elements_for_node_size::<u64>(node_size_for_num_elements::<u64>(i * 10)),
//         i * 10
//       );
//     }

//     for i in 1..100 {
//       assert_eq!(
//         num_elements_for_node_size::<u64>(node_size_for_num_elements::<u64>(i * 100)),
//         i * 100
//       );
//     }

//     for i in 1..100 {
//       assert_eq!(
//         num_elements_for_node_size::<u64>(node_size_for_num_elements::<u64>(i * 1000)),
//         i * 1000
//       );
//     }
//   }

//   #[test]
//   fn u64_node_size_right_fits() {
//     for i in 5..32 {
//       let requested_size = 1 << i;
//       let num_elements = num_elements_for_node_size::<u64>(requested_size);
//       let min_size = node_size_for_num_elements::<u64>(num_elements);
//       assert!(requested_size >= min_size);
//       assert!(node_size_for_num_elements::<u64>(num_elements + 1) > requested_size);
//     }
//   }

//   #[test]
//   fn leaf_insert() {
//     let mut node = NodeBox::<u32>::with_capacity(4, true);
//     assert_eq!(node.entries(), []);

//     let res = node.insert(0);
//     assert!(res.is_none());
//     assert_eq!(node.entries(), [0]);

//     let res = node.insert(1);
//     assert!(res.is_none());
//     assert_eq!(node.entries(), [0, 1]);

//     let res = node.insert(2);
//     assert!(res.is_none());
//     assert_eq!(node.entries(), [0, 1, 2]);

//     let res = node.insert(3);
//     assert!(res.is_none());
//     assert_eq!(node.entries(), [0, 1, 2, 3]);

//     let Some((p, sib)) = node.insert(4) else {
//       panic!()
//     };
//     assert_eq!(
//       (node.entries(), p, sib.entries()),
//       (&[0, 1][..], 2, &[3, 4][..])
//     );
//   }

//   #[test]
//   fn node_insert() {
//     let leaf_with = |i| {
//       let mut n = NodeBox::<u32>::with_capacity(4, true);
//       n.insert(i);
//       n
//     };
//     let mut node = NodeBox::<u32>::root_with_capacity(4, 1, leaf_with(10), leaf_with(20));
//     node.place_at(1, 2, Some(leaf_with(30)));
//     node.place_at(2, 3, Some(leaf_with(40)));
//     node.place_at(3, 4, Some(leaf_with(50)));

//     assert_eq!(node.entries(), [1, 2, 3, 4]);
//     assert_eq!(node.ptrs()[0].as_ref().unwrap().entries(), [10]);
//     assert_eq!(node.ptrs()[1].as_ref().unwrap().entries(), [20]);
//     assert_eq!(node.ptrs()[2].as_ref().unwrap().entries(), [30]);
//     assert_eq!(node.ptrs()[3].as_ref().unwrap().entries(), [40]);
//     assert_eq!(node.ptrs()[4].as_ref().unwrap().entries(), [50]);

//     println!(
//       "{}",
//       BTree {
//         root: node.into(),
//         max_elements: 4
//       }
//       .output_dot()
//     );
//   }

//   #[test]
//   fn leaf_split() {
//     let mut a = BTree::with_node_capacity(2);
//     for i in 0..=3 {
//       a.insert(i);
//       for j in 0..=i {
//         let v = a
//           .get(&j)
//           .unwrap_or_else(|| panic!("could not find {j} in {}", a.output_dot()));
//         assert_eq!(v, &j);
//       }
//     }

//     println!("{}", a.output_dot());

//     a.insert(4);

//     for i in 0..=4 {
//       let v = a
//         .get(&i)
//         .unwrap_or_else(|| panic!("could not find {i} in {}", a.output_dot()));
//       assert_eq!(v, &i);
//     }
//   }

//   #[test]
//   fn node_split() {
//     let mut a = BTree::with_node_capacity(2);
//     let max = 6;
//     for i in 0..max {
//       a.insert(i);
//       for j in 0..=i {
//         let v = a
//           .get(&j)
//           .unwrap_or_else(|| panic!("could not find {j} in {}", a.output_dot()));
//         assert_eq!(v, &j);
//       }
//     }

//     println!("{}", a.output_dot());

//     a.insert(max);

//     println!("{}", a.output_dot());

//     for i in 0..=max {
//       let v = a
//         .get(&i)
//         .unwrap_or_else(|| panic!("could not find {i} in {}", a.output_dot()));
//       assert_eq!(v, &i);
//     }
//   }

//   #[test]
//   fn exhaust_4() {
//     for i in 0..=16 {
//       let values = 0..((1 << i) - 1);
//       // let mut a = Tree::with_capacity(values.len());
//       let mut a = BTree::with_node_capacity(4);
//       for value in values.clone() {
//         // println!("i: {i}, {value}");
//         // println!("{}", a.output_dot());
//         a.insert(value);
//         // for j in 0..=value {
//         //   let v = a
//         //     .get(&j)
//         //     .unwrap_or_else(|| panic!("could not find {j} in {}", a.output_dot()));
//         //   assert_eq!(v, &j);
//         // }
//       }
//       // println!("{}", a.output_dot());
//       // if i == 5 {
//       //   println!("aa {a:?}");
//       // }
//       // println!("({}, {})", values.len(), a.mem_cap());
//       for value in values.clone() {
//         catch_unwind(AssertUnwindSafe(|| {
//           let v = a
//             .get(&value)
//             .unwrap_or_else(|| panic!("could not find {value} in {}", a.output_dot()));
//           assert_eq!(v, &value);
//         }))
//         .unwrap_or_else(|e| {
//           println!(
//             "when searching for {value}\nin with tree len {}: {}",
//             values.len(),
//             a.output_dot()
//           );
//           resume_unwind(e)
//         });
//       }
//     }
//   }

//   #[test]
//   fn exhaust_2k() {
//     for i in 0..=16 {
//       let values = 0..((1 << i) - 1);
//       // let mut a = Tree::with_capacity(values.len());
//       let mut a = BTree::with_node_capacity(1 << 11);
//       for value in values.clone() {
//         // println!("i: {i}, {value}");
//         // println!("{}", a.output_dot());
//         a.insert(value);
//         // for j in 0..=value {
//         //   let v = a
//         //     .get(&j)
//         //     .unwrap_or_else(|| panic!("could not find {j} in {}", a.output_dot()));
//         //   assert_eq!(v, &j);
//         // }
//       }
//       // println!("{}", a.output_dot());
//       // if i == 5 {
//       //   println!("aa {a:?}");
//       // }
//       // println!("({}, {})", values.len(), a.mem_cap());
//       for value in values.clone() {
//         catch_unwind(AssertUnwindSafe(|| {
//           let v = a
//             .get(&value)
//             .unwrap_or_else(|| panic!("could not find {value} in {}", a.output_dot()));
//           assert_eq!(v, &value);
//         }))
//         .unwrap_or_else(|e| {
//           println!(
//             "when searching for {value}\nin with tree len {}: {}",
//             values.len(),
//             a.output_dot()
//           );
//           resume_unwind(e)
//         });
//       }
//     }
//   }
// }
