// Based on https://rust-unofficial.github.io/too-many-lists/fourth-final.html.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct List<T> {
    head: Link<T>,
    tail: Link<T>,
    list_id: usize,
    len: usize,
}

pub type NodeElem<T> = Rc<RefCell<Node<T>>>;
type Link<T> = Option<NodeElem<T>>;

pub struct Node<T> {
    elem: T,
    next: Link<T>,
    prev: Link<T>,
    list_id: usize,
}

impl<T> Node<T> {
    fn new(elem: T, list_id: usize) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Node {
            elem,
            prev: None,
            next: None,
            list_id,
        }))
    }

    pub fn value(&self) -> &T {
        &self.elem
    }

    pub fn value_mut(&mut self) -> &mut T {
        &mut self.elem
    }
}

impl<T> List<T> {
    pub fn new() -> Self {
        static NEXT: LazyLock<AtomicUsize> = LazyLock::new(|| 0.into());
        let list_id = NEXT.fetch_add(1, Ordering::AcqRel);

        List {
            head: None,
            tail: None,
            list_id,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // new_head must not already be on list!
    pub fn push_front_node(&mut self, new_head: NodeElem<T>) {
        self.len += 1;
        self.check_list(&new_head);
        assert!(new_head.borrow().next.is_none());
        assert!(new_head.borrow().prev.is_none());
        match self.head.take() {
            Some(old_head) => {
                old_head.borrow_mut().prev = Some(new_head.clone());
                new_head.borrow_mut().next = Some(old_head);
                self.head = Some(new_head);
            }
            None => {
                self.tail = Some(new_head.clone());
                self.head = Some(new_head);
            }
        }
    }

    pub fn push_front(&mut self, elem: T) {
        self.push_front_node(Node::new(elem, self.list_id))
    }

    #[allow(unused)]
    pub fn push_back(&mut self, elem: T) {
        self.len += 1;
        let new_tail = Node::new(elem, self.list_id);
        match self.tail.take() {
            Some(old_tail) => {
                old_tail.borrow_mut().next = Some(new_tail.clone());
                new_tail.borrow_mut().prev = Some(old_tail);
                self.tail = Some(new_tail);
            }
            None => {
                self.head = Some(new_tail.clone());
                self.tail = Some(new_tail);
            }
        }
    }

    pub fn pop_back(&mut self) -> Option<T> {
        self.pop_back_node()
            .map(|x| Rc::try_unwrap(x).ok().unwrap().into_inner().elem)
    }

    pub fn pop_back_node(&mut self) -> Option<NodeElem<T>> {
        self.tail.take().inspect(|old_tail| {
            self.len -= 1;
            match old_tail.borrow_mut().prev.take() {
                Some(new_tail) => {
                    new_tail.borrow_mut().next.take();
                    self.tail = Some(new_tail);
                }
                None => {
                    self.head.take();
                }
            }
        })
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.head.take().map(|old_head| {
            self.len -= 1;
            match old_head.borrow_mut().next.take() {
                Some(new_head) => {
                    new_head.borrow_mut().prev.take();
                    self.head = Some(new_head);
                }
                None => {
                    self.tail.take();
                }
            }
            Rc::try_unwrap(old_head).ok().unwrap().into_inner().elem
        })
    }

    #[allow(unused)]
    pub fn peek_front(&self) -> Option<Ref<T>> {
        self.head
            .as_ref()
            .map(|node| Ref::map(node.borrow(), |node| &node.elem))
    }

    pub fn peek_front_node(&self) -> Option<NodeElem<T>> {
        self.head.clone()
    }

    #[allow(unused)]
    pub fn peek_back(&self) -> Option<Ref<T>> {
        self.tail
            .as_ref()
            .map(|node| Ref::map(node.borrow(), |node| &node.elem))
    }

    #[allow(unused)]
    pub fn peek_back_node(&self) -> Option<NodeElem<T>> {
        self.tail.clone()
    }

    #[allow(unused)]
    pub fn peek_back_mut(&mut self) -> Option<RefMut<T>> {
        self.tail
            .as_ref()
            .map(|node| RefMut::map(node.borrow_mut(), |node| &mut node.elem))
    }

    #[allow(unused)]
    pub fn peek_front_mut(&mut self) -> Option<RefMut<T>> {
        self.head
            .as_ref()
            .map(|node| RefMut::map(node.borrow_mut(), |node| &mut node.elem))
    }

    // node must be on this list!
    pub fn move_to_front(&mut self, node: NodeElem<T>) {
        self.check_list(&node);
        let (prev, mut next) = {
            let node = node.borrow();
            (node.prev.clone(), node.next.clone())
        };

        // If node is already the head.
        let prev = match prev {
            Some(x) => x,
            None => {
                if self.head.as_ref().map(|x| x.as_ptr()) != Some(node.as_ptr()) {
                    panic!("move_to_front called on node with prev == None, but isn't head");
                }
                return;
            }
        };

        prev.borrow_mut().next = next.clone();
        if let Some(next) = &mut next {
            next.borrow_mut().prev = Some(prev.clone());
        }

        let mut nodeb = node.borrow_mut();

        if nodeb.next.is_none() {
            self.tail = Some(prev.clone());
        }

        nodeb.prev = None;
        nodeb.next = self.head.clone();
        self.head.clone().unwrap().borrow_mut().prev = Some(node.clone());
        self.head = Some(node.clone());
    }

    fn check_list(&self, node: &NodeElem<T>) {
        assert_eq!(node.borrow().list_id, self.list_id);
    }
}

impl<T> Drop for List<T> {
    fn drop(&mut self) {
        while self.pop_front().is_some() {}
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::iter::IntoIterator for List<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self)
    }
}

pub struct IntoIter<T>(List<T>);

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.0.pop_front()
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.0.pop_back()
    }
}

// const _: () = {
//     use std::fmt::{self, Debug};
//     impl<T> Debug for List<T>
//     where
//         T: Debug,
//     {
//         fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//             f.debug_list().entries(self.iter()).finish()
//         }
//     }
// };

#[cfg(test)]
mod test {
    use super::{List, NodeElem};

    #[test]
    fn basics() {
        let mut list = List::new();
        assert_eq!(list.len(), 0);

        // Check empty list behaves right
        assert_eq!(list.pop_front(), None);

        // Populate list
        list.push_front(1);
        assert_eq!(list.len(), 1);
        list.push_front(2);
        assert_eq!(list.len(), 2);
        list.push_front(3);
        assert_eq!(list.len(), 3);

        // Check normal removal
        assert_eq!(list.pop_front(), Some(3));
        assert_eq!(list.len(), 2);
        assert_eq!(list.pop_front(), Some(2));
        assert_eq!(list.len(), 1);

        // Push some more just to make sure nothing's corrupted
        list.push_front(4);
        list.push_front(5);
        assert_eq!(list.len(), 3);

        // Check normal removal
        assert_eq!(list.pop_front(), Some(5));
        assert_eq!(list.pop_front(), Some(4));
        assert_eq!(list.len(), 1);

        // Check exhaustion
        assert_eq!(list.pop_front(), Some(1));
        assert_eq!(list.pop_front(), None);
        assert_eq!(list.len(), 0);

        // ---- back -----

        // Check empty list behaves right
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);

        // Populate list
        list.push_back(1);
        list.push_back(2);
        list.push_back(3);
        assert_eq!(list.len(), 3);

        // Check normal removal
        assert_eq!(list.pop_back(), Some(3));
        assert_eq!(list.pop_back(), Some(2));
        assert_eq!(list.len(), 1);

        // Push some more just to make sure nothing's corrupted
        list.push_back(4);
        list.push_back(5);
        assert_eq!(list.len(), 3);

        // Check normal removal
        assert_eq!(list.pop_back(), Some(5));
        assert_eq!(list.pop_back(), Some(4));
        assert_eq!(list.len(), 1);

        // Check exhaustion
        assert_eq!(list.pop_back(), Some(1));
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn peek() {
        let mut list = List::new();
        assert!(list.peek_front().is_none());
        assert!(list.peek_back().is_none());
        assert!(list.peek_front_mut().is_none());
        assert!(list.peek_back_mut().is_none());

        list.push_front(1);
        list.push_front(2);
        list.push_front(3);

        assert_eq!(&*list.peek_front().unwrap(), &3);
        assert_eq!(&mut *list.peek_front_mut().unwrap(), &mut 3);
        assert_eq!(&*list.peek_back().unwrap(), &1);
        assert_eq!(&mut *list.peek_back_mut().unwrap(), &mut 1);
    }

    fn check_node_eq<T>(lhs: &NodeElem<T>, rhs: &NodeElem<T>) {
        assert_eq!(lhs.as_ptr(), rhs.as_ptr());
    }

    fn check_node_opt_eq<T>(lhs: &Option<NodeElem<T>>, rhs: &Option<NodeElem<T>>) {
        assert_eq!(
            lhs.as_ref().map(|x| x.as_ptr()),
            rhs.as_ref().map(|x| x.as_ptr())
        );
    }

    #[test]
    #[should_panic]
    fn wrong_list_a() {
        let mut list_a = List::new();
        let mut list_b = List::new();

        list_a.push_front(3);
        list_b.push_front_node(list_a.pop_back_node().unwrap());
    }

    #[test]
    #[should_panic]
    fn wrong_list_b() {
        let mut list_a = List::new();
        let mut list_b = List::new();

        list_a.push_front(3);
        list_b.move_to_front(list_a.pop_back_node().unwrap());
    }

    #[test]
    fn move_to_front_1() {
        let mut list = List::new();
        list.push_back(1);
        /* scope */
        {
            let node = list.peek_front_node().unwrap();
            list.move_to_front(node.clone());

            let nodeb = node.borrow();
            check_node_opt_eq(&nodeb.prev, &None);
            check_node_opt_eq(&nodeb.next, &None);

            check_node_eq(&list.head.clone().unwrap(), &node.clone());
            check_node_eq(&list.tail.clone().unwrap(), &node.clone());
        }

        assert_eq!(list.len(), 1);
        let mut it = list.into_iter();
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn move_to_front_2a() {
        let mut list = List::new();
        list.push_back(1);
        list.push_back(2);

        /* scope */
        {
            let one = list.peek_front_node().unwrap();
            let two = list.peek_back_node().unwrap();
            list.move_to_front(two.clone());

            let twob = two.borrow();
            check_node_opt_eq(&twob.prev, &None);
            check_node_opt_eq(&twob.next, &Some(one.clone()));

            let oneb = one.borrow();
            check_node_opt_eq(&oneb.prev, &Some(two.clone()));
            check_node_opt_eq(&oneb.next, &None);

            check_node_eq(&list.head.clone().unwrap(), &two.clone());
            check_node_eq(&list.tail.clone().unwrap(), &one.clone());
        }

        assert_eq!(list.len(), 2);
        let mut it = list.into_iter();
        assert_eq!(it.next(), Some(2));
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn move_to_front_2b() {
        let mut list = List::new();
        list.push_back(1);
        list.push_back(2);

        /* scope */
        {
            let one = list.peek_front_node().unwrap();
            let two = list.peek_back_node().unwrap();
            list.move_to_front(one.clone());

            let twob = two.borrow();
            check_node_opt_eq(&twob.prev, &Some(one.clone()));
            check_node_opt_eq(&twob.next, &None);

            let oneb = one.borrow();
            check_node_opt_eq(&oneb.prev, &None);
            check_node_opt_eq(&oneb.next, &Some(two.clone()));

            check_node_eq(&list.head.clone().unwrap(), &one.clone());
            check_node_eq(&list.tail.clone().unwrap(), &two.clone());
        }

        assert_eq!(list.len(), 2);
        let mut it = list.into_iter();
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), Some(2));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn move_to_front_3a() {
        let mut list = List::new();
        list.push_back(1);
        list.push_back(2);
        list.push_back(3);

        /* scope */
        {
            let one = list.peek_front_node().unwrap();
            let two = one.borrow().next.clone().unwrap();
            let three = list.peek_back_node().unwrap();
            list.move_to_front(one.clone());

            let oneb = one.borrow();
            check_node_opt_eq(&oneb.prev, &None);
            check_node_opt_eq(&oneb.next, &Some(two.clone()));

            let twob = two.borrow();
            check_node_opt_eq(&twob.prev, &Some(one.clone()));
            check_node_opt_eq(&twob.next, &Some(three.clone()));

            let threeb = three.borrow();
            check_node_opt_eq(&threeb.prev, &Some(two.clone()));
            check_node_opt_eq(&threeb.next, &None);

            check_node_eq(&list.head.clone().unwrap(), &one.clone());
            check_node_eq(&list.tail.clone().unwrap(), &three.clone());
        }

        assert_eq!(list.len(), 3);
        let mut it = list.into_iter();
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), Some(2));
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn move_to_front_3b() {
        let mut list = List::new();
        list.push_back(1);
        list.push_back(2);
        list.push_back(3);

        /* scope */
        {
            let one = list.peek_front_node().unwrap();
            let two = one.borrow().next.clone().unwrap();
            let three = list.peek_back_node().unwrap();
            list.move_to_front(two.clone());

            let twob = two.borrow();
            check_node_opt_eq(&twob.prev, &None);
            check_node_opt_eq(&twob.next, &Some(one.clone()));

            let oneb = one.borrow();
            check_node_opt_eq(&oneb.prev, &Some(two.clone()));
            check_node_opt_eq(&oneb.next, &Some(three.clone()));

            let threeb = three.borrow();
            check_node_opt_eq(&threeb.prev, &Some(one.clone()));
            check_node_opt_eq(&threeb.next, &None);

            check_node_eq(&list.head.clone().unwrap(), &two.clone());
            check_node_eq(&list.tail.clone().unwrap(), &three.clone());
        }

        assert_eq!(list.len(), 3);
        let mut it = list.into_iter();
        assert_eq!(it.next(), Some(2));
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn move_to_front_3c() {
        let mut list = List::new();
        list.push_back(1);
        list.push_back(2);
        list.push_back(3);

        /* scope */
        {
            let one = list.peek_front_node().unwrap();
            let two = one.borrow().next.clone().unwrap();
            let three = list.peek_back_node().unwrap();
            list.move_to_front(three.clone());

            let threeb = three.borrow();
            check_node_opt_eq(&threeb.prev, &None);
            check_node_opt_eq(&threeb.next, &Some(one.clone()));

            let oneb = one.borrow();
            check_node_opt_eq(&oneb.prev, &Some(three.clone()));
            check_node_opt_eq(&oneb.next, &Some(two.clone()));

            let twob = two.borrow();
            check_node_opt_eq(&twob.prev, &Some(one.clone()));
            check_node_opt_eq(&twob.next, &None);

            check_node_eq(&list.head.clone().unwrap(), &three.clone());
            check_node_eq(&list.tail.clone().unwrap(), &two.clone());
        }

        assert_eq!(list.len(), 3);
        let mut it = list.into_iter();
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), Some(2));
        assert_eq!(it.next(), None);
    }
}
