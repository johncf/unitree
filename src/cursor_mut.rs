use super::*;
use std::fmt;

// Note: The working of `CursorMut` is fundamentally different from `Cursor`. `CursorMut` can
//       become empty (iff `cur_node` is empty. `cur_node` empty implies `steps` is also empty).

/// A useful type alias for easy initialization of `CursorMutT`.
pub type CursorMut<L> = CursorMutT<L, ()>;

/// A useful type alias for easy initialization of `CursorMutT`.
pub type CursorMutGather<L: Leaf> = CursorMutT<L, L::Info>;

/// A cursor object that can be used to modify internals of `Node` while maintaining balance.
///
/// `CursorMutT` is heavier compared to `CursorT`. Even though `CursorMutT` does not do any heap
/// allocations for its own operations, most operations tries to make the current node writable
/// using `Arc::make_mut`. This could result in a heap allocation if the number of references to
/// that node is more than one.
pub struct CursorMutT<L: Leaf, I> {
    cur_node: Option<Node<L>>,
    steps: CVec<CursorMutStep<L, I>>,
}

struct CursorMutStep<L: Leaf, I> {
    nodes: RC<NVec<Node<L>>>,
    idx: usize,
    extra: I,
}

impl<L, I> fmt::Debug for CursorMutStep<L, I> where L: Leaf, I: InfoExt<L::Info> + fmt::Debug {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CursorMutStep {{ nodes.len: {}, idx: {}, extra: {:?} }}",
                  self.nodes.len(), self.idx, self.extra)
    }
}

impl<L, I> CursorMutT<L, I> where L: Leaf, I: InfoExt<L::Info> {
    pub fn new() -> CursorMutT<L, I> {
        CursorMutT {
            cur_node: None,
            steps: CVec::new(),
        }
    }

    pub fn from_node(node: Node<L>) -> CursorMutT<L, I> {
        CursorMutT {
            cur_node: Some(node),
            steps: CVec::new(),
        }
    }

    pub fn into_root(mut self) -> Option<Node<L>> {
        self.reset();
        self.cur_node.take()
    }

    pub fn current(&mut self) -> Option<&mut Node<L>> {
        self.cur_node.as_mut()
    }

    /// TODO
    pub fn extra(&self) -> I {
        match self.steps.last() {
            Some(cstep) => cstep.extra,
            None => I::identity(),
        }
    }

    pub fn reset(&mut self) {
        while let Some(_) = self.ascend() {}
    }

    pub fn ascend(&mut self) -> Option<&mut Node<L>> {
        match self.cur_node.take() {
            Some(cur_node) => match self.steps.pop() {
                Some(CursorMutStep { mut nodes, idx, .. }) => {
                    RC::make_mut(&mut nodes).insert(idx, cur_node);
                    let parent = Node::from_nodes(nodes); // compute cumulative info
                    self.cur_node = Some(parent);
                    self.cur_node.as_mut()
                }
                None => { // cur_node is the root
                    self.cur_node = Some(cur_node);
                    None
                }
            },
            None => None, // cursor is empty
        }
    }

    pub fn descend(&mut self, idx: usize) -> Option<&mut Node<L>> {
        self.descend_by_ext(|_, _, i, _| i == idx, false)
    }

    pub fn descend_last(&mut self, idx: usize) -> Option<&mut Node<L>> {
        self.descend_by_ext(|_, _, _, i| i == idx, true)
    }

    /// Descend the tree once, on the child for which `f` returns `true`.
    ///
    /// Returns `None` if `f` returned `false` on all children, or if it was a leaf node.
    ///
    /// The arguments to `f` are exactly the same as in [`Node::traverse`].
    ///
    /// Panics if tree depth is greater than 8.
    ///
    /// [`Node::traverse`]: ../enum.Node.html#method.traverse
    pub fn descend_by<F>(&mut self, mut f: F, reversed: bool) -> Option<&mut Node<L>>
        where F: FnMut(L::Info, usize, usize) -> bool
    {
        self.descend_by_ext(|_, a, i, j| f(a, i, j), reversed)
    }

    /// TODO
    pub fn descend_by_ext<F>(&mut self, f: F, reversed: bool) -> Option<&mut Node<L>>
        where F: FnMut(I, L::Info, usize, usize) -> bool
    {
        match self.cur_node.take() {
            Some(cur_node) => {
                let res = if reversed {
                    cur_node.gather_traverse_rev(self.extra(), f)
                } else {
                    cur_node.gather_traverse(self.extra(), f)
                }.map(|(index, extra, _)| (index, extra));

                match res {
                    Ok((index, extra)) => {
                        self.descend_raw(cur_node.into_children_raw(), index, extra);
                        self.cur_node.as_mut()
                    }
                    Err(_) => {
                        self.cur_node = Some(cur_node);
                        None
                    },
                }
            }
            None => None, // empty cursor
        }
    }

    /// Insert a leaf at the current position if currently focused on a leaf, or as the first leaf
    /// under the current node.
    ///
    /// It is unspecified where the cursor will be after this operation. The user should ensure
    /// that the cursor is at the correct location after this.
    ///
    /// _Side node:_ As per the current implementation, the cursor will either be pointing to the
    /// newly inserted node, or be to an ancestor of the new node, or to one directly on the right
    /// of an ancestor node. But this behavior may change in future.
    pub fn insert(&mut self, leaf: L) {
        while let Some(_) = self.descend(0) {}
        self.insert_raw(Node::from_leaf(leaf), false);
    }

    /// Same as `insert` but insert after the current node (incl. all its leaf children).
    pub fn insert_after(&mut self, leaf: L) {
        while let Some(_) = self.descend_last(0) {}
        self.insert_raw(Node::from_leaf(leaf), true);
    }

    /// Remove the current node and return it. If the cursor is empty, return `None`.
    ///
    /// It is unspecified where the cursor will be after this operation. The user should ensure
    /// that the cursor is at the correct location after this.
    ///
    /// Panics if called on a leaf node.
    pub fn remove(&mut self, _idx: usize) -> Option<Node<L>> {
        // the cursor should point to the same position if possible, or if there are no children on
        // the right to replace it, move left, or if it underflows, move up and merge with an
        // adjacent node (the return value should reflect any changes in position. even though the
        // exact changes need not be conveyed, the cursor should be guaranteed to be in the
        // ancestor line)
        unimplemented!()
    }

    /// Split the tree into two, and return the right part of it. The current node, all leaves
    /// under it, as well as all leaves to the right of it will be included in the returned tree.
    pub fn split_off(&mut self) -> Node<L> {
        unimplemented!()
    }
}

fn insert_maybe_split<L: Leaf>(
    nodes: &mut NVec<Node<L>>,
    idx: usize,
    newnode: Node<L>
) -> Option<Node<L>> {
    if nodes.len() < MAX_CHILDREN {
        let res = nodes.insert(idx, newnode);
        debug_assert!(res.is_none());
        None
    } else {
        let extra = nodes.insert(idx, newnode).unwrap(); // like unwrap_err
        let mut after: NVec<_> = nodes.drain(MIN_CHILDREN+1..).collect();
        let res = after.push(extra);
        debug_assert!(res.is_none());
        Some(Node::from_nodes(RC::new(after)))
    }
}

impl<L, I> CursorMutT<L, I> where L: Leaf, I: InfoExt<L::Info> {
    fn insert_raw(&mut self, newnode: Node<L>, after: bool) {
        match self.cur_node.take() {
            Some(cur_node) => {
                assert_eq!(cur_node.height(), newnode.height());
                match self.steps.pop() {
                    Some(mut cstep) => {
                        // TODO cursor can actually hold one more than MAX_CHILDREN in current node
                        let _res = RC::make_mut(&mut cstep.nodes).insert(cstep.idx, cur_node);
                        debug_assert!(_res.is_none());
                        let newidx = if after { cstep.idx + 1 } else { cstep.idx };
                        let maybe_split = insert_maybe_split(RC::make_mut(&mut cstep.nodes), newidx, newnode);
                        if let Some(split_node) = maybe_split {
                            let parent = Node::from_nodes(cstep.nodes); // compute cumulative info
                            self.cur_node = Some(parent);
                            self.insert_raw(split_node, true);
                        } else {
                            // FIXME optimize this using above TODO
                            let newnode = RC::make_mut(&mut cstep.nodes).remove(newidx);
                            debug_assert!(newnode.is_some());
                            self.cur_node = newnode;
                            cstep.idx = newidx;
                            self.steps.push(cstep);
                        }
                    }
                    None => { // cur_node is the root
                        self.cur_node = Some(if after {
                            Node::concat(cur_node, newnode)
                        } else {
                            Node::concat(newnode, cur_node)
                        });
                    }
                }
            }
            None => { // cursor was empty
                self.cur_node = Some(newnode);
            }
        }
    }

    fn descend_raw(&mut self, mut nodes: RC<NVec<Node<L>>>, idx: usize, extra: I) {
        debug_assert!(self.cur_node.is_none());
        let cur_node = RC::make_mut(&mut nodes).remove(idx).unwrap();
        self.cur_node = Some(cur_node);
        let _res = self.steps.push(CursorMutStep { nodes, idx, extra });
        assert!(_res.is_none());
    }
}

impl<L, I> FromIterator<L> for CursorMutT<L, I> where L: Leaf, I: InfoExt<L::Info> {
    fn from_iter<J: IntoIterator<Item=L>>(iter: J) -> Self {
        let mut curs = CursorMutT::new();
        let mut iter = iter.into_iter().map(|e| Node::from_leaf(e));

        loop {
            loop {
                match curs.current().map(|node| node.height()) {
                    Some(h) if h > 1 => { curs.descend_last(0); }
                    _ => break,
                }
            }
            let nodes: NVec<_> = iter.by_ref().take(MAX_CHILDREN).collect();
            if nodes.len() > 0 {
                curs.insert_raw((Node::from_nodes(RC::new(nodes))), true);
            } else {
                break;
            }
        }
        curs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::*;

    #[test]
    fn insert() {
        let mut cursor_mut = CursorMut::new();
        for i in 0..128 {
            cursor_mut.insert_after(TestLeaf(i));
        }
        let root = cursor_mut.into_root().unwrap();
        let mut cursor = Cursor::new(&root);
        for i in 0..128 {
            assert_eq!(cursor.next_leaf(), Some(&TestLeaf(i)));
        }
        assert_eq!(cursor.next_leaf(), None);
    }

    // FIXME need more tests
}
