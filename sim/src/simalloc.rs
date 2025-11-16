use std::{
    cell::RefCell,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr::NonNull,
    rc::{Rc, Weak},
    slice,
};

use crate::{hierarchy::Hierarchy, lru_cache::Cache};

pub use crate::hierarchy::{CacheSpec, CacheStats};

pub struct Simalloc {
    cache_hierarchy: Hierarchy,
    this: Weak<RefCell<Self>>,
}

impl Simalloc {
    pub fn new(sizes: impl IntoIterator<Item = CacheSpec>) -> Rc<RefCell<Self>> {
        Rc::new_cyclic(|weak| {
            RefCell::new(Self {
                cache_hierarchy: Hierarchy::new(sizes),
                this: weak.clone(),
            })
        })
    }

    pub fn access_ref<T>(&mut self, val: &T) {
        let addr = (val as *const T).addr();
        // FIXME the correct block to access depends on the block size
        self.cache_hierarchy
            .access_addr(addr, std::mem::size_of::<T>());
    }

    pub fn pin_ref<'t, T: 't>(&mut self, val: T) -> PinGuard<T>
    where
        T: Ptr,
    {
        let addr = val.get_addr();
        let num_bytes = std::mem::size_of::<T>();
        self.cache_hierarchy.pin_addr(addr, num_bytes);
        PinGuard {
            addr,
            num_bytes,
            sim: self.this.upgrade().unwrap(),
            val,
        }
    }

    pub fn stats(&self) -> impl Iterator<Item = CacheStats> {
        self.cache_hierarchy.stats()
    }

    pub fn reset(&mut self) {
        self.cache_hierarchy.reset();
    }
}

pub trait Ptr {
    fn get_addr(&self) -> usize;
}

impl<T> Ptr for &'_ T {
    fn get_addr(&self) -> usize {
        (*self as *const T).addr()
    }
}

impl<T> Ptr for &'_ mut T {
    fn get_addr(&self) -> usize {
        (*self as *const T).addr()
    }
}

pub struct PinGuard<T: ?Sized> {
    addr: usize,
    num_bytes: usize,
    sim: Rc<RefCell<Simalloc>>,
    val: T,
}

impl<T: ?Sized> Deref for PinGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

impl<T: ?Sized> DerefMut for PinGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.val
    }
}

impl<T: ?Sized> Drop for PinGuard<T> {
    fn drop(&mut self) {
        self.sim
            .borrow_mut()
            .cache_hierarchy
            .unpin_addr(self.addr, self.num_bytes);
    }
}

pub static HASH_PTRS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct SPtr<T: ?Sized>(NonNull<T>, Rc<RefCell<Simalloc>>);

impl<T: ?Sized> SPtr<T> {
    pub fn sim(&self) -> &Rc<RefCell<Simalloc>> {
        &self.1
    }
}

impl<T> SPtr<T> {
    pub unsafe fn new_in_sim(ptr: NonNull<T>, sim: Rc<RefCell<Simalloc>>) -> Self
    where
        T: Sized,
    {
        Self(ptr, sim)
    }

    pub fn pin(&self) -> PinGuard<&T> {
        self.1.borrow_mut().pin_ref(unsafe { self.0.as_ref() })
    }

    pub fn pin_mut(&mut self) -> PinGuard<&mut T> {
        self.1.borrow_mut().pin_ref(unsafe { self.0.as_mut() })
    }

    pub fn into_inner_ptr(&mut self) -> NonNull<T> {
        self.0
    }

    pub unsafe fn byte_add(&self, count: usize) -> SRef<'_, T> {
        SRef(unsafe { self.0.byte_add(count) }, &self.1)
    }

    pub unsafe fn byte_add_mut(&mut self, count: usize) -> SRefMut<'_, T> {
        SRefMut(unsafe { self.0.byte_add(count) }, &self.1)
    }
}

#[derive(Clone, Copy)]
pub struct SRef<'t, T>(NonNull<T>, &'t RefCell<Simalloc>);

impl<'t, T> SRef<'t, T> {
    pub unsafe fn as_slice(self, len: usize) -> SSlice<'t, T> {
        SSlice(
            unsafe { slice::from_raw_parts(self.0.as_ptr(), len) },
            self.1,
        )
    }

    pub unsafe fn cast<U: 't>(self) -> SRef<'t, U> {
        SRef(self.0.cast(), self.1)
    }

    pub fn pin(&self) -> PinGuard<&T> {
        self.1.borrow_mut().pin_ref(unsafe { self.0.as_ref() })
    }

    pub fn into_ptr(self) -> *mut T {
        self.0.as_ptr()
    }
}

#[derive(Clone)]
pub struct SRefMut<'t, T>(NonNull<T>, &'t RefCell<Simalloc>);

impl<'t, T> SRefMut<'t, T> {
    pub unsafe fn as_slice(self, len: usize) -> SSlice<'t, T> {
        SSlice(
            unsafe { slice::from_raw_parts(self.0.as_ptr(), len) },
            self.1,
        )
    }

    pub unsafe fn as_slice_mut(self, len: usize) -> SSliceMut<'t, T> {
        SSliceMut(
            unsafe { slice::from_raw_parts_mut(self.0.as_ptr(), len) },
            self.1,
        )
    }

    pub unsafe fn cast<U: 't>(self) -> SRefMut<'t, U> {
        SRefMut(self.0.cast(), self.1)
    }

    pub fn pin(&self) -> PinGuard<&T> {
        self.1.borrow_mut().pin_ref(unsafe { self.0.as_ref() })
    }

    pub fn pin_mut(&mut self) -> PinGuard<&mut T> {
        self.1.borrow_mut().pin_ref(unsafe { self.0.as_mut() })
    }
}

pub struct SBox<T: ?Sized>(Box<T>, Rc<RefCell<Simalloc>>);

impl<T: ?Sized> SBox<T> {
    pub fn default_in_sim(sim: Rc<RefCell<Simalloc>>) -> SBox<T>
    where
        Box<T>: Default,
    {
        SBox(Box::default(), sim)
    }

    pub fn sim(&self) -> &Rc<RefCell<Simalloc>> {
        &self.1
    }
}

impl<T> SBox<[T]> {
    pub fn unsize_in_sim<const S: usize>(val: [T; S], sim: Rc<RefCell<Simalloc>>) -> Self {
        SBox(Box::new(val), sim)
    }

    pub fn new_uninit_in_sim(
        len: usize,
        sim: Rc<RefCell<Simalloc>>,
    ) -> SBox<[std::mem::MaybeUninit<T>]> {
        SBox(Box::new_uninit_slice(len), sim)
    }
}

impl<T> SBox<[std::mem::MaybeUninit<T>]> {
    pub unsafe fn assume_init(self) -> SBox<[T]> {
        let raw = Box::into_raw(self.0);
        unsafe { SBox(Box::from_raw(raw as *mut [T]), self.1) }
    }
}

impl<T> SBox<T> {
    pub fn new_in_sim(val: T, sim: Rc<RefCell<Simalloc>>) -> SBox<T>
    where
        T: Sized,
    {
        SBox(Box::new(val), sim)
    }

    pub fn pin(&self) -> PinGuard<&T> {
        self.1.borrow_mut().pin_ref(&*self.0)
    }

    pub fn pin_mut(&mut self) -> PinGuard<&mut T> {
        self.1.borrow_mut().pin_ref(&mut *self.0)
    }
}

impl<T> SBox<[T]> {
    pub fn pin_at(&self, i: usize) -> PinGuard<&T> {
        self.1.borrow_mut().pin_ref(&self.0[i])
    }

    pub fn pin_mut_at(&mut self, i: usize) -> PinGuard<&mut T> {
        self.1.borrow_mut().pin_ref(&mut self.0[i])
    }

    pub fn slice<R>(&self, range: R) -> SSlice<'_, T>
    where
        R: std::slice::SliceIndex<[T], Output = [T]>,
    {
        SSlice(&self.0[range], &*self.1)
    }

    pub fn slice_mut<R>(&mut self, range: R) -> SSliceMut<'_, T>
    where
        R: std::slice::SliceIndex<[T], Output = [T]>,
    {
        SSliceMut(&mut self.0[range], &*self.1)
    }
}

const _: () = {
    use std::fmt::Debug;
    impl<T: Debug> Debug for SBox<[Option<T>]> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            struct Blank;
            impl std::fmt::Debug for Blank {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("_")
                }
            }
            f.debug_list()
                .entries(self.0.iter().map(|e| match e {
                    Some(e) => e as &dyn Debug,
                    None => &Blank as _,
                }))
                .finish()
        }
    }
};

// impl<T> Deref for SBox<[T]> {
//     type Target = SSlice<T>;

//     fn deref(&self) -> &Self::Target {
//         unsafe { std::mem::transmute(&*self.0) }
//     }
// }

// impl<T> DerefMut for SBox<[T]> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         unsafe { std::mem::transmute(&mut *self.0) }
//     }
// }

// #[repr(transparent)]
// pub struct SVal<T>(T);

// impl<T> SVal<T> {
//     pub fn pin(&self) -> PinGuard<&T> {
//         self.0.pin_ref(&*self.0)
//     }

//     pub fn pin_mut(&mut self) -> PinGuard<&T> {
//         self.0.pin_ref(&mut *self.0)
//     }
// }

// impl<T> From<T> for SVal<T> {
//     fn from(value: T) -> Self {
//         Self(value)
//     }
// }

#[repr(C)]
pub struct SSlice<'t, T>(&'t [T], &'t RefCell<Simalloc>);

impl<'t, T> SSlice<'t, T> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn pin_at(&self, i: usize) -> PinGuard<&T> {
        self.1.borrow_mut().pin_ref(&self.0[i])
    }

    pub fn into_value_at(&self, i: usize) -> PinGuard<&'t T> {
        self.1.borrow_mut().pin_ref(&self.0[i])
    }

    pub fn slice<R>(&self, range: R) -> SSlice<'_, T>
    where
        R: std::slice::SliceIndex<[T], Output = [T]>,
    {
        SSlice(&self.0[range], &*self.1)
    }

    pub fn binary_search(&self, value: &T) -> Result<usize, usize>
    where
        T: Ord,
    {
        use std::{cmp::Ordering::*, hint};
        // based on the std version, adapted.
        let f = |t: &T| t.cmp(value);

        let mut size = self.len();
        if size == 0 {
            return Err(0);
        }
        let mut base = 0usize;

        while size > 1 {
            let half = size / 2;
            let mid = base + half;

            let cmp = f(&**self.pin_at(mid));
            base = hint::select_unpredictable(cmp == Greater, base, mid);
            size -= half;
        }

        let cmp = f(&**self.pin_at(base));
        if cmp == Equal {
            unsafe { hint::assert_unchecked(base < self.len()) };
            Ok(base)
        } else {
            let result = base + (cmp == Less) as usize;
            unsafe { hint::assert_unchecked(result <= self.len()) };
            Err(result)
        }
    }

    pub fn into_inner(self) -> &'t [T] {
        self.0
    }
}

pub struct SSliceMut<'t, T>(&'t mut [T], &'t RefCell<Simalloc>);

impl<'t, T> SSliceMut<'t, T> {
    pub fn pin_mut_at(&mut self, i: usize) -> PinGuard<&mut T> {
        self.1.borrow_mut().pin_ref(&mut self.0[i])
    }

    pub fn slice_mut<R>(&mut self, range: R) -> SSliceMut<'_, T>
    where
        R: std::slice::SliceIndex<[T], Output = [T]>,
    {
        SSliceMut(&mut self.0[range], &*self.1)
    }

    pub fn subrange<R>(self, range: R) -> SSliceMut<'t, T>
    where
        R: std::slice::SliceIndex<[T], Output = [T]>,
    {
        SSliceMut(&mut self.0[range], &*self.1)
    }

    pub unsafe fn as_maybe_uninit(self) -> SSliceMut<'t, MaybeUninit<T>> {
        SSliceMut(unsafe { std::mem::transmute(self.0) }, &*self.1)
    }

    pub fn into_inner(self) -> &'t mut [T] {
        self.0
    }
}

impl<'t, T> SSliceMut<'t, MaybeUninit<T>> {
    pub unsafe fn shift_insert(&mut self, value: T, idx: usize) {
        assert!(idx < self.len());
        for i in (idx + 1..(self.len())).rev() {
            let val = unsafe { self.pin_mut_at(i - 1).assume_init_read() };
            self.pin_mut_at(i).write(val);
        }
        self.pin_mut_at(idx).write(value);
    }
}

impl<'t, T> Deref for SSliceMut<'t, T>
where
    T: Sized,
{
    type Target = SSlice<'t, T>;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self as *const SSliceMut<'t, T> as *const SSlice<'t, T>) }
    }
}

// impl<T> Index<Range<usize>> for SBox<[T]> {
//     type Output = SSlice<T>;

//     fn index(&self, index: Range<usize>) -> &Self::Output {
//         unsafe { std::mem::transmute(&self.0[index]) }
//     }
// }

// impl<T> IndexMut<Range<usize>> for SBox<[T]> {
//     fn index_mut(&mut self, index: Range<usize>) -> &mut Self::Output {
//         unsafe { std::mem::transmute(&mut self.0[index]) }
//     }
// }

// impl<T> Deref for SBox<T>
// where
//     T: Sized,
// {
//     type Target = T;

//     fn deref(&self) -> &Self::Target {
//         let val = &*self.0;
//         self.1.borrow_mut().access_ref(val);
//         val
//     }
// }

// impl<T> DerefMut for SBox<T>
// where
//     T: Sized,
// {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         let val = &mut *self.0;
//         self.1.borrow_mut().access_ref(val);
//         val
//     }
// }

// impl<T> Index<usize> for SBox<[T]>
// where
//     T: Sized,
// {
//     type Output = T;

//     fn index(&self, index: usize) -> &Self::Output {
//         let val = &self.0[index];
//         self.1.borrow_mut().access_ref(val);
//         val
//     }
// }

// impl<T> IndexMut<usize> for SBox<[T]>
// where
//     T: Sized,
// {
//     fn index_mut(&mut self, index: usize) -> &mut Self::Output {
//         let val = &mut self.0[index];
//         self.1.borrow_mut().access_ref(val);
//         val
//     }
// }
