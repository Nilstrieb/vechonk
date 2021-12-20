#![no_std]
#![feature(ptr_metadata)]
#![deny(unsafe_op_in_unsafe_fn)]

//!
//! A `Vec<T: ?Sized>`
//!
//! It's implemented by laying out the elements in memory contiguously like [`alloc::vec::Vec`]
//!
//! # Layout
//!
//! A [`Vechonk`] is 3 `usize` long. It owns a single allocation, containing the elements and the metadata.
//! The elements are laid out contiguously from the front, while the metadata is laid out contiguously from the back.
//! Both grow towards the center until they meet and get realloced to separate them again.
//!
//! ```txt
//!
//!             Vechonk<str>
//!             ╭──────────────────────────────────╮
//!             │ ptr   | len   | cap  | elem_size │
//!             ╰──────────────────────────────────╯
//!                │               │        │
//!                │               ╰────────│──────────────────────────────────────╮
//!                │                        │                                      │
//!                │               ╭────────╯                                      │
//!         Heap   ▼               ▼                                               ▼
//!         ╭────────────┬─────────┬─────────────────┬──────────────┬──────────────╮
//! value   │ "hello"    │ "uwu"   │  <uninit>       │ 0 - 5        │ 5 - 3        │
//!         ├────────────┼─────────┼─────────────────┼──────────────┼──────────────┤
//!  size   │ dynamic    │ dynamic │  rest of alloc  │ usize + meta │ usize + meta │
//!         ╰────────────┴─────────┴─────────────────┴──────────────┴──────────────╯
//!             ▲            ▲                          │              │
//!             ╰────────────│──────────────────────────╯              │
//!                          ╰─────────────────────────────────────────╯
//! ```

mod test;

extern crate alloc;

use alloc::boxed::Box;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::num::NonZeroUsize;
use core::ops::Index;
use core::ptr::{NonNull, Pointee};
use core::{mem, ptr};

/// chonky af
///
/// note: it does not run destructors for now, thankfully that is 100% safe :))))
pub struct Vechonk<T: ?Sized> {
    /// A pointer to the first element
    ptr: NonNull<u8>,
    /// How many elements the Vechonk has
    len: usize,
    /// How much memory the Vechonk owns
    cap: usize,
    /// How much memory has been used by the elements, where the next element starts
    elem_size: usize,
    _marker: PhantomData<T>,
}

struct PtrData<T: ?Sized> {
    offset: usize,
    meta: <T as Pointee>::Metadata,
}

impl<T: ?Sized> Copy for PtrData<T> {}
impl<T: ?Sized> Clone for PtrData<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Vechonk<T> {
    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Create a new empty Vechonk that doesn't allocate anything
    pub const fn new() -> Self {
        Self {
            // SAFETY: 1 is not 0
            ptr: NonNull::dangling(),
            len: 0,
            cap: 0,
            elem_size: 0,
            _marker: PhantomData,
        }
    }

    /// Create a new Vechonk that allocates `capacity` bytes. `capacity` gets shrunken down
    /// to the next multiple of the alignment of usize + metadata of `T`
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = force_align(capacity, Self::data_align());

        let mut vechonk = Self::new();

        if capacity == 0 {
            return vechonk;
        }

        // SAFETY: capacity has been checked to not be 0 and the len is 0
        unsafe {
            vechonk.grow_to(NonZeroUsize::new_unchecked(capacity));
        }
        vechonk
    }

    /// Pushes a new element into the [`Vechonk`]. Does panic (for now) if there is no more capacity
    /// todo: don't take a box but some U that can be unsized into T
    pub fn push(&mut self, element: Box<T>) {
        let elem_size = mem::size_of_val(element.as_ref());

        let elem_ptr = Box::into_raw(element);
        let meta = ptr::metadata(elem_ptr);

        let data_size = mem::size_of::<PtrData<T>>();

        // just panic here instead of a proper realloc
        if self.needs_grow(elem_size + data_size) {
            self.regrow(self.cap + elem_size + data_size);
        }

        let elem_offset = self.elem_size;

        let data = PtrData {
            offset: elem_offset,
            meta,
        };

        // Copy the element to the new location
        // SAFETY: `self.elem_size` can't be longer than the allocation, because `PtrData<T>` needs space as well
        let dest_ptr = unsafe { self.ptr.as_ptr().add(elem_offset) };

        // SAFETY: `elem_ptr` comes from `Box`, and is therefore valid to read from for the size
        //         We have made sure above that we have more than `elem_size` bytes free
        //         The two allocations cannot overlap, since the `Box` owned its contents, and so do we
        unsafe {
            ptr::copy_nonoverlapping::<u8>(elem_ptr as _, dest_ptr, elem_size);
        }

        let data_offset = self.offset_for_data(self.len);

        // SAFETY: The offset will always be less than `self.cap`, because we can't have more than `self.len` `PtrData`
        let data_ptr = unsafe { self.ptr.as_ptr().add(data_offset) };
        let data_ptr = data_ptr as *mut PtrData<T>;

        // SAFETY: The pointer is aligned, because `self.ptr` and `self.cap` are
        //         It's not out of bounds for our allocation, see above
        unsafe {
            *data_ptr = data;
        }

        self.elem_size += elem_size;
        self.len += 1;

        // SAFETY: This was allocated by `Box`, so we know that it is valid.
        //         The ownership of the value was transferred to `Vechonk` by copying it out
        unsafe {
            alloc::alloc::dealloc(
                elem_ptr as _,
                Layout::from_size_align(elem_size, mem::align_of_val(&*elem_ptr)).unwrap(),
            )
        }
    }

    fn regrow(&mut self, min_size: usize) {
        // new_cap must be properly "aligned" for `PtrData<T>`
        let new_cap = force_align(min_size * 2, Self::data_align());

        let old_ptr = self.ptr.as_ptr();
        let old_cap = self.cap;

        let last_data_index = self.len.saturating_sub(1);
        let old_data_offset = self.offset_for_data(last_data_index);

        // SAFETY: new_cap can't be 0 because of the +1
        //         We will copy the elements over
        unsafe {
            self.grow_to(NonZeroUsize::new_unchecked(new_cap));
        }

        // copy the elements first
        // SAFETY: both pointers point to the start of allocations smaller than `self.elem_size` and own them
        unsafe {
            ptr::copy_nonoverlapping(old_ptr, self.ptr.as_ptr(), self.elem_size);
        }

        // then copy the data
        // SAFETY: both pointers have been offset by less than `self.cap`, and the `data_section_size` fills the allocation perfectly
        unsafe {
            let new_data_ptr = self.ptr.as_ptr().add(self.offset_for_data(last_data_index));

            ptr::copy_nonoverlapping(
                old_ptr.add(old_data_offset),
                new_data_ptr,
                self.data_section_size(),
            )
        }

        // now free the old data
        // SAFETY: This was previously allocated and is not used anymore
        unsafe {
            Self::dealloc(old_cap, old_ptr);
        }
    }

    /// Grows the `Vechonk` to a new capacity. This will not copy any elements. This will put the `Vechonk`
    /// into an invalid state, since the `len` is still the length of the old allocation.
    ///
    /// This doesn't free any memory
    ///
    /// # Safety
    /// The caller must either set the `len` to zero, or copy the elements to the new allocation by saving
    /// `self.ptr` before calling this function.
    unsafe fn grow_to(&mut self, size: NonZeroUsize) {
        let layout = Layout::from_size_align(size.get(), Self::data_align()).unwrap();

        // SAFETY: layout is guaranteed to have a non-zero size
        let alloced_ptr;

        // we only care about it being zeroed for debugging since it makes it easier
        #[cfg(debug_assertions)]
        unsafe {
            alloced_ptr = alloc::alloc::alloc_zeroed(layout)
        }
        #[cfg(not(debug_assertions))]
        unsafe {
            alloced_ptr = alloc::alloc::alloc(layout)
        }

        self.ptr =
            NonNull::new(alloced_ptr).unwrap_or_else(|| alloc::alloc::handle_alloc_error(layout));

        self.cap = size.get();
    }

    unsafe fn get_unchecked(&self, index: usize) -> &T {
        let data_offset = self.offset_for_data(index);

        // SAFETY: We can assume that the index is valid.
        let data_ptr = unsafe { self.ptr.as_ptr().add(data_offset) };
        let data_ptr = data_ptr as *mut PtrData<T>;

        // SAFETY: The pointer is aligned because `self.ptr` is aligned and `data_offset` is a multiple of the alignment
        //         The value behind it is always a `PtrData<T>`
        let data = unsafe { *data_ptr };

        let elem_ptr = unsafe { self.ptr.as_ptr().add(data.offset) };

        let elem_meta_ptr = ptr::from_raw_parts(elem_ptr as *const (), data.meta);

        // SAFETY: The metadata is only assigned directly from the pointer metadata of the original object and therefore valid
        //         The pointer is calculated from the offset, which is also valid
        //         The pointer is not aligned btw, todo lol
        unsafe { &*elem_meta_ptr }
    }

    // SAFETY: The allocation must be owned by `ptr` and have the length `cap`
    unsafe fn dealloc(cap: usize, ptr: *mut u8) {
        if cap == 0 {
            return;
        }

        // SAFETY: Align must be valid since it's obtained using `align_of`
        let layout =
            unsafe { Layout::from_size_align_unchecked(cap, mem::align_of::<PtrData<T>>()) };

        unsafe { alloc::alloc::dealloc(ptr, layout) };
    }

    /// Returns a multiple of the alignment of `PtrData<T>`, since `self.cap` is one, and so is the size
    const fn offset_for_data(&self, index: usize) -> usize {
        self.cap
            .saturating_sub(mem::size_of::<PtrData<T>>() * (index + 1))
    }

    fn needs_grow(&self, additional_size: usize) -> bool {
        additional_size > self.cap - (self.elem_size + self.data_section_size())
    }

    const fn data_section_size(&self) -> usize {
        self.len * mem::size_of::<PtrData<T>>()
    }

    const fn data_align() -> usize {
        mem::align_of::<PtrData<T>>()
    }

    /// used for debugging memory layout
    /// safety: cap must be 96
    #[allow(dead_code)]
    #[doc(hidden)]
    pub unsafe fn debug_chonk(&self) {
        let array = unsafe { *(self.ptr.as_ptr() as *mut [u8; 96]) };

        panic!("{:?}", array)
    }
}

impl<T: ?Sized> Index<usize> for Vechonk<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index >= self.len {
            panic!("Out of bounds, index {} for len {}", index, self.len);
        }

        // SAFETY: The index is not out of bounds
        unsafe { self.get_unchecked(index) }
    }
}

/// don't bother with destructors for now
impl<T: ?Sized> Drop for Vechonk<T> {
    fn drop(&mut self) {
        unsafe {
            Self::dealloc(self.cap, self.ptr.as_ptr());
        }
    }
}

impl<T: ?Sized> Default for Vechonk<T> {
    fn default() -> Self {
        Self::new()
    }
}

const fn force_align(size: usize, align: usize) -> usize {
    size - (size % align)
}
