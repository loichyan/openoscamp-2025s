use std::alloc::Layout;
use std::marker::PhantomData;
use std::ptr::NonNull;

use evering::uring::{Header as UringHeader, RawUring};

pub struct ShmHeader<A, B, Ext = ()> {
    header: UringHeader<Ext>,
    // Relative offsets of uring buffers
    buf_a: usize,
    buf_b: usize,
    // Size of the entier Uring
    size: usize,
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
        let mut size;

        let this = start.cast::<Self>();
        size = size_of::<Self>();

        let layout_a = Layout::array::<A>(header.size_a()).unwrap();
        let buf_a = align_up(size, layout_a.align());
        size = buf_a + layout_a.size();

        let layout_b = Layout::array::<B>(header.size_b()).unwrap();
        let buf_b = align_up(size, layout_b.align());
        size = buf_b + layout_b.size();

        assert!(size <= len);

        // Initialize the Uring
        unsafe {
            this.write(Self {
                header,
                buf_a,
                buf_b,
                size,
                marker: PhantomData,
            });
        }

        this
    }

    pub unsafe fn build_raw_uring(&self) -> RawUring<A, B, Ext> {
        let mut raw = RawUring::<A, B, Ext>::dangling();
        unsafe {
            let start = NonNull::from(self);
            raw.header = NonNull::new_unchecked(std::ptr::from_ref(&self.header).cast_mut());
            raw.buf_a = start.byte_add(self.buf_a).cast();
            raw.buf_b = start.byte_add(self.buf_b).cast();
        }
        raw
    }
}

const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}
