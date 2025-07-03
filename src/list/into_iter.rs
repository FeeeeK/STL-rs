use std::{
    alloc::System as SysAlloc,
    iter::FusedIterator,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ptr::{self},
};

use cstl_sys::{CSTL_ListVal, CSTL_ListNode};

use crate::{
    alloc::CxxProxy,
    list::{CxxList, Layout},
};

pub struct IntoIter<T, A: CxxProxy = SysAlloc> {
    alloc: ManuallyDrop<A>,
    list: CSTL_ListVal,
    current: *const CSTL_ListNode,
    sentinel: *const CSTL_ListNode,
    _marker: PhantomData<T>,
}

impl<T, A: CxxProxy> IntoIter<T, A> {
    pub(super) unsafe fn new(alloc: ManuallyDrop<A>, list: CSTL_ListVal) -> Self {
        let current = (*list.sentinel).next;
        let sentinel = list.sentinel;
        Self {
            alloc,
            list,
            current,
            sentinel,
            _marker: PhantomData,
        }
    }

    pub fn allocator(&self) -> &A {
        &self.alloc
    }
}

impl<T, A: CxxProxy> Iterator for IntoIter<T, A> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.current, self.sentinel) {
            return None;
        }
        unsafe {
            let value_ptr = self.current.add(1) as *const T;
            let value = ptr::read(value_ptr);
            self.current = (*self.current).next;
            Some(value)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.list.size, Some(self.list.size))
    }
}

impl<T, A: CxxProxy> ExactSizeIterator for IntoIter<T, A> {}

impl<T, A: CxxProxy> FusedIterator for IntoIter<T, A> {}

impl<T, A: CxxProxy> Drop for IntoIter<T, A> {
    fn drop(&mut self) {
        // The remaining elements are dropped when the CxxList is dropped.
        // We reconstruct it here and let it drop.
        unsafe {
            let _ = CxxList {
                inner: Layout {
                    alloc: ManuallyDrop::take(&mut self.alloc),
                    val: mem::replace(
                        &mut self.list,
                        CSTL_ListVal {
                            sentinel: ptr::null_mut(),
                            size: 0,
                        },
                    ),
                    _marker: PhantomData::<T>,
                },
                _marker: PhantomData,
            };
        }
    }
}

unsafe impl<T: Send, A: CxxProxy + Send> Send for IntoIter<T, A> {}
unsafe impl<T: Sync, A: CxxProxy + Sync> Sync for IntoIter<T, A> {}

pub struct Iter<'a, T> {
    sentinel: *const CSTL_ListNode,
    current: *const CSTL_ListNode,
    _marker: PhantomData<&'a T>,
}

impl<'a, T> Iter<'a, T> {
    pub(super) fn new(list: &CSTL_ListVal) -> Self {
        Self {
            sentinel: list.sentinel,
            current: unsafe { (*list.sentinel).next },
            _marker: PhantomData,
        }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() || self.current == self.sentinel {
            return None;
        }
        unsafe {
            let value_ptr = self.current.add(1) as *const T;
            let value = &*value_ptr;
            self.current = (*self.current).next;
            Some(value)
        }
    }
}

impl<'a, T> FusedIterator for Iter<'a, T> {}
