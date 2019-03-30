//! Code for doing intervals

use std::ptr;

use crate::{
    lisp::{ExternalPtr, LispObject},
    remacs_sys::Lisp_Interval,
    remacs_sys::Qnil,
    remacs_sys::{make_interval, CHECK_IMPURE},
};

pub type IntervalRef = ExternalPtr<Lisp_Interval>;

impl IntervalRef {
    /// Create the root interval of a buffer or string object.
    pub fn create_root(parent: LispObject) -> Self {
        let mut new = unsafe { ExternalPtr::new(make_interval()) };
        if let Some(mut buffer) = parent.as_buffer() {
            new.total_length = buffer.z() - buffer.beg();
            assert!(new.total_length >= 0);
            buffer.set_intervals(new);
            new.position = buffer.beg();
        } else {
            let mut string = parent.force_string();
            unsafe { CHECK_IMPURE(parent, string.as_mut() as *mut libc::c_void) };
            new.total_length = string.len_chars();
            assert!(new.total_length >= 0);
            string.set_intervals(new);
            new.position = 0;
        }
        assert!(new.length() > 0);

        new.set_object(parent);
        new
    }

    /// Checks if the interval is a default one, which is true if it's null or
    /// has no properties.
    pub fn is_default(self) -> bool {
        self.is_null() || self.plist == Qnil
    }

    /// Checks if the interval is the left child of another interval.
    pub fn is_left_child(self) -> bool {
        match self.get_parent() {
            Some(parent) => parent.left == self.as_ptr() as *mut Lisp_Interval,
            None => false,
        }
    }

    /// Checks if the interval is the right child of another interval.
    pub fn is_right_child(self) -> bool {
        match self.get_parent() {
            Some(parent) => parent.right == self.as_ptr() as *mut Lisp_Interval,
            None => false,
        }
    }

    /// Checks if the interval is the only one in its tree.
    pub fn is_only(self) -> bool {
        !(self.has_parent() || self.has_children())
    }

    /// Checks if the interval has a (non-null) parent.
    pub fn has_parent(self) -> bool {
        !self.up_obj() && unsafe { !self.up.interval.is_null() }
    }

    /// Checks if the interval's left child is non-null.
    pub fn has_left_child(self) -> bool {
        !self.left.is_null()
    }

    /// Checks if the interval's right child is non-null.
    pub fn has_right_child(self) -> bool {
        !self.right.is_null()
    }

    /// Checks if the interval has any children.
    pub fn has_children(self) -> bool {
        self.has_left_child() || self.has_right_child()
    }

    /// Checks if the interval has both children.
    pub fn has_both_children(self) -> bool {
        self.has_left_child() && self.has_right_child()
    }

    /// Get's the object containing the interval if there is one.
    pub fn get_object(self) -> Option<LispObject> {
        match self.up_obj() {
            false => None,
            true => unsafe { Some(self.up.obj) },
        }
    }

    /// Gets the interval's parent if it has one.
    pub fn get_parent(self) -> Option<Self> {
        match self.up_obj() {
            false => unsafe { Self::from_ptr(self.up.interval as *mut libc::c_void) },
            true => None,
        }
    }

    /// Get the interval's left child if it has one.
    pub fn get_left(self) -> Option<Self> {
        Self::from_ptr(self.left as *mut libc::c_void)
    }

    /// Get the interval's right child if it has one.
    pub fn get_right(self) -> Option<Self> {
        Self::from_ptr(self.right as *mut libc::c_void)
    }

    pub fn get_left_raw(self) -> Self {
        Self::new(self.left)
    }

    pub fn get_right_raw(self) -> Self {
        Self::new(self.right)
    }

    /// The size of text represented by this interval alone.
    pub fn length(self) -> isize {
        self.total_length - self.left_total_length() - self.right_total_length()
    }

    pub fn left_total_length(self) -> isize {
        self.get_left().map_or(0, |left| left.total_length)
    }

    pub fn right_total_length(self) -> isize {
        self.get_right().map_or(0, |right| right.total_length)
    }

    /// Check if two intervals have the same properties
    /*pub fn equal(other: IntervalRef) -> bool {
        if self.is_default() && other.is_default() {
            return true;
        } else if self.is_default() || other.is_default() {
            return false;
        }
        // The is_default checks guarantees the plists are not nil
        let plist1 = self.plist.force_cons();
        let plist2 = other.plist.force_cons();

        plist1
            .iter_cars()
            .all(|sym1| plist2.iter_cars().any(|sym2| val == sym))
    }*/

    /// Find the interval containing text position POSITION in the text
    /// represented by the interval tree TREE.  POSITION is a buffer position
    /// (starting from 1) or a string index (starting from 0).
    ///
    /// If `position` is at the end of the buffer or string, return the interval
    /// containing the last character.
    ///
    /// The `position' field, which is a cache of an interval's position, is
    /// updated in the interval found. Other functions (e.g., next_interval) will
    /// update this cache based on the result of find_interval.
    pub fn find(self, position: isize) -> Self {
        let mut relative_position = position;

        if let Some(parent) = self.get_object() {
            if let Some(buffer) = parent.as_buffer() {
                relative_position -= buffer.beg()
            }
        }

        assert!(relative_position <= self.total_length);
        unimplemented!()
    }

    pub fn iter(self) -> DepthIter {
        DepthIter {
            stack: if !self.is_null() {
                vec![self]
            } else {
                Vec::new()
            },
        }
    }

    /// Make the parent of `other` whatever the parent of `self` is, regardless
    /// of the type.
    fn copy_parent_to(self, other: &mut Self) {
        other.set_up_obj(self.up_obj());
        if self.up_obj() {
            other.set_object(unsafe { self.up.obj });
        } else {
            other.set_parent(ExternalPtr::new(unsafe { self.up.interval }));
        }
    }

    pub fn set_object(&mut self, obj: LispObject) {
        self.set_up_obj(true);
        self.up.obj = obj;
    }

    pub fn set_parent(&mut self, parent: Self) {
        self.set_up_obj(false);
        self.up.interval = parent.as_ptr() as *mut Lisp_Interval;
    }

    /// Assuming that a left child exists, perform the following operation:
    ///```
    ///     A		  B
    ///    / \		 / \
    ///   B       =>        A
    ///  / \		   / \
    ///     c		  c
    ///```
    pub fn rotate_right(&mut self) {
        let a = self;
        let mut b = a.get_left_raw();
        let mut c = ExternalPtr::new(b.right);
        let old_total = a.total_length;

        assert!(old_total > 0);
        assert!(a.length() > 0);
        assert!(b.length() > 0);

        // Deal with any parent of A, make it point to B.
        if let Some(mut parent) = a.get_parent() {
            if a.is_left_child() {
                parent.left = b.as_mut();
            } else {
                parent.right = b.as_mut();
            }
        }
        a.copy_parent_to(&mut b);

        // Make B the parent of A.
        b.right = a.as_mut();
        a.set_parent(b);

        // Make A point to c.
        a.left = c.as_mut();
        if !c.is_null() {
            c.set_parent(*a);
        }

        // A's total length is decreased by the length of B and the left child of A.
        a.total_length -= b.total_length - if c.is_null() { 0 } else { c.total_length };
        assert!(a.total_length > 0);
        assert!(a.length() > 0);

        // B must have the same total length of A
        b.total_length = old_total;
        assert!(b.length() > 0);

        a.replace_ptr(b.as_mut());
    }

    /// Assuming that a right child exists, perform the following operation:
    ///```
    ///    A               B
    ///   / \	          / \
    ///      B    =>     A
    ///     / \         / \
    ///    c               c
    pub fn rotate_left(&mut self) {
        let a = self;
        let mut b = a.get_right_raw();
        let mut c = ExternalPtr::new(b.left);
        let old_total = a.total_length;

        assert!(old_total > 0);
        assert!(a.length() > 0);
        assert!(b.length() > 0);

        // Make the parent of A point to B.
        if let Some(mut parent) = a.get_parent() {
            if a.is_left_child() {
                parent.left = b.as_mut();
            } else {
                parent.right = b.as_mut();
            }
        }
        a.copy_parent_to(&mut b);

        // Make B the parent of A.
        b.left = a.as_mut();
        a.set_parent(b);

        // Make A point to c.
        a.right = c.as_mut();
        if !c.is_null() {
            c.set_parent(*a);
        }

        // A's total length is decreased by the length of B and its right child.
        a.total_length -= b.total_length - if c.is_null() { 0 } else { c.total_length };
        assert!(a.total_length > 0);
        assert!(a.length() > 0);

        // B must have the same total length of A.
        b.total_length = old_total;
        assert!(b.length() > 0);

        a.replace_ptr(b.as_mut());
    }

    /// Traverse the interval tree, performing `function` on each node. No
    /// guarantee is made about the order of traversal.
    pub fn traverse_unordered(
        self,
        function: fn(IntervalRef, *mut libc::c_void),
        arg: *mut libc::c_void,
    ) {
        for tree in self.iter() {
            function(tree, arg);
        }
    }

    /// Traverse the interval tree, performing `function` on each node. The
    /// traversal is depth first from left to right.
    pub fn traverse(
        self,
        mut position: isize,
        function: fn(IntervalRef, LispObject),
        arg: LispObject,
    ) {
        for mut tree in self.iter() {
            position += tree.left_total_length();
            tree.position = position;
            function(tree, arg);
            position += tree.length();
        }
    }

    /// Balance the interval tree with the assumption that the subtrees
    /// themselves are already balanced.
    pub fn balance(&mut self) {
        assert!(self.length() > 0);
        assert!(self.total_length >= self.length());

        loop {
            let old_diff = self.left_total_length() - self.right_total_length();

            if old_diff > 0 {
                // Since the left child is longer, there must be one.
                let left = self.get_left_raw();
                let new_diff = self.total_length - left.total_length + left.right_total_length()
                    - left.left_total_length();

                if new_diff.abs() >= -old_diff {
                    break;
                }
                self.rotate_right();
                self.get_right_raw().balance();
            } else if old_diff < 0 {
                // Since the left child is longer, there must be one.
                let right = self.get_right_raw();
                let new_diff = self.total_length - right.total_length + right.left_total_length()
                    - right.right_total_length();

                if new_diff.abs() >= -old_diff {
                    break;
                }
                self.rotate_left();
                self.get_left_raw().balance();
            } else {
                break;
            }
        }
    }

    /// Balance the interval, potentially putting it back into its parent
    /// `LispObject`.
    pub fn balance_possible_root(&mut self) {
        let parent = self.get_object();

        if parent.is_none() && !self.has_parent() {
            return;
        }

        self.balance();

        if let Some(parent) = parent {
            if let Some(mut buffer) = parent.as_buffer() {
                buffer.set_intervals(*self);
            } else if let Some(mut string) = parent.as_string() {
                string.set_intervals(*self)
            }
        }
    }

    /// Reset the interval to its default no-property state
    pub fn reset(&mut self) {
        self.total_length = 0;
        self.position = 0;
        self.left = ptr::null_mut();
        self.right = ptr::null_mut();
        self.set_parent(ExternalPtr::new(ptr::null_mut()));
        self.plist = Qnil;
    }
}

pub struct DepthIter {
    stack: Vec<IntervalRef>,
}

impl Iterator for DepthIter {
    type Item = IntervalRef;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stack.is_empty() {
            None
        } else {
            let current = self.stack.pop();
            for tree in current.iter() {
                for t in tree.get_right_raw().iter() {
                    self.stack.push(t)
                }
                for t in tree.get_left_raw().iter() {
                    self.stack.push(t)
                }
            }
            current
        }
    }
}

#[no_mangle]
pub extern "C" fn create_root_interval(parent: LispObject) -> IntervalRef {
    IntervalRef::create_root(parent)
}

/// Balance an interval tree by weight (the amount of text).
#[no_mangle]
pub extern "C" fn balance_intervals(tree: IntervalRef) -> IntervalRef {
    fn recursion(mut tree: IntervalRef) -> IntervalRef {
        if let Some(left) = tree.get_left() {
            recursion(left);
        } else if let Some(right) = tree.get_right() {
            recursion(right);
        }
        tree.balance();
        tree
    };
    if tree.is_null() {
        ExternalPtr::new(ptr::null_mut() as *mut Lisp_Interval)
    } else {
        recursion(tree)
    }
}

#[no_mangle]
pub extern "C" fn traverse_intervals_noorder(
    tree: IntervalRef,
    function: fn(IntervalRef, *mut libc::c_void),
    arg: *mut libc::c_void,
) {
    tree.traverse_unordered(function, arg);
}

#[no_mangle]
pub extern "C" fn traverse_intervals(
    tree: IntervalRef,
    position: isize,
    function: fn(IntervalRef, LispObject),
    arg: LispObject,
) {
    tree.traverse(position, function, arg);
}
