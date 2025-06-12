use std::alloc::Layout;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

pub trait Uring {
    type A;
    type B;
    type Ext;

    fn send(&mut self, val: Self::A) -> Result<(), Self::A>;

    fn recv(&mut self) -> Option<Self::B>;

    fn ext(&self) -> &Self::Ext
    where
        Self::Ext: Sync;
}

pub type Sender<Sqe, Rqe, T = ()> = UringA<Sqe, Rqe, T>;
pub type Receiver<Sqe, Rqe, T = ()> = UringB<Sqe, Rqe, T>;

pub struct UringA<A, B, T = ()>(RawUring<A, B, T>);
pub struct UringB<A, B, T = ()>(RawUring<A, B, T>);

unsafe impl<A: Send, B: Send, T: Send> Send for UringA<A, B, T> {}
unsafe impl<A: Send, B: Send, T: Send> Send for UringB<A, B, T> {}

impl<A, B, T> UringA<A, B, T> {
    pub fn into_raw(self) -> RawUring<A, B, T> {
        let inner = RawUring {
            header: self.0.header,
            buf_a: self.0.buf_a,
            buf_b: self.0.buf_b,
            marker: PhantomData,
        };
        std::mem::forget(self);
        inner
    }

    /// # Safety
    ///
    /// The specified [`RawUring`] must be a valid value returned from
    /// [`into_raw`](Self::into_raw).
    pub unsafe fn from_raw(uring: RawUring<A, B, T>) -> Self {
        Self(uring)
    }
}

impl<A, B, T> UringB<A, B, T> {
    pub fn into_raw(self) -> RawUring<A, B, T> {
        let inner = RawUring {
            header: self.0.header,
            buf_a: self.0.buf_a,
            buf_b: self.0.buf_b,
            marker: PhantomData,
        };
        std::mem::forget(self);
        inner
    }

    /// # Safety
    ///
    /// The specified [`RawUring`] must be a valid value returned from
    /// [`into_raw`](Self::into_raw).
    pub unsafe fn from_raw(uring: RawUring<A, B, T>) -> Self {
        Self(uring)
    }
}

impl<A, B, T> Uring for UringA<A, B, T> {
    type A = A;
    type B = B;
    type Ext = T;

    fn send(&mut self, val: A) -> Result<(), A> {
        unsafe { self.0.queue_a().enqueue(val) }
    }

    fn recv(&mut self) -> Option<B> {
        unsafe { self.0.queue_b().dequeue() }
    }

    fn ext(&self) -> &T
    where
        T: Sync,
    {
        unsafe { &self.0.header.as_ref().ext }
    }
}

impl<A, B, T> Uring for UringB<A, B, T> {
    type A = B;
    type B = A;
    type Ext = T;

    fn send(&mut self, val: B) -> Result<(), B> {
        unsafe { self.0.queue_b().enqueue(val) }
    }

    fn recv(&mut self) -> Option<A> {
        unsafe { self.0.queue_a().dequeue() }
    }

    fn ext(&self) -> &T
    where
        T: Sync,
    {
        unsafe { &self.0.header.as_ref().ext }
    }
}

impl<A, B, T> Drop for UringA<A, B, T> {
    fn drop(&mut self) {
        unsafe { self.0.drop_in_place() }
    }
}

impl<A, B, T> Drop for UringB<A, B, T> {
    fn drop(&mut self) {
        unsafe { self.0.drop_in_place() }
    }
}

pub struct UringHeader<T> {
    off_a: Offsets,
    off_b: Offsets,
    rc: AtomicU32,
    ext: T,
}

struct Offsets {
    head: AtomicU32,
    tail: AtomicU32,
    ring_mask: u32,
}

impl Offsets {
    fn new(size: u32) -> Self {
        debug_assert!(size.is_power_of_two());
        Self {
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            ring_mask: size - 1,
        }
    }
}

pub struct RawUring<A, B, T> {
    pub header: NonNull<UringHeader<T>>,
    pub buf_a: NonNull<A>,
    pub buf_b: NonNull<B>,
    marker: PhantomData<fn(T) -> T>,
}

pub struct Queue<'a, T> {
    off: &'a Offsets,
    buf: NonNull<T>,
}

impl<A, B, T> RawUring<A, B, T> {
    unsafe fn queue_a(&self) -> Queue<'_, A> {
        Queue {
            off: unsafe { &self.header.as_ref().off_a },
            buf: self.buf_a,
        }
    }

    unsafe fn queue_b(&self) -> Queue<'_, B> {
        Queue {
            off: unsafe { &self.header.as_ref().off_b },
            buf: self.buf_b,
        }
    }

    unsafe fn drop_in_place(&mut self) {
        let h = unsafe { self.header.as_ref() };
        // `Release` enforeces any use of the data to happen before here.
        if h.rc.fetch_sub(1, Ordering::Release) != 1 {
            return;
        }
        // `Acquire` enforces the deletion of the data to happen after here.
        std::sync::atomic::fence(Ordering::Acquire);

        unsafe {
            self.queue_a().drop_in_place();
            self.queue_b().drop_in_place();
            dealloc(self.header);
        }
    }
}

impl<T> Queue<'_, T> {
    // TODO: support enqueuing multiple entries
    unsafe fn enqueue(&mut self, val: T) -> Result<(), T> {
        let Self { off, buf } = self;
        debug_assert!((off.ring_mask + 1).is_power_of_two());

        let tail = off.tail.load(Ordering::Relaxed);
        let next_tail = tail.wrapping_add(1) & off.ring_mask;
        if next_tail == off.head.load(Ordering::Acquire) {
            return Err(val);
        }

        unsafe { buf.add(tail as usize).write(val) };
        off.tail.store(next_tail, Ordering::Release);

        Ok(())
    }

    // TODO: support dequeuing all available entries
    unsafe fn dequeue(&mut self) -> Option<T> {
        let Self { off, buf } = self;
        debug_assert!((off.ring_mask + 1).is_power_of_two());

        let head = off.head.load(Ordering::Relaxed);
        if head == off.tail.load(Ordering::Acquire) {
            return None;
        }
        let next_head = head.wrapping_add(1) & off.ring_mask;

        let val = unsafe { buf.add(head as usize).read() };
        off.head.store(next_head, Ordering::Release);

        Some(val)
    }

    unsafe fn drop_in_place(&mut self) {
        debug_assert!((self.off.ring_mask + 1).is_power_of_two());
        unsafe {
            for i in self.off.head.as_ptr().read()..self.off.tail.as_ptr().read() {
                self.buf.add(i as usize).drop_in_place();
            }
            dealloc_buffer(self.buf, self.off.ring_mask as usize + 1);
        }
    }
}

pub struct UringBuilder<A, B, T> {
    size_a: usize,
    size_b: usize,
    ext: T,
    marker: PhantomData<(A, B)>,
}

impl<A, B, T> UringBuilder<A, B, T> {
    pub fn new(ext: T) -> Self {
        Self {
            size_a: 32,
            size_b: 32,
            ext,
            marker: PhantomData,
        }
    }

    pub fn size_a(&mut self, size: usize) -> &mut Self {
        assert!(size.is_power_of_two());
        self.size_a = size;
        self
    }

    pub fn size_b(&mut self, size: usize) -> &mut Self {
        assert!(size.is_power_of_two());
        self.size_b = size;
        self
    }

    pub fn build(self) -> (UringA<A, B, T>, UringB<A, B, T>) {
        let Self {
            size_a,
            size_b,
            ext,
            marker: _,
        } = self;

        let header;
        let buf_a;
        let buf_b;

        unsafe {
            header = alloc::<UringHeader<T>>();
            buf_a = alloc_buffer(size_a);
            buf_b = alloc_buffer(size_b);

            header.write(UringHeader {
                off_a: Offsets::new(size_a as u32),
                off_b: Offsets::new(size_b as u32),
                rc: AtomicU32::new(2),
                ext,
            });
        }

        let ring_a = UringA(RawUring {
            header,
            buf_a,
            buf_b,
            marker: PhantomData,
        });
        let ring_b = UringB(RawUring {
            header,
            buf_a,
            buf_b,
            marker: PhantomData,
        });

        (ring_a, ring_b)
    }
}

unsafe fn alloc_buffer<T>(size: usize) -> NonNull<T> {
    let layout = Layout::array::<T>(size).unwrap();
    NonNull::new(unsafe { std::alloc::alloc(layout) })
        .unwrap_or_else(|| std::alloc::handle_alloc_error(layout))
        .cast()
}

unsafe fn alloc<T>() -> NonNull<T> {
    let layout = Layout::new::<T>();
    NonNull::new(unsafe { std::alloc::alloc(layout) })
        .unwrap_or_else(|| std::alloc::handle_alloc_error(layout))
        .cast()
}

unsafe fn dealloc_buffer<T>(ptr: NonNull<T>, size: usize) {
    let layout = Layout::array::<T>(size).unwrap();
    unsafe { std::alloc::dealloc(ptr.as_ptr().cast(), layout) }
}

unsafe fn dealloc<T>(ptr: NonNull<T>) {
    let layout = Layout::new::<T>();
    unsafe { std::alloc::dealloc(ptr.as_ptr().cast(), layout) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    #[test]
    fn uring_drop() {
        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        #[derive(Debug)]
        struct DropCounter(char);
        impl Drop for DropCounter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        let input = std::iter::repeat_with(fastrand::alphabetic)
            .take(30)
            .collect::<Vec<_>>();

        let (mut qa, mut qb) = UringBuilder::new(()).build();
        std::thread::scope(|cx| {
            cx.spawn(|| {
                for i in input.iter().copied().map(DropCounter) {
                    if i.0.is_uppercase() {
                        qa.send(i).unwrap();
                    } else {
                        _ = qa.recv();
                    }
                }
                drop(qa);
            });
            cx.spawn(|| {
                for i in input.iter().copied().map(DropCounter) {
                    if i.0.is_lowercase() {
                        qb.send(i).unwrap();
                    } else {
                        _ = qb.recv();
                    }
                }
                drop(qb);
            });
        });

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), input.len() * 2);
    }

    #[test]
    fn uring_threaded() {
        let input = std::iter::repeat_with(fastrand::alphabetic)
            .take(30)
            .collect::<Vec<_>>();

        let (mut qa, mut qb) = UringBuilder::new(()).build();
        let (qa_finished, qb_finished) = (AtomicBool::new(false), AtomicBool::new(false));
        std::thread::scope(|cx| {
            cx.spawn(|| {
                let mut r = vec![];
                for i in input.iter().copied() {
                    qa.send(i).unwrap();
                    while let Some(i) = qa.recv() {
                        r.push(i);
                    }
                }
                qa_finished.store(true, Ordering::Release);
                while !qb_finished.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                while let Some(i) = qa.recv() {
                    r.push(i);
                }
                assert_eq!(r, input);
            });
            cx.spawn(|| {
                let mut r = vec![];
                for i in input.iter().copied() {
                    qb.send(i).unwrap();
                    while let Some(i) = qb.recv() {
                        r.push(i);
                    }
                }
                qb_finished.store(true, Ordering::Release);
                while !qa_finished.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                while let Some(i) = qb.recv() {
                    r.push(i);
                }
                assert_eq!(r, input);
            });
        });
    }
}
