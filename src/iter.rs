use crate::{MutGuard, RawVechonk, Vechonk};
use alloc::boxed::Box;
use core::marker::PhantomData;
use core::mem;

/// An iterator over the elements of a [`Vechonk`]
pub struct Iter<'a, T: ?Sized> {
    raw: RawVechonk<T>,
    current_index: usize,
    _marker: PhantomData<&'a T>,
}

impl<'a, T: ?Sized> Iter<'a, T> {
    pub(super) fn new(chonk: &'a Vechonk<T>) -> Iter<'a, T> {
        Self {
            raw: chonk.raw.copy(),
            current_index: 0,
            _marker: PhantomData,
        }
    }
}

impl<'a, T: ?Sized> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_index == self.raw.len {
            return None;
        }

        // SAFETY: We just did a bounds check above
        let ptr = unsafe { self.raw.get_unchecked_ptr(self.current_index) };

        self.current_index += 1;

        // SAFETY: We rely on `get_unchecked_ptr` returning a valid pointer, which is does, see its SAFETY comments
        unsafe { Some(&*ptr) }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.raw.len - self.current_index;

        (count, Some(count))
    }
}

impl<'a, T: ?Sized> ExactSizeIterator for Iter<'a, T> {
    fn len(&self) -> usize {
        self.raw.len - self.current_index
    }
}

/// An iterator over the elements of a [`Vechonk`]
pub struct IterMut<'a, T: ?Sized> {
    raw: RawVechonk<T>,
    current_index: usize,
    _marker: PhantomData<&'a T>,
}

impl<'a, T: ?Sized> IterMut<'a, T> {
    pub(super) fn new(chonk: &'a mut Vechonk<T>) -> IterMut<'a, T> {
        Self {
            raw: chonk.raw.copy(),
            current_index: 0,
            _marker: PhantomData,
        }
    }
}

impl<'a, T: ?Sized> Iterator for IterMut<'a, T> {
    type Item = MutGuard<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_index == self.raw.len {
            return None;
        }

        let old_index = self.current_index;

        self.current_index += 1;

        // SAFETY: We did a bounds check above, and taken `&mut Vechonk`
        unsafe { Some(MutGuard::new(self.raw.copy(), old_index)) }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.raw.len - self.current_index;

        (count, Some(count))
    }
}

impl<'a, T: ?Sized> ExactSizeIterator for IterMut<'a, T> {
    fn len(&self) -> usize {
        self.raw.len - self.current_index
    }
}

/// An iterator over the elements of a [`Vechonk`]
pub struct IntoIter<T: ?Sized> {
    raw: RawVechonk<T>,
    current_index: usize,
    _marker: PhantomData<T>,
}

impl<'a, T: ?Sized> IntoIter<T> {
    pub(super) fn new(chonk: Vechonk<T>) -> IntoIter<T> {
        let raw = chonk.raw.copy();

        // We don't want to free the memory!
        mem::forget(chonk);

        Self {
            raw,
            current_index: 0,
            _marker: PhantomData,
        }
    }
}

impl<T: ?Sized> Iterator for IntoIter<T> {
    type Item = Box<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_index == self.raw.len {
            return None;
        }

        // SAFETY: We just did a bounds check above
        //         We also increment the `current_index`, to make sure that we never access it again
        let ptr = unsafe { self.raw.box_elem_unchecked(self.current_index) };

        self.current_index += 1;

        Some(ptr)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.raw.len - self.current_index;

        (count, Some(count))
    }
}

impl<T: ?Sized> ExactSizeIterator for IntoIter<T> {
    fn len(&self) -> usize {
        self.raw.len - self.current_index
    }
}

impl<T: ?Sized> Drop for IntoIter<T> {
    fn drop(&mut self) {
        // SAFETY: We as `Vechonk` do own the data, and it has the length `self.raw.cap`
        unsafe {
            RawVechonk::<T>::dealloc(self.raw.cap, self.raw.ptr.as_ptr());
        }
    }
}
