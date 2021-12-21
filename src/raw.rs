use crate::force_align;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::num::NonZeroUsize;
use core::ptr::{NonNull, Pointee};
use core::{mem, ptr};

/// The offset + metadata for each element, stored at the end
pub struct PtrData<T: ?Sized> {
    offset: usize,
    meta: <T as Pointee>::Metadata,
}

impl<T: ?Sized> Copy for PtrData<T> {}
impl<T: ?Sized> Clone for PtrData<T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// `RawVechonk` is a view onto heap memory layout like shown in the crate level docs ([`crate`])
///
/// This could be owned or borrowed, ownership semantics are added by the wrapper
pub struct RawVechonk<T: ?Sized> {
    /// A pointer to the first element
    pub ptr: NonNull<u8>,
    /// How many elements the Vechonk has
    pub len: usize,
    /// How much memory the Vechonk owns
    pub cap: usize,
    /// How much memory has been used by the elements, where the next element starts
    pub elem_size: usize,
    pub _marker: PhantomData<T>,
}

impl<T: ?Sized> RawVechonk<T> {
    /// crate a shallow copy of this
    pub const fn copy(&self) -> Self {
        Self {
            ptr: self.ptr,
            len: self.len,
            cap: self.cap,
            elem_size: self.elem_size,
            _marker: PhantomData,
        }
    }

    pub const fn new() -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            cap: 0,
            elem_size: 0,
            _marker: PhantomData,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = force_align(capacity, Self::data_align());

        let mut vechonk = Self::new();

        if capacity == 0 {
            return vechonk;
        }

        // SAFETY: capacity has been checked to not be 0 and the len is 0
        unsafe {
            vechonk.realloc(NonZeroUsize::new_unchecked(capacity));
        }
        vechonk
    }

    pub fn push(&mut self, element: Box<T>) {
        let elem_size = mem::size_of_val(element.as_ref());

        let elem_align = mem::align_of_val(element.as_ref());
        let elem_ptr = Box::into_raw(element);
        let meta = ptr::metadata(elem_ptr);

        let data_size = mem::size_of::<PtrData<T>>();
        let elem_offset = self.elem_size;

        // SAFETY: `self.elem_size` can't be longer than the allocation, because `PtrData<T>` needs space as well
        let required_align_offset =
            unsafe { self.ptr.as_ptr().add(elem_offset).align_offset(elem_align) };

        if required_align_offset == usize::MAX {
            panic!(
                "Cannot align pointer for element with size: {}, alignment: {}",
                elem_size, elem_align
            );
        }

        // just panic here instead of a proper realloc
        if self.needs_grow(elem_size + data_size + required_align_offset) {
            self.regrow(self.cap + elem_size + data_size);
        }

        // Copy the element to the new location
        // Calculate the dest pointer again because we might have realloced
        // SAFETY: `self.elem_size` can't be longer than the allocation, because `PtrData<T>` needs space as well
        let dest_ptr = unsafe { self.ptr.as_ptr().add(elem_offset) };
        let dest_align_offset = dest_ptr.align_offset(elem_align);
        let dest_ptr = unsafe { dest_ptr.add(dest_align_offset) };

        let data = PtrData {
            offset: elem_offset + dest_align_offset,
            meta,
        };

        // SAFETY: `elem_ptr` comes from `Box`, and is therefore valid to read from for the size
        //         We have made sure above that we have more than `elem_size` bytes free
        //         The two allocations cannot overlap, since the `Box` owned its contents, and so do we
        //         `dest_ptr` has been aligned above
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

    pub fn pop(&mut self) -> Option<Box<T>> {
        if self.len == 0 {
            return None;
        }

        // SAFETY: `self.len - 1` is the last element, and therefore not out of bounds
        let data = unsafe { self.get_data(self.len - 1) };

        // SAFETY: We can assume that the `offset` from `data` is not out of bounds
        let elem_ptr = unsafe { self.ptr.as_ptr().add(data.offset) };

        // allocate a new `Box` for the return value
        let elem_fat_ptr = ptr::from_raw_parts_mut::<T>(elem_ptr as *mut (), data.meta);
        // SAFETY: The metadata has been preserved, and the pointer has been properly aligned and initialized
        // when the element was added
        let elem_fat_ref = unsafe { &*elem_fat_ptr };

        let element_box_layout = Layout::for_value(elem_fat_ref);

        // SAFETY: TODO does not work with ZST
        let box_ptr = unsafe { alloc::alloc::alloc(element_box_layout) };

        if box_ptr.is_null() {
            alloc::alloc::handle_alloc_error(element_box_layout);
        }

        let elem_size = mem::size_of_val(elem_fat_ref);
        // SAFETY: The new allocation doesn't overlap, `box_ptr` was just allocated and is non_null
        //         For `elem_ptr`, see safety comments above, the size was obtained above as well
        unsafe {
            ptr::copy_nonoverlapping(elem_ptr, box_ptr, elem_size);
        }

        // SAFETY: See above for both variables. `data.meta` is the valid metadata for the element
        let box_fat_ptr = ptr::from_raw_parts_mut(box_ptr as *mut (), data.meta);

        // We don't need to care about our memory, we can just decrement the `len` and let the old memory be, it's
        // now semantically uninitialized
        self.len -= 1;

        // SAFETY: We decremented the `len`, so no one else can get access to the element,
        //         therefore it's safe to transfer ownership to the Box here
        let return_box = unsafe { Box::from_raw(box_fat_ptr) };

        Some(return_box)
    }

    /// Get a raw ptr to an element. Be careful about casting this into a `mut &T`
    /// # SAFETY
    /// The index must be in bounds
    pub unsafe fn get_unchecked_ptr(&self, index: usize) -> *mut T {
        // SAFETY: We can assume that `index` is valid
        let data = unsafe { self.get_data(index) };

        let elem_ptr = unsafe { self.ptr.as_ptr().add(data.offset) };

        ptr::from_raw_parts_mut(elem_ptr as *mut (), data.meta)
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
            self.realloc(NonZeroUsize::new_unchecked(new_cap));
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

    /// Reallocs the `Vechonk`, setting its capacity to `size`. This will not copy any elements. This will put the `Vechonk`
    /// into an invalid state, since the `len` is still the length of the elements in the old allocation.
    ///
    /// This doesn't free any memory
    ///
    /// # Safety
    /// The caller must either set the `len` to zero, or copy the elements to the new allocation by saving
    /// `self.ptr` before calling this function.
    unsafe fn realloc(&mut self, size: NonZeroUsize) {
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

    /// Get the data for the index
    /// # Safety
    /// `index` must not be out of bounds
    unsafe fn get_data(&self, index: usize) -> PtrData<T> {
        let data_offset = self.offset_for_data(index);

        // SAFETY: The offset will always be less than `self.cap`, because we can't have more than `self.len` `PtrData`
        let data_ptr = unsafe { self.ptr.as_ptr().add(data_offset) };
        let data_ptr = data_ptr as *mut PtrData<T>;

        // SAFETY: The pointer is aligned because `self.ptr` is aligned and `data_offset` is a multiple of the alignment
        //         The value behind it is always a `PtrData<T>`
        unsafe { *data_ptr }
    }

    /// SAFETY: The allocation must be owned by `ptr` and have the length `cap`
    pub unsafe fn dealloc(cap: usize, ptr: *mut u8) {
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

    pub const fn data_section_size(&self) -> usize {
        self.len * mem::size_of::<PtrData<T>>()
    }

    const fn data_align() -> usize {
        mem::align_of::<PtrData<T>>()
    }
}
