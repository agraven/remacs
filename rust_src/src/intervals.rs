//! Code for doing intervals
#![allow(dead_code)]

use std::mem;

use crate::{fns, lisp::LispObject, remacs_sys::Qnil};

#[derive(Copy, Clone)]
pub enum Parent {
    /// The interval has a parent interval
    Interval(*mut Interval),
    /// The interval belongs to an object
    Object(LispObject),
}

#[derive(Clone)]
pub struct Interval {
    /// Left child interval.
    left: Option<Box<Interval>>,
    /// Right child interval.
    right: Option<Box<Interval>>,
    /// The parent interval or LispObject containing this tree.
    parent: Parent,

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

impl Node {
    fn new() -> Node {
        Node {
            total_length: 0,
            position: 0,
            write_protect: false,
            visible: false,
            front_sticky: false,
            rear_sticky: false,
            plist: Qnil,
        }
    }
}

impl Interval {
    pub fn new(parent: Parent) -> Interval {
        Interval {
            left: None,
            right: None,
            parent,
            node: Node::new(),
        }
    }

    /// Whether the interval has a left child
    pub fn has_left(&self) -> bool {
        self.left.is_some()
    }

    /// Whether the interval has a right child
    pub fn has_right(&self) -> bool {
        self.right.is_some()
    }

    /// Whether the interval has any child
    pub fn has_child(&self) -> bool {
        self.has_left() || self.has_right()
    }

    /// Whether the interval has both children
    pub fn has_children(&self) -> bool {
        self.has_left() && self.has_right()
    }

    /// Whether the interval has a parent interval
    pub fn has_parent(&self) -> bool {
        match self.parent {
            Parent::Interval(_) => true,
            Parent::Object(_) => false,
        }
    }

    /// Whether the interval belongs to an object
    pub fn has_object(&self) -> bool {
        match self.parent {
            Parent::Interval(_) => false,
            Parent::Object(_) => true,
        }
    }

    /// Whether the interval's property list is empty
    pub fn is_default(&self) -> bool {
        self.node.plist.is_nil()
    }

    pub fn is_left_child(&self) -> bool {
        match self.parent {
            Parent::Object(_) => false,
            Parent::Interval(parent) => unsafe { &*parent }.left().map_or(false, |left| {
                left as *const Interval == self as *const Interval
            }),
        }
    }

    pub fn is_right_child(&self) -> bool {
        match self.parent {
            Parent::Object(_) => false,
            Parent::Interval(parent) => unsafe { &*parent }.right().map_or(false, |right| {
                right as *const Interval == self as *const Interval
            }),
        }
    }

    pub fn length(&self) -> usize {
        self.node.total_length - self.left_total_length() - self.right_total_length()
    }

    pub fn left<'a>(&'a self) -> Option<&'a Interval> {
        self.left.as_ref().map(|left| &**left)
    }

    pub fn right<'a>(&'a self) -> Option<&'a Interval> {
        self.right.as_ref().map(|right| &**right)
    }

    /// Get a reference to the parent interval of this interval
    pub fn parent<'a>(&'a self) -> Option<&'a Interval> {
        match self.parent {
            Parent::Interval(parent) => unsafe { Some(&*parent) },
            Parent::Object(_) => None,
        }
    }

    /// Get a mutable reference to the parent interval of this interval
    pub fn parent_mut<'a>(&'a mut self) -> Option<&'a mut Interval> {
        match self.parent {
            Parent::Interval(parent) => unsafe { Some(&mut *parent) },
            Parent::Object(_) => None,
        }
    }

    /// Get the lisp object that owns this interval
    pub fn object(&self) -> Option<LispObject> {
        match self.parent {
            Parent::Object(obj) => Some(obj),
            Parent::Interval(_) => None,
        }
    }

    /// Get a mutable reference to the interval's left child
    pub fn left_mut<'a>(&'a mut self) -> Option<&'a mut Interval> {
        self.left.as_mut().map(|left| &mut **left)
    }

    /// Get a mutable reference to the interval's right child
    pub fn right_mut<'a>(&'a mut self) -> Option<&'a mut Interval> {
        self.right.as_mut().map(|right| &mut **right)
    }

    /// Take the interval's left child, leaving None its place
    pub fn take_left(&mut self) -> Option<Interval> {
        self.left.take().map(|boxed| *boxed)
    }

    pub fn take_right(&mut self) -> Option<Interval> {
        self.right.take().map(|boxed| *boxed)
    }

    /// Set the interval's left child
    pub fn set_left(&mut self, mut left: Interval) {
        left.set_parent(self);
        self.left = Some(Box::new(left));
    }

    /// Set the interval's right child
    pub fn set_right(&mut self, mut right: Interval) {
        right.set_parent(self);
        self.right = Some(Box::new(right));
    }

    /// Set the parent interval of this interval
    fn set_parent(&mut self, parent: *mut Interval) {
        self.parent = Parent::Interval(parent)
    }

    /// Set the object this interval belongs to
    fn set_object(&mut self, object: LispObject) {
        self.parent = Parent::Object(object)
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

    /// Return the proper position for the first character described by the
    /// interval tree. Returns 1 if the parent is a buffer, and 0 if the
    /// parent is a string or none
    fn start_pos(&self) -> usize {
        match self.parent {
            Parent::Interval(_) => 0,
            Parent::Object(parent) => match parent.as_buffer() {
                Some(buffer) => buffer.beg() as usize,
                None => 0,
            },
        }
    }

    /// Find the (lexicographically) succeeding interval, i.e. either the leftmost child
    /// of this interval's right child or .
    ///
    /// Updates the `position` field based on that of self (see find_interval).
    pub fn next<'a>(&'a mut self) -> Option<&'a mut Interval> {
        let next_position = self.node.position + self.length();

        self.right_mut().map(|mut next| loop {
            if next.has_left() {
                next = next.left_mut().unwrap();
                continue;
            }
            next.node.position = next_position;
            return Some(next);
        });

        // Iterate parents until first left child found
        let mut i = self;
        loop {
            if i.is_left_child() {
                let parent = i.parent_mut().unwrap();
                parent.node.position = next_position;
                return Some(parent);
            }
            match i.parent_mut() {
                Some(parent) => i = parent,
                None => break,
            }
        }
        None
    }

    /// Find the (lexicographically) preceding interval, i.e. the rightmost
    /// child of this interval's left child.
    ///
    /// Updates the `position` field based on that of self (see find_interval).
    pub fn prev<'a>(&'a mut self) -> Option<&'a mut Interval> {
        let position = self.node.position;

        // Get rightmost child of left child
        self.left_mut().map(|mut prev| loop {
            if prev.has_right() {
                prev = prev.right_mut().unwrap();
                continue;
            }
            prev.node.position = position - prev.length();
            return Some(prev);
        });

        // Iterate parents until right child found
        let mut i = self;
        loop {
            if i.is_right_child() {
                let parent = i.parent_mut().unwrap();
                parent.node.position = position - parent.length();
                break Some(parent);
            }
            match i.parent_mut() {
                Some(parent) => i = parent,
                None => break None,
            }
        }
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

        if let Some(buffer) = self.object().and_then(LispObject::as_buffer) {
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
    ///      B    =>     A
    ///     / \         / \
    ///    c               c
    ///```
    /// If the interval is the child of another interval, the caller must
    /// reinsert the rotated tree back into the same child node.
    pub fn rotate_left_owned(mut self) -> Interval {
        let old_total = self.node.total_length;
        debug_assert!(old_total > 0);
        debug_assert!(self.length() > 0);

        let mut b = match self.take_right() {
            Some(right) => right,
            None => return self,
        };
        let c = b.take_left();
        debug_assert!(b.length() > 0);

        let parent = self.parent;

        // Make A the parent of C.
        if let Some(c) = c {
            self.set_right(c)
        }

        // Make B the parent of A.
        b.set_left(self);
        let mut a = b.left.as_mut().unwrap();

        // A's total length is decreased by the length of B and the left child of A.
        a.node.total_length -= b.node.total_length - a.right_total_length();
        debug_assert!(a.node.total_length > 0);
        debug_assert!(a.length() > 0);

        // B must have the some total length as A's original total length.
        b.node.total_length = old_total;
        debug_assert!(b.length() > 0);

        // Make the parent of A point to B (parent interval's child is not
        // altered, as we have ownership of it and assume it's temporarily
        // taken, and will thus be reinserted after the operation).
        match parent {
            Parent::Interval(parent) => b.set_parent(parent),
            Parent::Object(obj) => {
                if let Some(_buffer) = obj.as_buffer() {
                    //buffer.set_intervals(&mut b)
                } else if let Some(_string) = obj.as_string() {
                    //string.set_intervals(&mut b)
                }
            }
        }
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
        let self_ptr: *mut Interval = self;
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
        let a_ptr: *mut Interval = &mut **a;
        // Swap d and e.
        mem::swap(&mut self.right, &mut a.right);
        // Update d and e's parents
        self.right.as_mut().map(|right| right.set_parent(self_ptr));
        a.right.as_mut().map(|right| right.set_parent(a_ptr));
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
        let old_total = self.node.total_length;
        debug_assert!(old_total > 0);
        debug_assert!(self.length() > 0);

        let mut b = match self.take_left() {
            Some(left) => left,
            None => return self,
        };
        let c = b.take_right();
        debug_assert!(b.length() > 0);

        let parent = self.parent;

        // Make A the parent of C
        if let Some(c) = c {
            self.set_left(c);
        }

        // Make B the parent of A.
        b.set_right(self);
        let mut a = b.right.as_mut().unwrap();

        // A's total length is decreased by the length of B and the left child of A.
        a.node.total_length -= b.node.total_length - a.left_total_length();
        debug_assert!(a.node.total_length > 0);
        debug_assert!(a.length() > 0);

        // B must have the some total length as A's original total length.
        b.node.total_length = old_total;
        debug_assert!(b.length() > 0);

        // Make the parent of A point to B (parent interval's child is not
        // altered, as we have ownership of it and assume it's temporarily
        // taken, and will thus be reinserted after the operation).
        match parent {
            Parent::Interval(parent) => b.set_parent(parent),
            Parent::Object(obj) => {
                if let Some(_buffer) = obj.as_buffer() {
                    //buffer.set_intervals(&mut b)
                } else if let Some(_string) = obj.as_string() {
                    //string.set_intervals(&mut b)
                }
            }
        }
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
        let self_ptr: *mut Interval = self;
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
        let a_ptr: *mut Interval = &mut **a;
        // Swap d and e.
        mem::swap(&mut self.left, &mut a.left);
        // Update d and e's parents
        self.left.as_mut().map(|left| left.set_parent(self_ptr));
        a.left.as_mut().map(|left| left.set_parent(a_ptr));
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

    /// Balance the interval tree with the assumption that the subtrees
    /// themselves are already balanced.
    pub fn balance(&mut self) {
        self.left.as_mut().map(|left| left.balance());
        self.right.as_mut().map(|right| right.balance());
        self.balance_self();
    }

    /// Balance the interval, potentially putting it back into its parent
    /// `LispObject`.
    pub fn balance_possible_root(&mut self) {
        if let Some(parent) = self.object() {
            self.balance_self();
            if let Some(_buffer) = parent.as_buffer() {
                //buffer.set_intervals(&mut self)
            } else if let Some(_string) = parent.as_string() {
                //string.set_intervals(&mut self)
            }
        }
    }

    /// Split the interval into two pieces, starting the second piece at
    /// the character position `offset`, counting from 0, relative the
    /// interval's position. The new left-hand piece (first lexicographically)
    /// is returned.
    ///
    /// The size and position fields of the two intervals are set based on the
    /// ones of the original interval. The property list of the new interval is
    /// reset, so it's up to the caller to modify the returned value
    /// appropriately.
    ///
    /// The position of the interval is not changed, if it's a root, it stays a
    /// root after the operation.
    pub fn split_left<'a>(&'a mut self, offset: usize) -> &'a mut Interval {
        let mut new = Interval::new(Parent::Interval(self));
        let new_length = offset;

        new.node.position = self.node.position;
        self.node.position += offset;

        match self.take_left() {
            None => {
                new.node.total_length = new_length;
                assert!(new.length() > 0)
            }
            Some(mut left) => {
                // Insert the new node between self and its left child
                new.node.total_length = new_length + left.node.total_length;
                left.set_parent(&mut new);
                new.set_left(left);
                new.balance_self();
            }
        }
        self.set_left(new);
        self.balance_possible_root();

        self.left_mut().unwrap()
    }

    /// Split the interval into two pieces, starting the second piece at
    /// the character position `offset`, counting from 0, relative the
    /// interval's position. The new right-hand piece (second lexicographically)
    /// is returned.
    ///
    /// The size and position fields of the two intervals are set based on the
    /// ones of the original interval. The property list of the new interval is
    /// reset, so it's up to the caller to modify the returned value
    /// appropriately.
    ///
    /// The position of the interval is not changed, if it's a root, it stays a
    /// root after the operation.
    pub fn split_right<'a>(&'a mut self, offset: usize) -> &'a mut Interval {
        let mut new = Interval::new(Parent::Interval(self));
        let position = self.node.position;
        let new_length = self.length() - offset;

        new.node.position = position + offset;

        match self.take_right() {
            None => {
                new.node.total_length = new_length;
                assert!(new.length() > 0);
            }
            Some(mut right) => {
                // Insert the new node between self and its right child.
                let right_length = right.node.total_length;
                right.set_parent(&mut new);
                new.set_right(right);
                new.node.total_length = new_length + right_length;
                new.balance_self();
            }
        }
        self.set_right(new);
        self.balance_possible_root();

        self.right_mut().unwrap()
    }

    /// Make the interval have exactly the properties of `source`.
    pub fn copy_properties(&mut self, source: &Interval) {
        if self.is_default() && source.is_default() {
            return;
        }
        self.node.write_protect = source.node.write_protect;
        self.node.visible = source.node.visible;
        self.node.front_sticky = source.node.front_sticky;
        self.node.rear_sticky = source.node.rear_sticky;
        self.node.plist = fns::copy_sequence(source.node.plist);
    }

    /// Reset the interval to its default no-property state
    pub fn reset(&mut self) {
        self.left = None;
        self.right = None;
        self.node = Node {
            total_length: 0,
            position: 0,
            plist: Qnil,
            ..self.node
        };
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

#[cfg(test)]
mod tests {
    use super::{Interval, Parent};
    use crate::remacs_sys::Qnil;

    #[test]
    fn is_child() {
        let mut parent = Interval::new(Parent::Object(Qnil));
        let parent_ptr: *mut Interval = &mut parent;
        let child = Interval::new(Parent::Interval(parent_ptr));
        parent.set_right(child);
        assert!(parent.right().unwrap().is_right_child());
        assert!(!parent.right().unwrap().is_left_child());
    }
}
