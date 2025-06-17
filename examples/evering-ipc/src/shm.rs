mod boxed;

use std::alloc::Layout;
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::num::NonZeroUsize;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use evering::uring::{Header as UringHeader, RawUring};
use rlsf::Tlsf;

pub use self::boxed::{ShmBox, init_client, init_server};

pub struct ShmHeader<A = crate::op::Sqe, B = crate::op::Rqe, Ext = ()> {
    header: UringHeader<Ext>,
    allocator_taken: AtomicBool,
    allocator: Allocator, // Max block size: 32 << 24 = 512MB
    // Relative offsets of uring buffers
    buf_a: usize,
    buf_b: usize,
    /// The offset of the free memory block
    free_memory: usize,
    /// The count of total bytes available
    len: usize,
    marker: PhantomData<(A, B)>,
}

impl<A, B, Ext> ShmHeader<A, B, Ext> {
    pub unsafe fn init(start: NonNull<u8>, len: usize, header: UringHeader<Ext>) -> NonNull<Self> {
        let page_size = 4096; // TODO: use sysconf(2)

        // Check alignments
        assert!(start.addr().get() & (page_size - 1) == 0);
        assert!(align_of::<Self>() < page_size);
        assert!(align_of::<A>() < page_size);
        assert!(align_of::<B>() < page_size);

        // Check overflows
        let mut end;

        let this = start.cast::<Self>();
        end = size_of::<Self>();

        let layout_a = Layout::array::<A>(header.size_a()).unwrap();
        let buf_a = align_up(end, layout_a.align());
        end = buf_a + layout_a.size();

        let layout_b = Layout::array::<B>(header.size_b()).unwrap();
        let buf_b = align_up(end, layout_b.align());
        end = buf_b + layout_b.size();

        assert!(end <= len);

        // Initialize the Uring
        unsafe {
            this.write(Self {
                header,
                allocator_taken: AtomicBool::new(false),
                allocator: Allocator::new(),
                buf_a,
                buf_b,
                free_memory: end,
                len,
                marker: PhantomData,
            });
        }

        this
    }

    pub fn build_raw_uring(&self) -> RawUring<A, B, Ext> {
        let mut raw = RawUring::<A, B, Ext>::dangling();
        unsafe {
            let start = NonNull::from(self);
            raw.header = NonNull::new_unchecked(std::ptr::from_ref(&self.header).cast_mut());
            raw.buf_a = start.byte_add(self.buf_a).cast();
            raw.buf_b = start.byte_add(self.buf_b).cast();
        }
        raw
    }

    pub fn get_allocator(&self) -> &Allocator {
        if self.allocator_taken.swap(true, Ordering::Acquire) {
            panic!("allocator has been taken");
        }
        unsafe {
            let start = NonNull::from(self).cast::<u8>();
            let data = start.byte_add(self.free_memory);
            let block = NonNull::slice_from_raw_parts(data, self.len - self.free_memory);
            tracing::info!(
                "added free memory, start={data:#x?}, length={}KB",
                block.len() / 1024
            );
            self.allocator
                .tlsf
                .borrow_mut()
                .append_free_block_ptr(block);
            &self.allocator
        }
    }

    fn start_addr(&self) -> usize {
        std::ptr::from_ref(self).addr()
    }

    pub fn get_shm<T: ?Sized>(&self, ptr: NonNull<T>) -> ShmToken<T> {
        let start = self.start_addr();
        let addr = ptr.addr().get();
        assert!(addr > start);
        let shm = NonZeroUsize::new(addr - start).unwrap();
        ShmToken(ptr.with_addr(shm))
    }

    /// # Safety
    ///
    /// The given `shm` must belong to this memory region.
    pub fn get_ptr<T: ?Sized>(&self, shm: ShmToken<T>) -> NonNull<T> {
        let start = self.start_addr();
        unsafe { shm.0.byte_add(start) }
    }
}

pub struct Allocator {
    tlsf: RefCell<Tlsf<'static, u32, u32, 24, 8>>, // max block size: 32 << 24 = 512MB
}

impl Allocator {
    pub const fn new() -> Self {
        Self {
            tlsf: RefCell::new(Tlsf::new()),
        }
    }

    pub fn alloc<T>(&self, val: T) -> NonNull<T> {
        unsafe {
            let mut ptr = self.alloc_uninit();
            ptr.as_mut().write(val);
            std::mem::transmute(ptr)
        }
    }

    pub fn alloc_uninit<T>(&self) -> NonNull<MaybeUninit<T>> {
        unsafe { self.alloc_raw(Layout::new::<T>()).cast() }
    }

    pub fn alloc_copied_slice<T: Copy>(&self, src: &[T]) -> NonNull<[T]> {
        unsafe {
            let mut ptr = self.alloc_uninit_slice(src.len());
            let src_uninit: &[MaybeUninit<T>] = std::mem::transmute(src);
            ptr.as_mut().copy_from_slice(src_uninit);
            std::mem::transmute(ptr)
        }
    }

    pub fn alloc_uninit_slice<T>(&self, n: usize) -> NonNull<[MaybeUninit<T>]> {
        unsafe {
            let data = self.alloc_raw(Layout::array::<T>(n).unwrap());
            NonNull::slice_from_raw_parts(data.cast(), n)
        }
    }

    /// # Safety
    ///
    /// The given `ptr` must belong to this allocator.
    pub unsafe fn dealloc<T: ?Sized>(&self, ptr: NonNull<T>) {
        unsafe { self.dealloc_raw(ptr.cast(), Layout::for_value_raw(ptr.as_ptr())) }
    }

    unsafe fn alloc_raw(&self, layout: Layout) -> NonNull<u8> {
        assert_ne!(layout.size(), 0);
        let mut tlsf = self.tlsf.borrow_mut();
        tlsf.allocate(layout)
            .expect("no shared memory available for allocation")
    }

    unsafe fn dealloc_raw(&self, ptr: NonNull<u8>, layout: Layout) {
        assert_ne!(layout.size(), 0);
        unsafe { self.tlsf.borrow_mut().deallocate(ptr, layout.align()) }
    }
}

pub struct ShmToken<T: ?Sized>(NonNull<T>);

impl<T: ?Sized> ShmToken<T> {
    pub fn as_ptr(&self) -> NonNull<T> {
        boxed::ShmHandle::get().get_ptr(*self)
    }
}

impl<T: ?Sized> fmt::Debug for ShmToken<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ShmToken")
            .field(&self.0.cast::<()>())
            .finish()
    }
}

impl<T: ?Sized> Clone for ShmToken<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Copy for ShmToken<T> {}

const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}
