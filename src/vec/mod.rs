use std::{
    alloc::System as SysAlloc,
    borrow::{Borrow, BorrowMut},
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut, Index, IndexMut, Range},
    ptr::{self, NonNull},
    slice::{self, SliceIndex},
};

pub use cstl_sys::CSTL_VectorVal as RawVec;
use cstl_sys::{
    CSTL_vector_begin, CSTL_vector_clear, CSTL_vector_copy_assign, CSTL_vector_copy_assign_range,
    CSTL_vector_destroy, CSTL_vector_end, CSTL_vector_erase, CSTL_vector_iterator_add,
    CSTL_vector_iterator_eq, CSTL_vector_move_assign, CSTL_vector_move_assign_range,
    CSTL_vector_move_insert, CSTL_vector_move_push_back, CSTL_vector_pop_back, CSTL_vector_reserve,
    CSTL_vector_resize, CSTL_vector_shrink_to_fit, CSTL_vector_truncate,
};
use into_iter::IntoIter;

use crate::{
    alloc::{CxxProxy, WithCxxProxy},
    semantics::{BaseType, CopyMoveType, CopyOnlyType, DefaultUninit, MoveType},
};

pub mod into_iter;
#[cfg(feature = "msvc2012")]
pub mod msvc2012;

pub type CxxVec<T, A = SysAlloc> = CxxVecLayout<T, A, Layout<A>>;

#[repr(C)]
pub struct Layout<A: CxxProxy> {
    alloc: A,
    val: RawVec,
}

#[repr(C)]
pub struct CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    inner: L,
    _marker: PhantomData<(T, A)>,
}

impl<A: CxxProxy> Layout<A> {
    pub const fn new_in(alloc: A) -> Self {
        Self {
            alloc,
            val: new_val(),
        }
    }
}

impl<T> CxxVec<T, SysAlloc> {
    pub const fn new() -> Self {
        Self {
            inner: Layout::new_in(SysAlloc),
            _marker: PhantomData,
        }
    }
}

impl<T, A: CxxProxy> CxxVec<T, A> {
    pub const fn new_in(alloc: A) -> Self {
        Self {
            inner: Layout::new_in(alloc),
            _marker: PhantomData,
        }
    }

    pub const fn allocator(&self) -> &A {
        &self.inner.alloc
    }
}

impl<T, A, L> CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    pub fn from_vec_in<L2, A2>(vec: CxxVecLayout<T, A2, L2>, alloc: A) -> Self
    where
        L2: WithCxxProxy<Alloc = A2, Value = RawVec>,
        A2: CxxProxy,
    {
        let mut new = Self::from_alloc(alloc);
        let mut drained = vec;

        drained.inner.with_proxy_mut(|old_val, old_alloc| {
            new.inner.with_proxy_mut(|new_val, new_alloc| unsafe {
                let moved = CSTL_vector_move_assign(
                    new_val,
                    <T as BaseType>::TYPE,
                    &<DefaultUninit<T> as MoveType>::MOVE,
                    old_val,
                    old_alloc,
                    new_alloc,
                    false,
                );

                if moved {
                    old_val.last = old_val.first;
                }
            })
        });

        new
    }

    pub fn into_vec_in<A2, L2>(self, alloc: A2) -> CxxVecLayout<T, A2, L2>
    where
        L2: WithCxxProxy<Alloc = A2, Value = RawVec>,
        A2: CxxProxy,
    {
        CxxVecLayout::from_vec_in(self, alloc)
    }

    pub fn from_rust_vec_in(vec: Vec<T>, alloc: A) -> Self {
        let mut new = Self::from_alloc(alloc);
        let mut drained = vec;

        new.inner.with_proxy_mut(|val, alloc| unsafe {
            let Range { start, end } = drained.as_mut_ptr_range();

            let moved = CSTL_vector_move_assign_range(
                val,
                <T as BaseType>::TYPE,
                &<DefaultUninit<T> as MoveType>::MOVE,
                start as _,
                end as _,
                alloc,
            );

            if moved {
                drained.set_len(0);
            }
        });

        new
    }

    pub fn into_rust_vec(self) -> Vec<T> {
        let mut new = Vec::new();
        let mut drained = self;

        unsafe {
            let left_uninit = slice::from_raw_parts_mut(
                drained.first_ptr_mut() as *mut DefaultUninit<T>,
                drained.len(),
            );

            new.extend(left_uninit.iter_mut().map(|v| mem::take(v).assume_init()));

            drained.inner.value_as_mut().last = drained.inner.value_as_mut().first;
        };

        new
    }

    pub fn from_slice_in(slice: &[T], alloc: A) -> Self
    where
        T: Clone,
    {
        let mut new = Self::from_alloc(alloc);

        new.reserve(slice.len());

        new.inner.with_proxy_mut(|val, alloc| unsafe {
            let Range { start, end } = slice.as_ptr_range();

            CSTL_vector_copy_assign_range(
                val,
                <T as BaseType>::TYPE,
                &<T as CopyOnlyType>::COPY,
                start as _,
                end as _,
                alloc,
            );
        });

        new
    }

    pub fn as_ptr(&self) -> *const T {
        if !self.first_ptr().is_null() {
            self.first_ptr()
        } else {
            ptr::dangling()
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        if !self.first_ptr_mut().is_null() {
            self.first_ptr_mut()
        } else {
            ptr::dangling_mut()
        }
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }

    pub fn len(&self) -> usize {
        unsafe { self.last_ptr().offset_from(self.first_ptr()) as usize }
    }

    pub fn is_empty(&self) -> bool {
        self.first_ptr() == self.last_ptr()
    }

    pub fn capacity(&self) -> usize {
        unsafe { self.end_ptr().offset_from(self.first_ptr()) as usize }
    }

    pub fn push(&mut self, value: T) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            let mut value = DefaultUninit::new(value);

            let pushed = CSTL_vector_move_push_back(
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

    pub fn pop(&mut self) -> Option<T> {
        if !self.is_empty() {
            unsafe {
                let last = self.last_ptr().offset(-1).read();

                CSTL_vector_pop_back(
                    self.inner.value_as_mut(),
                    <T as BaseType>::TYPE,
                    &<DefaultUninit<T> as BaseType>::DROP,
                );

                Some(last)
            }
        } else {
            None
        }
    }

    pub fn insert(&mut self, index: usize, value: T) {
        let len = self.len();

        if index > len {
            panic!("insertion index (is {index}) should be <= len (is {len})");
        }

        self.inner.with_proxy_mut(|val, alloc| unsafe {
            let pos = CSTL_vector_iterator_add(
                CSTL_vector_begin(val, <T as BaseType>::TYPE),
                index as isize,
            );

            let mut value = DefaultUninit::new(value);

            let inserted = CSTL_vector_move_insert(
                val,
                &<DefaultUninit<T> as MoveType>::MOVE,
                pos,
                value.as_mut_ptr() as _,
                alloc,
            );

            if CSTL_vector_iterator_eq(inserted, CSTL_vector_end(val, <T as BaseType>::TYPE)) {
                drop(value.assume_init());
            }
        });
    }

    pub fn remove(&mut self, index: usize) -> T {
        let len = self.len();

        if index > len {
            panic!("removal index (is {index}) should be <= len (is {len})");
        }

        unsafe {
            let removed = self.first_ptr().add(index).read();

            let pos = CSTL_vector_iterator_add(
                CSTL_vector_begin(self.inner.value_as_ref(), <T as BaseType>::TYPE),
                index as isize,
            );

            CSTL_vector_erase(
                self.inner.value_as_mut(),
                &<DefaultUninit<T> as MoveType>::MOVE,
                pos,
            );

            removed
        }
    }

    pub fn clear(&mut self) {
        unsafe {
            CSTL_vector_clear(self.inner.value_as_mut(), &<T as BaseType>::DROP);
        }
    }

    pub fn resize(&mut self, new_len: usize, value: T)
    where
        T: Clone,
    {
        if new_len > isize::MAX as usize {
            panic!("requested length ({new_len}) exceeded `isize::MAX`");
        }

        if new_len > self.len() {
            self.inner.with_proxy_mut(|val, alloc| unsafe {
                CSTL_vector_resize(
                    val,
                    <T as BaseType>::TYPE,
                    &<DefaultUninit<T> as CopyMoveType>::COPY,
                    new_len,
                    &value as *const T as _,
                    alloc,
                );
            });
        } else {
            self.truncate(new_len);
        }
    }

    pub fn resize_with<F>(&mut self, new_len: usize, mut f: F)
    where
        F: FnMut() -> T,
    {
        let len = self.len();

        if new_len > len {
            let additional = new_len - len;

            self.reserve(additional);

            for _ in 0..additional {
                self.push(f());
            }
        } else {
            self.truncate(new_len);
        }
    }

    pub fn truncate(&mut self, new_len: usize) {
        if new_len < self.len() {
            unsafe {
                CSTL_vector_truncate(
                    self.inner.value_as_mut(),
                    <T as BaseType>::TYPE,
                    &<T as BaseType>::DROP,
                    new_len,
                );
            }
        }
    }

    pub fn reserve(&mut self, additional: usize) {
        let capacity = self.capacity();

        if isize::MAX as usize - capacity < additional {
            panic!("requested capacity ({capacity} + {additional}) overflowed `isize::MAX`");
        }

        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_vector_reserve(
                val,
                <T as BaseType>::TYPE,
                &<DefaultUninit<T> as MoveType>::MOVE,
                capacity + additional,
                alloc,
            );
        });
    }

    pub fn shrink_to_fit(&mut self) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_vector_shrink_to_fit(
                val,
                <T as BaseType>::TYPE,
                &<DefaultUninit<T> as MoveType>::MOVE,
                alloc,
            );
        });
    }
}

impl<T, A, L> CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn from_alloc(alloc: A) -> Self {
        Self {
            inner: L::new_in(alloc),
            _marker: PhantomData,
        }
    }

    fn first_ptr(&self) -> *const T {
        self.inner.value_as_ref().first as _
    }

    fn last_ptr(&self) -> *const T {
        self.inner.value_as_ref().last as _
    }

    fn end_ptr(&self) -> *const T {
        self.inner.value_as_ref().end as _
    }

    fn first_ptr_mut(&mut self) -> *mut T {
        self.inner.value_as_mut().first as _
    }
}

impl<T, A, L> AsRef<CxxVecLayout<T, A, L>> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<T, A, L> AsRef<[T]> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T, A, L> AsMut<CxxVecLayout<T, A, L>> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl<T, A, L> AsMut<[T]> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

impl<T, A, L> Borrow<[T]> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn borrow(&self) -> &[T] {
        &self[..]
    }
}

impl<T, A, L> BorrowMut<[T]> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn borrow_mut(&mut self) -> &mut [T] {
        &mut self[..]
    }
}

impl<T, A, L> fmt::Debug for CxxVecLayout<T, A, L>
where
    T: fmt::Debug,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T, A, L> Default for CxxVecLayout<T, A, L>
where
    A: CxxProxy + Default,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn default() -> Self {
        Self::from_alloc(A::default())
    }
}

impl<T, A, L> Deref for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T, A, L> DerefMut for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T, A, L> Drop for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn drop(&mut self) {
        self.inner.with_proxy_mut(|val, alloc| unsafe {
            CSTL_vector_destroy(val, <T as BaseType>::TYPE, &<T as BaseType>::DROP, alloc);
        });
    }
}

impl<T, A, L> Clone for CxxVecLayout<T, A, L>
where
    T: Clone + Sized,
    A: CxxProxy + Clone,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn clone(&self) -> Self {
        let mut new = Self::from_alloc(self.inner.alloc_as_ref().clone());

        self.inner.with_proxy(|old_val, old_alloc| {
            new.inner.with_proxy_mut(|new_val, new_alloc| unsafe {
                CSTL_vector_copy_assign(
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

impl<T, I, A, L> Index<I> for CxxVecLayout<T, A, L>
where
    I: SliceIndex<[T]>,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    type Output = I::Output;

    fn index(&self, index: I) -> &Self::Output {
        Index::index(&**self, index)
    }
}

impl<T, I, A, L> IndexMut<I> for CxxVecLayout<T, A, L>
where
    I: SliceIndex<[T]>,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        IndexMut::index_mut(&mut **self, index)
    }
}

impl<T, A, L> Extend<T> for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.reserve(iter.size_hint().0);
        iter.for_each(|e| self.push(e));
    }
}

impl<'a, T, A, L> Extend<&'a T> for CxxVecLayout<T, A, L>
where
    T: Copy + 'a,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.extend(iter.into_iter().copied())
    }
}

impl<T, A1, A2, L1, L2> PartialEq<CxxVecLayout<T, A2, L2>> for CxxVecLayout<T, A1, L1>
where
    T: PartialEq,
    A1: CxxProxy,
    A2: CxxProxy,
    L1: WithCxxProxy<Alloc = A1, Value = RawVec>,
    L2: WithCxxProxy<Alloc = A2, Value = RawVec>,
{
    fn eq(&self, other: &CxxVecLayout<T, A2, L2>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<T, A1, A2, L1, L2> PartialOrd<CxxVecLayout<T, A2, L2>> for CxxVecLayout<T, A1, L1>
where
    T: PartialOrd,
    A1: CxxProxy,
    A2: CxxProxy,
    L1: WithCxxProxy<Alloc = A1, Value = RawVec>,
    L2: WithCxxProxy<Alloc = A2, Value = RawVec>,
{
    fn partial_cmp(&self, other: &CxxVecLayout<T, A2, L2>) -> Option<std::cmp::Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<T, A, L> Eq for CxxVecLayout<T, A, L>
where
    T: Eq,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
}

impl<T, A, L> Ord for CxxVecLayout<T, A, L>
where
    T: Ord,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<T, A, L> Hash for CxxVecLayout<T, A, L>
where
    T: Hash,
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<T, A, L> IntoIterator for CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    type Item = T;
    type IntoIter = IntoIter<T, A>;

    fn into_iter(self) -> Self::IntoIter {
        unsafe {
            // Dropped by IntoIter:
            let mut vec = ManuallyDrop::new(self);
            let alloc = ManuallyDrop::new(ptr::read(vec.inner.alloc_as_ref()));

            let ptr = NonNull::new_unchecked(vec.as_mut_ptr());
            let end = ptr.add(vec.len());

            let val = RawVec {
                first: vec.first_ptr() as _,
                last: vec.last_ptr() as _,
                end: vec.end_ptr() as _,
            };

            IntoIter {
                alloc,
                val,
                _marker: PhantomData,
                ptr,
                end,
            }
        }
    }
}

impl<'a, T, A, L> IntoIterator for &'a CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T, A, L> IntoIterator for &'a mut CxxVecLayout<T, A, L>
where
    A: CxxProxy,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
    type Item = &'a mut T;
    type IntoIter = slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

unsafe impl<T, A, L> Send for CxxVecLayout<T, A, L>
where
    T: Send,
    A: CxxProxy + Send,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
}

unsafe impl<T, A, L> Sync for CxxVecLayout<T, A, L>
where
    T: Sync,
    A: CxxProxy + Sync,
    L: WithCxxProxy<Alloc = A, Value = RawVec>,
{
}

const fn new_val() -> RawVec {
    RawVec {
        first: ptr::null_mut(),
        last: ptr::null_mut(),
        end: ptr::null_mut(),
    }
}

impl<A: CxxProxy> WithCxxProxy for Layout<A> {
    type Value = RawVec;
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
        Self {
            alloc,
            val: new_val(),
        }
    }
}
