mod boxed;

use std::alloc::Layout;
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::num::{NonZero, NonZeroUsize};
use std::os::fd::BorrowedFd;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use evering::uring::{Header as UringHeader, RawUring};
use rlsf::Tlsf;

pub use self::boxed::{ShmBox, init_client, init_server};
use crate::Result;

/// [`ShmHeader`] contains necessary metadata of a shared memory region.
///
/// Memory layout of the entire shared memory is illustrated as below,
///
/// ```svgbob
/// .-----------------------------------------------------------------------------.
/// |                   |               |                   |                     |
/// | [1] uring offsets | [2] allocator | [3] uring buffers | [4] free memory ... |
/// | ^                 |               |                   |                   ^ |
/// '-|-------------------------------------------------------------------------|-'
///   '-- start of the shared memory (page aligned)                             |
///                                                  end of the shared memory --'
/// ```
///
/// 1. Uring offsets are used to build [`RawUring`].
/// 2. Each shared memory region comes with one single-thread allocator.
///    Typically, it will be taken by the client after initialization.
/// 3. The submitted requests and responses are stored in these buffers.
/// 4. The rest of the shared memory are managed by the allocator. [`ShmBox`]
///    provides similar APIS to [`Box`], but it is allocated and deallocated by
///    the shared memory [`Allocator`] instead of the global allocator.
pub struct ShmHeader<A = crate::op::Sqe, B = crate::op::Rqe, Ext = ()> {
    header: UringHeader<Ext>,
    // Relative offsets of uring buffers
    buf_a: usize,
    buf_b: usize,
    allocator_taken: AtomicBool,
    allocator: Allocator, // Max block size: 32 << 24 = 512MB
    free_memory: (usize, usize),
    marker: PhantomData<(A, B)>,
}

impl<A, B, Ext> ShmHeader<A, B, Ext> {
    /// # Safety
    ///
    /// The given `fd` must be valid for the remaining lifetime of the running
    /// program.
    pub unsafe fn create(
        fd: BorrowedFd,
        size: usize,
        header: UringHeader<Ext>,
    ) -> Result<NonNull<Self>> {
        // Calculate offsets
        let mut cur = size_of::<Self>();

        let layout_a = Layout::array::<A>(header.size_a()).unwrap();
        let buf_a = align_up(cur, layout_a.align());
        cur = buf_a + layout_a.size();

        let layout_b = Layout::array::<B>(header.size_b()).unwrap();
        let buf_b = align_up(cur, layout_b.align());
        cur = buf_b + layout_b.size();

        assert!(cur < size, "capacity of shared memory is too small");

        // Initialize shared memory and the uring buffers
        unsafe {
            shm_grow(fd, size)?;
            let this = shm_mmap(fd, size, 0)?.cast::<Self>();

            this.write(Self {
                header,
                buf_a,
                buf_b,
                allocator_taken: AtomicBool::new(false),
                allocator: Allocator::new(),
                free_memory: (cur, size),
                marker: PhantomData,
            });

            Ok(this)
        }
    }

    /// # Safety
    ///
    /// The given `fd` must be valid for the remaining lifetime of the running
    /// program.
    pub unsafe fn open(fd: BorrowedFd, size: usize) -> Result<NonNull<Self>> {
        assert_eq!(
            nix::sys::stat::fstat(fd)
                .context("failed to read shmfd")?
                .st_size as i64,
            size as i64
        );
        unsafe { shm_mmap(fd, size, 0).map(NonNull::cast) }
    }

    /// # Safety
    ///
    /// The supplied `ptr` and `size` must match the previous `mmap` call.
    pub unsafe fn close(ptr: NonNull<Self>, size: usize) -> Result<()> {
        unsafe {
            nix::sys::mman::munmap(ptr.cast(), size).context("failed to munmap shared memory")
        }
    }

    pub fn build_raw_uring(&self) -> RawUring<A, B, Ext> {
        let mut raw = RawUring::<A, B, Ext>::dangling();
        unsafe {
            let start = self.start_ptr();
            raw.header = NonNull::from(&self.header);
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
            let (data_start, data_end) = self.free_memory;
            let data = self.start_ptr().byte_add(data_start);
            let block = NonNull::slice_from_raw_parts(data, data_end - data_start);

            tracing::info!(
                "added free memory, addr={data:#x?}, size={}",
                bytesize::ByteSize(block.len() as u64).display().iec_short()
            );
            self.allocator
                .tlsf
                .borrow_mut()
                .append_free_block_ptr(block);
        }
        &self.allocator
    }

    pub fn get_shm<T: ?Sized>(&self, ptr: NonNull<T>) -> ShmToken<T> {
        let start = self.start_addr().get();
        let addr = ptr.addr().get();
        assert!(addr > start);
        let shm = NonZeroUsize::new(addr - start).unwrap();
        ShmToken(ptr.with_addr(shm))
    }

    /// # Safety
    ///
    /// The given `shm` must belong to this memory region.
    pub fn get_ptr<T: ?Sized>(&self, shm: ShmToken<T>) -> NonNull<T> {
        let start = self.start_addr().get();
        unsafe { shm.0.byte_add(start) }
    }

    fn start_addr(&self) -> NonZeroUsize {
        self.start_ptr().addr()
    }

    fn start_ptr(&self) -> NonNull<u8> {
        NonNull::from(self).cast()
    }
}

pub struct Allocator {
    /// Max block size is `32 << 24 = 512MB`
    tlsf: RefCell<Tlsf<'static, u32, u32, 24, 8>>,
}

impl Allocator {
    const fn new() -> Self {
        Self {
            tlsf: RefCell::new(Tlsf::new()),
        }
    }

    pub fn alloc<T>(&self, val: T) -> NonNull<T> {
        unsafe {
            let mut ptr = self.alloc_uninit();
            ptr.as_mut().write(val);
            ptr.cast()
        }
    }

    pub fn alloc_uninit<T>(&self) -> NonNull<MaybeUninit<T>> {
        unsafe { self.alloc_raw(Layout::new::<T>()).cast() }
    }

    pub fn alloc_copied_slice<T: Copy>(&self, src: &[T]) -> NonNull<[T]> {
        unsafe {
            let mut ptr = self.alloc_uninit_slice(src.len());
            let src_uninit = src as *const [T] as *const [MaybeUninit<T>];
            ptr.as_mut().copy_from_slice(&*src_uninit);
            NonNull::new_unchecked(ptr.as_ptr() as *mut [T])
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
        unsafe { self.dealloc_raw(ptr.cast(), Layout::for_value(ptr.as_ref())) }
    }

    unsafe fn alloc_raw(&self, layout: Layout) -> NonNull<u8> {
        assert_ne!(layout.size(), 0);
        self.tlsf
            .borrow_mut()
            .allocate(layout)
            .unwrap_or_else(|| panic!("failed to allocate in shared memory"))
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

unsafe fn shm_mmap(fd: BorrowedFd, len: usize, offset: usize) -> Result<NonNull<u8>> {
    use nix::sys::mman::{MapFlags, ProtFlags};
    unsafe {
        nix::sys::mman::mmap(
            None,
            NonZero::new(len).unwrap(),
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            fd,
            offset as i64,
        )
        .map(NonNull::cast::<u8>)
        .context("failed to mmap shared memory")
    }
}

fn shm_grow(fd: BorrowedFd, new_len: usize) -> Result<()> {
    nix::unistd::ftruncate(fd, new_len as i64).context("failed to grow shared memory")
}

const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}
