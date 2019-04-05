//! Code for doing intervals
#![allow(dead_code)]

use std::mem;

use crate::{lisp::LispObject, remacs_sys::Qnil};

/*#[derive(Clone)]
enum Parent<'a> {
    Interval(&'a Interval<'a>),
    Object(LispObject),
}*/

#[derive(Clone)]
pub struct Interval {
    /// Left child interval.
    left: Option<Box<Interval>>,
    /// Right child interval.
    right: Option<Box<Interval>>,
    ///// The parent interval or LispObject containing this tree.
    //parent: Parent<'a>,
    parent: Option<LispObject>,

    node: Node,
}

#[derive(Clone)]
pub struct Node {
    /// Length of this interval and both children.
    pub total_length: usize,
    /// Cache of the interval's character position.
    pub position: usize,

    /// Whether modification is prevented.
    pub write_protect: bool,
    /// Whether the interval should be displayed.
    pub visible: bool,
    /// Whether text inserted just before the interval gets added to it.
    pub front_sticky: bool,
    /// Whether text inserted just after the interval gets added to it.
    pub rear_sticky: bool,

    /// Other properties.
    pub plist: LispObject,
}

impl Interval {
    pub fn has_left(&self) -> bool {
        self.left.is_some()
    }

    pub fn has_right(&self) -> bool {
        self.right.is_some()
    }

    pub fn has_child(&self) -> bool {
        self.has_left() || self.has_right()
    }

    pub fn has_children(&self) -> bool {
        self.has_left() || self.has_right()
    }

    pub fn length(&self) -> usize {
        self.node.total_length - self.left_total_length() - self.right_total_length()
    }

    pub fn left_mut<'a>(&'a mut self) -> Option<&'a mut Interval> {
        self.left.as_mut().map(|left| &mut **left)
    }

    pub fn right_mut<'a>(&'a mut self) -> Option<&'a mut Interval> {
        self.right.as_mut().map(|right| &mut **right)
    }

    /// Total length of the left child interval tree.
    pub fn left_total_length(&self) -> usize {
        self.left.as_ref().map_or(0, |left| left.node.total_length)
    }

    /// Total length of the right child interval tree.
    pub fn right_total_length(&self) -> usize {
        self.right
            .as_ref()
            .map_or(0, |right| right.node.total_length)
    }

    /// Find the (lexicographically) succeeding interval, i.e. the leftmost child
    /// of this interval's right child.
    ///
    /// Updates the `position` field based on that of self (see find_interval).
    pub fn next<'a>(&'a mut self) -> Option<&'a mut Interval> {
        let next_position = self.node.position + self.length();

        self.right_mut().take().map_or(None, |mut next| loop {
            if next.has_left() {
                next = next.left_mut().unwrap();
                continue;
            }
            next.node.position = next_position;
            break Some(next);
        })
    }

    /// Find the (lexicographically) preceding interval, i.e. the rightmost
    /// child of this interval's right child.
    ///
    /// Updates the `position` field based on that of self (see find_interval).
    pub fn prev<'a>(&'a mut self) -> Option<&'a mut Interval> {
        let position = self.node.position;

        self.left_mut().take().map_or(None, |mut prev| loop {
            if prev.has_right() {
                prev = prev.right_mut().unwrap();
                continue;
            }
            prev.node.position = position - prev.length();
            break Some(prev);
        })
    }

    /// Find the interval containing `position` in the tree. `position` is a
    /// buffer position (starting from 1) or a string index (starting from 0). If
    /// `position` is at the end of the buffer or string, return the interval
    /// containing the last character.
    ///
    /// The `position` field, which is a cache of an interval's position, is
    /// updated in the interval found. Other functions (e.g., next_interval) will
    /// update this cache based on the result of find_interval.
    pub fn find<'a>(&'a mut self, position: usize) -> &'a mut Interval {
        // The distance from the left edge of the subtree to position
        let mut relative_position = position;

        if let Some(buffer) = self.parent.map(|parent| parent.as_buffer()) {
            relative_position -= buffer.beg() as usize;
        }

        debug_assert!(relative_position <= self.node.total_length);

        self.balance_possible_root();

        let mut tree = self;
        loop {
            if relative_position < tree.left_total_length() {
                tree = tree.left_mut().unwrap();
            } else if tree.has_right()
                && relative_position >= tree.node.total_length - tree.right_total_length()
            {
                relative_position -= tree.node.total_length - tree.right_total_length();
                tree = tree.right_mut().unwrap();
            } else {
                tree.node.position = position - relative_position + tree.left_total_length();
                break tree;
            }
        }
    }

    /// If a right child exists, perform the following operation:
    ///```
    ///    A               B
    ///   / \	          / \
    ///  d   B    =>     A   e
    ///     / \         / \
    ///    c   e       d   c
    ///```
    pub fn rotate_left_owned(mut self) -> Interval {
        debug_assert!(self.length() > 0);

        let old_total = self.node.total_length;
        debug_assert!(old_total > 0);

        let mut b = match self.right {
            Some(right) => *right,
            None => return self,
        };
        debug_assert!(b.length() > 0);

        let c = b.left;

        // TODO: parent handling

        // Make A the parent of C.
        self.right = c;
        //c.set_parent(a)

        // Make B the parent of A.
        b.left = Some(Box::new(self));
        //a.set_parent(b)

        // A's total length is decreased by the length of B and the left child of A.
        let mut a = b.right.as_mut().unwrap();
        a.node.total_length -= b.node.total_length - a.right_total_length();
        debug_assert!(a.node.total_length > 0);
        debug_assert!(a.length() > 0);

        // B must have the some total length as A's original total length.
        b.node.total_length = old_total;
        debug_assert!(b.length() > 0);

        b
    }

    /// If a right child exists, perform the following operation:
    ///```
    ///    A               B
    ///   / \	          / \
    ///  d   B    =>     A   e
    ///     / \         / \
    ///    c   e       d   c
    ///```
    pub fn rotate_left(&mut self) {
        let old_total = self.node.total_length;
        debug_assert!(old_total > 0);
        debug_assert!(self.length() > 0);

        // Swap A and B's nodes.
        match self.right.as_mut() {
            Some(right) => mem::swap(&mut self.node, &mut right.node),
            None => return,
        }
        // Swap d and A.
        mem::swap(&mut self.left, &mut self.right);

        let a = self.left.as_mut().unwrap();
        // Swap d and e.
        mem::swap(&mut self.right, &mut a.right);
        // Swap d and c.
        mem::swap(&mut a.left, &mut a.right);

        // A's total length is decreased by the length of B and the right child of A.
        a.node.total_length -= self.node.total_length - a.right_total_length();
        debug_assert!(a.node.total_length > 0);
        debug_assert!(a.length() > 0);

        // B must have the some total length as A's original total length.
        self.node.total_length = old_total;
        debug_assert!(self.length() > 0);
    }

    /// If a left child exists, perform the following operation:
    ///```
    ///     A		  B
    ///    / \		 / \
    ///   B       =>        A
    ///  / \		   / \
    ///     c		  c
    ///```
    ///
    /// Returns an error with the original value if left child isn't present.
    pub fn rotate_right_owned(mut self) -> Interval {
        debug_assert!(self.length() > 0);

        let old_total = self.node.total_length;
        debug_assert!(old_total > 0);

        let mut b = match self.left {
            Some(left) => *left,
            None => return self,
        };
        debug_assert!(b.length() > 0);

        let c = b.right;

        // TODO: parent handling

        // Make A the parent of C
        self.left = c;
        //c.set_parent(a)

        // Make B the parent of A.
        b.right = Some(Box::new(self));
        //a.set_parent(b)

        // A's total length is decreased by the length of B and the left child of A.
        let mut a = b.right.as_mut().unwrap();
        a.node.total_length -= b.node.total_length - a.left_total_length();
        debug_assert!(a.node.total_length > 0);
        debug_assert!(a.length() > 0);

        // B must have the some total length as A's original total length.
        b.node.total_length = old_total;
        debug_assert!(b.length() > 0);

        b
    }

    /// If a left child exists, perform the following operation:
    ///```
    ///     A		  B
    ///    / \		 / \
    ///   B       =>        A
    ///  / \		   / \
    ///     c		  c
    ///```
    pub fn rotate_right(&mut self) {
        let old_total = self.node.total_length;
        debug_assert!(old_total > 0);
        debug_assert!(self.length() > 0);

        // Swap a and b's nodes.
        match self.left.as_mut() {
            Some(left) => mem::swap(&mut self.node, &mut left.node),
            None => return,
        }
        // Swap d and A
        mem::swap(&mut self.left, &mut self.right);

        let a = self.right.as_mut().unwrap();
        // Swap d and e.
        mem::swap(&mut self.left, &mut a.left);
        // Swap d and c.
        mem::swap(&mut a.left, &mut a.right);

        // A's total length is decreased by the length of B and A's left child.
        a.node.total_length -= self.node.total_length - a.left_total_length();
        debug_assert!(a.node.total_length > 0);
        debug_assert!(a.length() > 0);

        // b must have the same total length of A.
        self.node.total_length = old_total;
        debug_assert!(self.length() > 0);
    }

    /// Balance an interval tree with the assumptino that the subtrees themselves
    /// are already balanced.
    fn balance_self(&mut self) {
        debug_assert!(self.length() > 0);
        debug_assert!(self.node.total_length >= self.length());

        loop {
            let old_diff = self.left_total_length() as isize - self.right_total_length() as isize;

            if old_diff > 0 {
                // Since the left child is longer, there must be one.
                let left = self.left.as_ref().unwrap();
                let new_diff = self.node.total_length as isize - left.node.total_length as isize
                    + left.right_total_length() as isize
                    - left.left_total_length() as isize;

                if new_diff.abs() >= -old_diff {
                    break;
                }
                self.rotate_right();
                self.right.as_mut().map(|right| right.balance_self());
            } else if old_diff < 0 {
                // Must exist
                let right = self.right.as_ref().unwrap();
                let new_diff = self.node.total_length as isize - right.node.total_length as isize
                    + right.left_total_length() as isize
                    - right.right_total_length() as isize;

                if new_diff.abs() >= -old_diff {
                    break;
                }
                self.rotate_left();
                self.left.as_mut().map(|left| left.balance_self());
            } else {
                break;
            }
        }
    }

    pub fn balance(&mut self) {
        self.left.as_mut().map(|left| left.balance());
        self.right.as_mut().map(|right| right.balance());
        self.balance_self();
    }

    /// Balance the interval, potentially putting it back into its parent
    /// `LispObject`.
    pub fn balance_possible_root(&mut self) {
        if let Some(parent) = self.parent {
            self.balance_self();
            if let Some(buffer) = parent.as_buffer() {
                //buffer.set_intervals(&mut self)
            } else if let Some(string) = parent.as_string() {
                //string.set_intervals(&mut self)
            }
        }
    }

    /// Reset the interval to its default no-property state
    pub fn reset(&mut self) {
        *self = Interval {
            left: None,
            right: None,
            node: Node {
                total_length: 0,
                position: 0,
                plist: Qnil,
                ..self.node
            },
            ..*self
        }
    }
}

pub struct Iter<'a> {
    stack: Vec<&'a Interval>,
}

pub struct IterMut<'a> {
    stack: Vec<&'a mut Interval>,
}

impl Interval {
    pub fn iter(&self) -> Iter {
        Iter { stack: vec![self] }
    }

    pub fn iter_mut(&mut self) -> IterMut {
        IterMut { stack: vec![self] }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Interval;

    fn next(&mut self) -> Option<Self::Item> {
        self.stack.pop().map(|tree| {
            tree.right.as_ref().map(|r| self.stack.push(r));
            tree.left.as_ref().map(|l| self.stack.push(l));
            tree
        })
    }
}

impl<'a> Iterator for IterMut<'a> {
    type Item = &'a mut Node;

    fn next(&mut self) -> Option<Self::Item> {
        self.stack.pop().take().map(|tree| {
            tree.right.as_mut().map(|r| self.stack.push(r));
            tree.left.as_mut().map(|l| self.stack.push(l));
            &mut tree.node
        })
    }
}
