// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![allow(unstable_name_collisions)]
#![allow(dead_code)]
#![allow(unused_unsafe)]

//! Memory allocation APIs

use core::mem;
use core::usize;

pub use core::alloc::{Layout, LayoutError};

fn new_layout_err() -> LayoutError {
    Layout::from_size_align(1, 3).unwrap_err()
}

pub trait UnstableLayoutMethods {
    fn padding_needed_for(&self, align: usize) -> usize;
    fn repeat(&self, n: usize) -> Result<(Layout, usize), LayoutError>;
    fn array<T>(n: usize) -> Result<Layout, LayoutError>;
}

impl UnstableLayoutMethods for Layout {
    fn padding_needed_for(&self, align: usize) -> usize {
        let len = self.size();

        // Rounded up value is:
        //   len_rounded_up = (len + align - 1) & !(align - 1);
        // and then we return the padding difference: `len_rounded_up - len`.
        //
        // We use modular arithmetic throughout:
        //
        // 1. align is guaranteed to be > 0, so align - 1 is always
        //    valid.
        //
        // 2. `len + align - 1` can overflow by at most `align - 1`,
        //    so the &-mask wth `!(align - 1)` will ensure that in the
        //    case of overflow, `len_rounded_up` will itself be 0.
        //    Thus the returned padding, when added to `len`, yields 0,
        //    which trivially satisfies the alignment `align`.
        //
        // (Of course, attempts to allocate blocks of memory whose
        // size and padding overflow in the above manner should cause
        // the allocator to yield an error anyway.)

        let len_rounded_up = len.wrapping_add(align).wrapping_sub(1) & !align.wrapping_sub(1);
        len_rounded_up.wrapping_sub(len)
    }

    fn repeat(&self, n: usize) -> Result<(Layout, usize), LayoutError> {
        let padded_size = self
            .size()
            .checked_add(self.padding_needed_for(self.align()))
            .ok_or_else(new_layout_err)?;
        let alloc_size = padded_size.checked_mul(n).ok_or_else(new_layout_err)?;

        unsafe {
            // self.align is already known to be valid and alloc_size has been
            // padded already.
            Ok((
                Layout::from_size_align_unchecked(alloc_size, self.align()),
                padded_size,
            ))
        }
    }

    fn array<T>(n: usize) -> Result<Layout, LayoutError> {
        Layout::new::<T>().repeat(n).map(|(k, offs)| {
            debug_assert!(offs == mem::size_of::<T>());
            k
        })
    }
}

#[cfg(feature = "nightly")]
pub use core_alloc::alloc::{
    handle_alloc_error, AllocError, Allocator,
};

#[cfg(not(feature = "nightly"))]
pub use shim::*;

#[cfg(not(feature = "nightly"))]
mod shim {

    use core::fmt;
    use core::ptr::{self, NonNull};

    pub use core::alloc::{Layout, LayoutError};

    pub fn handle_alloc_error(layout: Layout) -> ! {
        panic!("encountered allocation error: {:?}", layout)
    }

    /// The `AllocError` error indicates an allocation failure
    /// that may be due to resource exhaustion or to
    /// something wrong when combining the given input arguments with this
    /// allocator.
    //#[unstable(feature = "allocator_api", issue = "32838")]
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    pub struct AllocError;

    // (we need this for downstream impl of trait Error)
    //#[unstable(feature = "allocator_api", issue = "32838")]
    impl fmt::Display for AllocError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("memory allocation failed")
        }
    }

    /// An implementation of `Allocator` can allocate, grow, shrink, and deallocate arbitrary blocks of
    /// data described via [`Layout`][].
    ///
    /// `Allocator` is designed to be implemented on ZSTs, references, or smart pointers because having
    /// an allocator like `MyAlloc([u8; N])` cannot be moved, without updating the pointers to the
    /// allocated memory.
    ///
    /// Unlike [`GlobalAlloc`][], zero-sized allocations are allowed in `Allocator`. If an underlying
    /// allocator does not support this (like jemalloc) or return a null pointer (such as
    /// `libc::malloc`), this must be caught by the implementation.
    ///
    /// ### Currently allocated memory
    ///
    /// Some of the methods require that a memory block be *currently allocated* via an allocator. This
    /// means that:
    ///
    /// * the starting address for that memory block was previously returned by [`alloc`], [`grow`], or
    ///   [`shrink`], and
    ///
    /// * the memory block has not been subsequently deallocated, where blocks are either deallocated
    ///   directly by being passed to [`dealloc`] or were changed by being passed to [`grow`] or
    ///   [`shrink`] that returns `Ok`. If `grow` or `shrink` have returned `Err`, the passed pointer
    ///   remains valid.
    ///
    /// [`alloc`]: Allocator::alloc
    /// [`grow`]: Allocator::grow
    /// [`shrink`]: Allocator::shrink
    /// [`dealloc`]: Allocator::dealloc
    ///
    /// ### Memory fitting
    ///
    /// Some of the methods require that a layout *fit* a memory block. What it means for a layout to
    /// "fit" a memory block means (or equivalently, for a memory block to "fit" a layout) is that the
    /// following conditions must hold:
    ///
    /// * The block must be allocated with the same alignment as [`layout.align()`], and
    ///
    /// * The provided [`layout.size()`] must fall in the range `min ..= max`, where:
    ///   - `min` is the size of the layout most recently used to allocate the block, and
    ///   - `max` is the latest actual size returned from [`alloc`], [`grow`], or [`shrink`].
    ///
    /// [`layout.align()`]: Layout::align
    /// [`layout.size()`]: Layout::size
    ///
    /// # Safety
    ///
    /// * Memory blocks returned from an allocator must point to valid memory and retain their validity
    ///   until the instance and all of its clones are dropped,
    ///
    /// * cloning or moving the allocator must not invalidate memory blocks returned from this
    ///   allocator. A cloned allocator must behave like the same allocator, and
    ///
    /// * any pointer to a memory block which is [*currently allocated*] may be passed to any other
    ///   method of the allocator.
    ///
    /// [*currently allocated*]: #currently-allocated-memory
    //#[unstable(feature = "allocator_api", issue = "32838")]
    pub unsafe trait Allocator {
        /// Attempts to allocate a block of memory.
        ///
        /// On success, returns a [`NonNull<[u8]>`][NonNull] meeting the size and alignment guarantees of `layout`.
        ///
        /// The returned block may have a larger size than specified by `layout.size()`, and may or may
        /// not have its contents initialized.
        ///
        /// # Errors
        ///
        /// Returning `Err` indicates that either memory is exhausted or `layout` does not meet
        /// allocator's size or alignment constraints.
        ///
        /// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
        /// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
        /// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
        ///
        /// Clients wishing to abort computation in response to an allocation error are encouraged to
        /// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
        ///
        /// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
        fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError>;

        /// Behaves like `alloc`, but also ensures that the returned memory is zero-initialized.
        ///
        /// # Errors
        ///
        /// Returning `Err` indicates that either memory is exhausted or `layout` does not meet
        /// allocator's size or alignment constraints.
        ///
        /// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
        /// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
        /// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
        ///
        /// Clients wishing to abort computation in response to an allocation error are encouraged to
        /// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
        ///
        /// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
        fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            let ptr = self.allocate(layout)?;
            // SAFETY: `alloc` returns a valid memory block
            unsafe { ptr.as_non_null_ptr().as_ptr().write_bytes(0, ptr.len()) }
            Ok(ptr)
        }

        /// Deallocates the memory referenced by `ptr`.
        ///
        /// # Safety
        ///
        /// * `ptr` must denote a block of memory [*currently allocated*] via this allocator, and
        /// * `layout` must [*fit*] that block of memory.
        ///
        /// [*currently allocated*]: #currently-allocated-memory
        /// [*fit*]: #memory-fitting
        unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout);

        /// Attempts to extend the memory block.
        ///
        /// Returns a new [`NonNull<[u8]>`][NonNull] containing a pointer and the actual size of the allocated
        /// memory. The pointer is suitable for holding data described by `new_layout`. To accomplish
        /// this, the allocator may extend the allocation referenced by `ptr` to fit the new layout.
        ///
        /// If this returns `Ok`, then ownership of the memory block referenced by `ptr` has been
        /// transferred to this allocator. The memory may or may not have been freed, and should be
        /// considered unusable unless it was transferred back to the caller again via the return value
        /// of this method.
        ///
        /// If this method returns `Err`, then ownership of the memory block has not been transferred to
        /// this allocator, and the contents of the memory block are unaltered.
        ///
        /// # Safety
        ///
        /// * `ptr` must denote a block of memory [*currently allocated*] via this allocator.
        /// * `old_layout` must [*fit*] that block of memory (The `new_layout` argument need not fit it.).
        /// * `new_layout.size()` must be greater than or equal to `old_layout.size()`.
        ///
        /// [*currently allocated*]: #currently-allocated-memory
        /// [*fit*]: #memory-fitting
        ///
        /// # Errors
        ///
        /// Returns `Err` if the new layout does not meet the allocator's size and alignment
        /// constraints of the allocator, or if growing otherwise fails.
        ///
        /// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
        /// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
        /// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
        ///
        /// Clients wishing to abort computation in response to an allocation error are encouraged to
        /// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
        ///
        /// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
        unsafe fn grow(
            &self,
            ptr: NonNull<u8>,
            old_layout: Layout,
            new_layout: Layout,
        ) -> Result<NonNull<[u8]>, AllocError> {
            debug_assert!(
                new_layout.size() >= old_layout.size(),
                "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
                );

            let new_ptr = self.allocate(new_layout)?;

            // SAFETY: because `new_layout.size()` must be greater than or equal to
            // `old_layout.size()`, both the old and new memory allocation are valid for reads and
            // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
            // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
            // safe. The safety contract for `dealloc` must be upheld by the caller.
            unsafe {
                ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(), old_layout.size());
                self.deallocate(ptr, old_layout);
            }

            Ok(new_ptr)
        }

        /// Behaves like `grow`, but also ensures that the new contents are set to zero before being
        /// returned.
        ///
        /// The memory block will contain the following contents after a successful call to
        /// `grow_zeroed`:
        ///   * Bytes `0..old_layout.size()` are preserved from the original allocation.
        ///   * Bytes `old_layout.size()..old_size` will either be preserved or zeroed, depending on
        ///     the allocator implementation. `old_size` refers to the size of the memory block prior
        ///     to the `grow_zeroed` call, which may be larger than the size that was originally
        ///     requested when it was allocated.
        ///   * Bytes `old_size..new_size` are zeroed. `new_size` refers to the size of the memory
        ///     block returned by the `grow_zeroed` call.
        ///
        /// # Safety
        ///
        /// * `ptr` must denote a block of memory [*currently allocated*] via this allocator.
        /// * `old_layout` must [*fit*] that block of memory (The `new_layout` argument need not fit it.).
        /// * `new_layout.size()` must be greater than or equal to `old_layout.size()`.
        ///
        /// [*currently allocated*]: #currently-allocated-memory
        /// [*fit*]: #memory-fitting
        ///
        /// # Errors
        ///
        /// Returns `Err` if the new layout does not meet the allocator's size and alignment
        /// constraints of the allocator, or if growing otherwise fails.
        ///
        /// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
        /// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
        /// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
        ///
        /// Clients wishing to abort computation in response to an allocation error are encouraged to
        /// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
        ///
        /// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
        unsafe fn grow_zeroed(
            &self,
            ptr: NonNull<u8>,
            old_layout: Layout,
            new_layout: Layout,
        ) -> Result<NonNull<[u8]>, AllocError> {
            debug_assert!(
                new_layout.size() >= old_layout.size(),
                "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
                );

            let new_ptr = self.allocate_zeroed(new_layout)?;

            // SAFETY: because `new_layout.size()` must be greater than or equal to
            // `old_layout.size()`, both the old and new memory allocation are valid for reads and
            // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
            // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
            // safe. The safety contract for `dealloc` must be upheld by the caller.
            unsafe {
                ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(), old_layout.size());
                self.deallocate(ptr, old_layout);
            }

            Ok(new_ptr)
        }

        /// Attempts to shrink the memory block.
        ///
        /// Returns a new [`NonNull<[u8]>`][NonNull] containing a pointer and the actual size of the allocated
        /// memory. The pointer is suitable for holding data described by `new_layout`. To accomplish
        /// this, the allocator may shrink the allocation referenced by `ptr` to fit the new layout.
        ///
        /// If this returns `Ok`, then ownership of the memory block referenced by `ptr` has been
        /// transferred to this allocator. The memory may or may not have been freed, and should be
        /// considered unusable unless it was transferred back to the caller again via the return value
        /// of this method.
        ///
        /// If this method returns `Err`, then ownership of the memory block has not been transferred to
        /// this allocator, and the contents of the memory block are unaltered.
        ///
        /// # Safety
        ///
        /// * `ptr` must denote a block of memory [*currently allocated*] via this allocator.
        /// * `old_layout` must [*fit*] that block of memory (The `new_layout` argument need not fit it.).
        /// * `new_layout.size()` must be smaller than or equal to `old_layout.size()`.
        ///
        /// [*currently allocated*]: #currently-allocated-memory
        /// [*fit*]: #memory-fitting
        ///
        /// # Errors
        ///
        /// Returns `Err` if the new layout does not meet the allocator's size and alignment
        /// constraints of the allocator, or if shrinking otherwise fails.
        ///
        /// Implementations are encouraged to return `Err` on memory exhaustion rather than panicking or
        /// aborting, but this is not a strict requirement. (Specifically: it is *legal* to implement
        /// this trait atop an underlying native allocation library that aborts on memory exhaustion.)
        ///
        /// Clients wishing to abort computation in response to an allocation error are encouraged to
        /// call the [`handle_alloc_error`] function, rather than directly invoking `panic!` or similar.
        ///
        /// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
        unsafe fn shrink(
            &self,
            ptr: NonNull<u8>,
            old_layout: Layout,
            new_layout: Layout,
        ) -> Result<NonNull<[u8]>, AllocError> {
            debug_assert!(
                new_layout.size() <= old_layout.size(),
                "`new_layout.size()` must be smaller than or equal to `old_layout.size()`"
                );

            let new_ptr = self.allocate(new_layout)?;

            // SAFETY: because `new_layout.size()` must be lower than or equal to
            // `old_layout.size()`, both the old and new memory allocation are valid for reads and
            // writes for `new_layout.size()` bytes. Also, because the old allocation wasn't yet
            // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
            // safe. The safety contract for `dealloc` must be upheld by the caller.
            unsafe {
                ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(), new_layout.size());
                self.deallocate(ptr, old_layout);
            }

            Ok(new_ptr)
        }

        /// Creates a "by reference" adaptor for this instance of `Allocator`.
        ///
        /// The returned adaptor also implements `Allocator` and will simply borrow this.
        #[inline(always)]
        fn by_ref(&self) -> &Self {
            self
        }
    }
}
