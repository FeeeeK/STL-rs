use std::{
    alloc::System as SysAlloc,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ptr::{self},
};

use cstl_sys::{
    CSTL_ListVal, CSTL_list_assign_n, CSTL_list_back, CSTL_list_clear, CSTL_list_const_back,
    CSTL_list_const_front, CSTL_list_construct, CSTL_list_copy_assign, CSTL_list_copy_push_back,
    CSTL_list_copy_push_front, CSTL_list_destroy, CSTL_list_empty, CSTL_list_front,
    CSTL_list_max_size, CSTL_list_move_assign, CSTL_list_move_push_back, CSTL_list_move_push_front,
    CSTL_list_pop_back, CSTL_list_pop_front, CSTL_list_resize, CSTL_list_size, CSTL_list_swap,
};
use into_iter::IntoIter;

use crate::{
    alloc::{CxxProxy, WithCxxProxy},
    semantics::{BaseType, CopyMoveType, CopyOnlyType, DefaultUninit, MoveType},
};

pub mod into_iter;

pub type CxxList<T, A = SysAlloc> = CxxListLayout<T, A, Layout<T, A>>;

#[repr(C)]
pub struct Layout<T, A: CxxProxy> {
    alloc: A,
    val: CSTL_ListVal,
    _marker: PhantomData<T>,
}

#[repr(C)]
pub struct CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    inner: L,
    _marker: PhantomData<(T, A)>,
}

impl<T, A: CxxProxy> Layout<T, A> {
    pub fn new_in(alloc: A) -> Self {
        let mut new = Self {
            alloc,
            val: unsafe { mem::zeroed() },
            _marker: PhantomData,
        };

        new.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_construct(val, alloc);
        });

        new
    }
}

impl<T> CxxList<T, SysAlloc> {
    pub fn new() -> Self {
        Self {
            inner: Layout::new_in(SysAlloc),
            _marker: PhantomData,
        }
    }
}

impl<T, A: CxxProxy> CxxList<T, A> {
    pub fn new_in(alloc: A) -> Self {
        Self {
            inner: Layout::new_in(alloc),
            _marker: PhantomData,
        }
    }

    pub fn allocator(&self) -> &A {
        self.inner.alloc_as_ref()
    }
}

impl<T, A, L> CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    pub fn from_list_in<L2, A2>(list: CxxListLayout<T, A2, L2>, alloc: A) -> Self
    where
        L2: WithCxxProxy<Alloc = A2, Value = CSTL_ListVal>,
        A2: CxxProxy,
    {
        let mut new = Self::from_alloc(alloc);
        let mut drained = list;

        drained.inner.with_proxy_mut(|old_val, old_alloc| {
            new.inner.with_proxy_mut(|new_val, new_alloc| unsafe {
                CSTL_list_move_assign(
                    new_val,
                    <T as BaseType>::TYPE,
                    &<DefaultUninit<T> as MoveType>::MOVE,
                    old_val,
                    old_alloc,
                    new_alloc,
                    false,
                );
            })
        });

        new
    }

    pub fn into_list_in<A2, L2>(self, alloc: A2) -> CxxListLayout<T, A2, L2>
    where
        L2: WithCxxProxy<Alloc = A2, Value = CSTL_ListVal>,
        A2: CxxProxy,
    {
        CxxListLayout::from_list_in(self, alloc)
    }

    pub fn len(&self) -> usize {
        unsafe { CSTL_list_size(self.inner.value_as_ref()) }
    }

    pub fn is_empty(&self) -> bool {
        unsafe { CSTL_list_empty(self.inner.value_as_ref()) }
    }

    pub fn max_size(&self) -> usize {
        unsafe { CSTL_list_max_size(<T as BaseType>::TYPE) }
    }

    pub fn front(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            unsafe {
                (CSTL_list_const_front(self.inner.value_as_ref(), <T as BaseType>::TYPE)
                    as *const T)
                    .as_ref()
            }
        }
    }

    pub fn front_mut(&mut self) -> Option<&mut T> {
        if self.is_empty() {
            None
        } else {
            self.inner.with_proxy_mut(|val, _| unsafe {
                (CSTL_list_front(val, <T as BaseType>::TYPE) as *mut T).as_mut()
            })
        }
    }

    pub fn back(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            unsafe {
                (CSTL_list_const_back(self.inner.value_as_ref(), <T as BaseType>::TYPE) as *const T)
                    .as_ref()
            }
        }
    }

    pub fn back_mut(&mut self) -> Option<&mut T> {
        if self.is_empty() {
            None
        } else {
            self.inner.with_proxy_mut(|val, _| unsafe {
                (CSTL_list_back(val, <T as BaseType>::TYPE) as *mut T).as_mut()
            })
        }
    }

    pub fn push_front(&mut self, value: T) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            let mut value = DefaultUninit::new(value);

            let pushed = CSTL_list_move_push_front(
                val,
                <T as BaseType>::TYPE,
                &<DefaultUninit<T> as MoveType>::MOVE,
                value.as_mut_ptr() as _,
                alloc,
            );

            if !pushed {
                let _ = value.assume_init();
            }
        });
    }

    pub fn push_front_copy(&mut self, value: &T)
    where
        T: Clone,
    {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_copy_push_front(
                val,
                <T as BaseType>::TYPE,
                &<T as CopyOnlyType>::COPY,
                value as *const T as *const _,
                alloc,
            );
        });
    }

    pub fn push_back(&mut self, value: T) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            let mut value = DefaultUninit::new(value);

            let pushed = CSTL_list_move_push_back(
                val,
                <T as BaseType>::TYPE,
                &<DefaultUninit<T> as MoveType>::MOVE,
                value.as_mut_ptr() as _,
                alloc,
            );

            if !pushed {
                let _ = value.assume_init();
            }
        });
    }

    pub fn push_back_copy(&mut self, value: &T)
    where
        T: Clone,
    {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_copy_push_back(
                val,
                <T as BaseType>::TYPE,
                &<T as CopyOnlyType>::COPY,
                value as *const T as *const _,
                alloc,
            );
        });
    }

    pub fn pop_back(&mut self) {
        if !self.is_empty() {
            self.inner.with_proxy_mut(|val, alloc| unsafe {
                CSTL_list_pop_back(
                    val,
                    <T as BaseType>::TYPE,
                    &<DefaultUninit<T> as BaseType>::DROP,
                    alloc,
                );
            });
        }
    }

    pub fn pop_front(&mut self) {
        if !self.is_empty() {
            self.inner.with_proxy_mut(|val, alloc| unsafe {
                CSTL_list_pop_front(
                    val,
                    <T as BaseType>::TYPE,
                    &<DefaultUninit<T> as BaseType>::DROP,
                    alloc,
                );
            });
        }
    }

    pub fn clear(&mut self) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_clear(val, <T as BaseType>::TYPE, &<T as BaseType>::DROP, alloc);
        });
    }

    pub fn assign(&mut self, new_size: usize, value: &T)
    where
        T: Clone,
    {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_assign_n(
                val,
                <T as BaseType>::TYPE,
                &<T as CopyOnlyType>::COPY,
                new_size,
                value as *const T as *const _,
                alloc,
            );
        });
    }

    pub fn swap(&mut self, other: &mut Self) {
        self.inner.with_proxy_mut(|val1, _| {
            other.inner.with_proxy_mut(|val2, _| unsafe {
                CSTL_list_swap(val1, val2);
            })
        })
    }

    pub fn resize(&mut self, new_len: usize, value: T)
    where
        T: Clone,
    {
        if new_len > isize::MAX as usize {
            panic!("requested length ({new_len}) exceeded `isize::MAX`");
        }

        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_resize(
                val,
                <T as BaseType>::TYPE,
                &<DefaultUninit<T> as CopyMoveType>::COPY,
                new_len,
                &value as *const T as _,
                alloc,
            );
        });
    }

    pub fn resize_with<F>(&mut self, new_len: usize, mut f: F)
    where
        F: FnMut() -> T,
        T: Clone,
    {
        let len = self.len();

        if new_len > len {
            let additional = new_len - len;

            for _ in 0..additional {
                self.push_back(f());
            }
        } else if new_len < len {
            self.resize(new_len, f());
        }
    }
}

impl<T, A, L> CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn from_alloc(alloc: A) -> Self {
        Self {
            inner: L::new_in(alloc),
            _marker: PhantomData,
        }
    }
}

impl<T, A, L> AsRef<CxxListLayout<T, A, L>> for CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<T, A, L> AsMut<CxxListLayout<T, A, L>> for CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl<T, A, L> fmt::Debug for CxxListLayout<T, A, L>
where
    T: fmt::Debug,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, A, L> Default for CxxListLayout<T, A, L>
where
    A: CxxProxy + Default,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn default() -> Self {
        Self::from_alloc(A::default())
    }
}

impl<T, A, L> Drop for CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn drop(&mut self) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_destroy(val, <T as BaseType>::TYPE, &<T as BaseType>::DROP, alloc);
        });
    }
}

impl<T, A, L> Clone for CxxListLayout<T, A, L>
where
    T: Clone + Sized,
    A: CxxProxy + Clone,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn clone(&self) -> Self {
        let mut new = Self::from_alloc(self.inner.alloc_as_ref().clone());

        self.inner.with_proxy(|old_val, old_alloc| {
            new.inner.with_proxy_mut(|new_val, new_alloc| unsafe {
                CSTL_list_copy_assign(
                    new_val,
                    <T as BaseType>::TYPE,
                    &<T as CopyOnlyType>::COPY,
                    old_val,
                    new_alloc,
                    old_alloc,
                    false,
                );
            });
        });

        new
    }
}

impl<T, A, L> Extend<T> for CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        iter.into_iter().for_each(|e| self.push_back(e));
    }
}

impl<'a, T, A, L> Extend<&'a T> for CxxListLayout<T, A, L>
where
    T: Copy + 'a,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.extend(iter.into_iter().copied())
    }
}

impl<T, A1, A2, L1, L2> PartialEq<CxxListLayout<T, A2, L2>> for CxxListLayout<T, A1, L1>
where
    T: PartialEq,
    A1: CxxProxy,
    A2: CxxProxy,
    L1: WithCxxProxy<Alloc = A1, Value = CSTL_ListVal>,
    L2: WithCxxProxy<Alloc = A2, Value = CSTL_ListVal>,
{
    fn eq(&self, other: &CxxListLayout<T, A2, L2>) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<T, A1, A2, L1, L2> PartialOrd<CxxListLayout<T, A2, L2>> for CxxListLayout<T, A1, L1>
where
    T: PartialOrd,
    A1: CxxProxy,
    A2: CxxProxy,
    L1: WithCxxProxy<Alloc = A1, Value = CSTL_ListVal>,
    L2: WithCxxProxy<Alloc = A2, Value = CSTL_ListVal>,
{
    fn partial_cmp(&self, other: &CxxListLayout<T, A2, L2>) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<T, A, L> Eq for CxxListLayout<T, A, L>
where
    T: Eq,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
}

impl<T, A, L> Ord for CxxListLayout<T, A, L>
where
    T: Ord,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.iter().cmp(other.iter())
    }
}

impl<T, A, L> Hash for CxxListLayout<T, A, L>
where
    T: Hash,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for item in self.iter() {
            item.hash(state);
        }
    }
}

impl<T, A, L> IntoIterator for CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    type Item = T;
    type IntoIter = IntoIter<T, A>;

    fn into_iter(self) -> Self::IntoIter {
        unsafe {
            let list = ManuallyDrop::new(self);
            let alloc = ManuallyDrop::new(ptr::read(list.inner.alloc_as_ref()));
            let val = ptr::read(list.inner.value_as_ref());

            IntoIter::new(alloc, val)
        }
    }
}

impl<'a, T, A, L> CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    pub fn iter(&self) -> crate::list::into_iter::Iter<'a, T> {
        crate::list::into_iter::Iter::new(self.inner.value_as_ref())
    }
}

impl<'a, T, A, L> IntoIterator for &'a CxxListLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
    type Item = &'a T;
    type IntoIter = crate::list::into_iter::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

unsafe impl<T, A, L> Send for CxxListLayout<T, A, L>
where
    T: Send,
    A: CxxProxy + Send,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
}

unsafe impl<T, A, L> Sync for CxxListLayout<T, A, L>
where
    T: Sync,
    A: CxxProxy + Sync,
    L: WithCxxProxy<Alloc = A, Value = CSTL_ListVal>,
{
}

impl<T, A: CxxProxy> WithCxxProxy for Layout<T, A> {
    type Value = CSTL_ListVal;
    type Alloc = A;

    fn value_as_ref(&self) -> &Self::Value {
        &self.val
    }

    fn value_as_mut(&mut self) -> &mut Self::Value {
        &mut self.val
    }

    fn alloc_as_ref(&self) -> &Self::Alloc {
        &self.alloc
    }

    fn new_in(alloc: Self::Alloc) -> Self {
        let mut new = Self {
            alloc,
            val: unsafe { mem::zeroed() },
            _marker: PhantomData,
        };

        new.with_proxy_mut(|val, alloc| unsafe {
            CSTL_list_construct(val, alloc);
        });

        new
    }
}
