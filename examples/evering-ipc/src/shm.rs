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

pub struct ShmHeader<A, B, Ext = ()> {
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

    pub fn take_allocator(&self) -> &Allocator {
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

    pub fn get_addr<T: ?Sized>(&self, boxed: &ShmBox<T>) -> ShmAddr<T> {
        let start_addr = self.start_addr();
        let boxed_addr = boxed.0.addr().get();
        assert!(boxed_addr > start_addr);
        let addr = NonZeroUsize::new(boxed_addr - start_addr).unwrap();
        ShmAddr(boxed.0.with_addr(addr))
    }

    pub unsafe fn get_ptr<T: ?Sized>(&self, addr: ShmAddr<T>) -> NonNull<T> {
        let start_addr = self.start_addr();
        unsafe { addr.0.byte_add(start_addr) }
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

    pub unsafe fn alloc<T>(&self, val: T) -> ShmBox<T> {
        unsafe {
            let mut boxed = self.alloc_uninit();
            boxed.write(val);
            boxed.assume_init()
        }
    }

    pub unsafe fn alloc_uninit<T>(&self) -> ShmBox<MaybeUninit<T>> {
        ShmBox(unsafe { self.alloc_raw(Layout::new::<T>()).cast() })
    }

    pub unsafe fn alloc_array_copied<T: Copy>(&self, src: &[T]) -> ShmBox<[T]> {
        unsafe {
            let mut boxed = self.alloc_array_uninit(src.len());
            let src_uninit: &[MaybeUninit<T>] = std::mem::transmute(src);
            boxed.copy_from_slice(src_uninit);
            boxed.assume_init()
        }
    }

    pub unsafe fn alloc_array_uninit<T>(&self, n: usize) -> ShmBox<[MaybeUninit<T>]> {
        ShmBox(unsafe {
            let data = self.alloc_raw(Layout::array::<T>(n).unwrap());
            NonNull::slice_from_raw_parts(data.cast(), n)
        })
    }

    pub unsafe fn dealloc<T: ?Sized>(&self, boxed: ShmBox<T>) {
        let ShmBox(ptr) = boxed;
        unsafe { self.dealloc_raw(ptr.cast(), Layout::for_value(ptr.as_ref())) }
    }

    unsafe fn alloc_raw(&self, layout: Layout) -> NonNull<u8> {
        let mut tlsf = self.tlsf.borrow_mut();
        tlsf.allocate(layout)
            .expect("no shared memory available for allocation")
    }

    unsafe fn dealloc_raw(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { self.tlsf.borrow_mut().deallocate(ptr, layout.align()) }
    }
}

pub struct ShmAddr<T: ?Sized>(NonNull<T>);

impl<T: ?Sized> fmt::Debug for ShmAddr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.cast::<()>().fmt(f)
    }
}

impl<T: ?Sized> Clone for ShmAddr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Copy for ShmAddr<T> {}

pub struct ShmBox<T: ?Sized>(NonNull<T>);

impl<T> ShmBox<MaybeUninit<T>> {
    pub unsafe fn assume_init(self) -> ShmBox<T> {
        ShmBox(self.0.cast())
    }
}

impl<T> ShmBox<[MaybeUninit<T>]> {
    pub unsafe fn assume_init(self) -> ShmBox<[T]> {
        ShmBox(unsafe { std::mem::transmute::<NonNull<[MaybeUninit<T>]>, NonNull<[T]>>(self.0) })
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for ShmBox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        T::fmt(self, f)
    }
}

impl<T: ?Sized> std::ops::Deref for ShmBox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> std::ops::DerefMut for ShmBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}

const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}
