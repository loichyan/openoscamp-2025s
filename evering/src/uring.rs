use std::alloc::Layout;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

pub trait Uring {
    type Sqe;
    type Rqe;
    type Ext;

    fn send(&mut self, val: Self::Sqe) -> Result<(), Self::Sqe>;

    fn recv(&mut self) -> Option<Self::Rqe>;

    fn ext(&self) -> &Self::Ext
    where
        Self::Ext: Sync;
}

pub struct Sender<Sqe, Rqe, T = ()>(RawUring<Sqe, Rqe, T>);

pub struct Receiver<Sqe, Rqe, T = ()>(RawUring<Sqe, Rqe, T>);

unsafe impl<Sqe: Send, Rqe: Send, T: Send> Send for Sender<Sqe, Rqe, T> {}
unsafe impl<Sqe: Send, Rqe: Send, T: Send> Send for Receiver<Sqe, Rqe, T> {}

impl<Sqe, Rqe, T> Sender<Sqe, Rqe, T> {
    pub fn into_raw(self) -> RawUring<Sqe, Rqe, T> {
        let inner = RawUring {
            header: self.0.header,
            sqbuf: self.0.sqbuf,
            rqbuf: self.0.rqbuf,
            marker: PhantomData,
        };
        std::mem::forget(self);
        inner
    }

    /// # Safety
    ///
    /// The specified [`RawUring`] must be a valid value returned from
    /// [`into_raw`](Self::into_raw).
    pub unsafe fn from_raw(uring: RawUring<Sqe, Rqe, T>) -> Self {
        Self(uring)
    }
}

impl<Sqe, Rqe, T> Receiver<Sqe, Rqe, T> {
    pub fn into_raw(self) -> RawUring<Sqe, Rqe, T> {
        let inner = RawUring {
            header: self.0.header,
            sqbuf: self.0.sqbuf,
            rqbuf: self.0.rqbuf,
            marker: PhantomData,
        };
        std::mem::forget(self);
        inner
    }

    /// # Safety
    ///
    /// The specified [`RawUring`] must be a valid value returned from
    /// [`into_raw`](Self::into_raw).
    pub unsafe fn from_raw(uring: RawUring<Sqe, Rqe, T>) -> Self {
        Self(uring)
    }
}

impl<Sqe, Rqe, T> Uring for Sender<Sqe, Rqe, T> {
    type Rqe = Rqe;
    type Sqe = Sqe;
    type Ext = T;

    fn send(&mut self, val: Sqe) -> Result<(), Sqe> {
        unsafe { self.0.sq().enqueue(val) }
    }

    fn recv(&mut self) -> Option<Rqe> {
        unsafe { self.0.rq().dequeue() }
    }

    fn ext(&self) -> &T
    where
        T: Sync,
    {
        unsafe { &self.0.header.as_ref().ext }
    }
}

impl<Sqe, Rqe, T> Uring for Receiver<Sqe, Rqe, T> {
    type Rqe = Sqe;
    type Sqe = Rqe;
    type Ext = T;

    fn send(&mut self, val: Rqe) -> Result<(), Rqe> {
        unsafe { self.0.rq().enqueue(val) }
    }

    fn recv(&mut self) -> Option<Sqe> {
        unsafe { self.0.sq().dequeue() }
    }

    fn ext(&self) -> &T
    where
        T: Sync,
    {
        unsafe { &self.0.header.as_ref().ext }
    }
}

impl<Sqe, Rqe, T> Drop for Sender<Sqe, Rqe, T> {
    fn drop(&mut self) {
        unsafe { self.0.drop_in_place() }
    }
}

impl<Sqe, Rqe, T> Drop for Receiver<Sqe, Rqe, T> {
    fn drop(&mut self) {
        unsafe { self.0.drop_in_place() }
    }
}

pub struct UringHeader<T> {
    sqoff: Offsets,
    rqoff: Offsets,
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

pub struct RawUring<Sqe, Rqe, T> {
    pub header: NonNull<UringHeader<T>>,
    pub sqbuf: NonNull<Sqe>,
    pub rqbuf: NonNull<Rqe>,
    marker: PhantomData<fn(T) -> T>,
}

pub struct Queue<'a, T> {
    off: &'a Offsets,
    buf: NonNull<T>,
}

impl<Sqe, Rqe, T> RawUring<Sqe, Rqe, T> {
    unsafe fn sq(&self) -> Queue<'_, Sqe> {
        Queue {
            off: unsafe { &self.header.as_ref().sqoff },
            buf: self.sqbuf,
        }
    }

    unsafe fn rq(&self) -> Queue<'_, Rqe> {
        Queue {
            off: unsafe { &self.header.as_ref().rqoff },
            buf: self.rqbuf,
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

        debug_assert!((h.sqoff.ring_mask + 1).is_power_of_two());
        debug_assert!((h.rqoff.ring_mask + 1).is_power_of_two());
        unsafe {
            for i in h.sqoff.head.as_ptr().read()..h.sqoff.tail.as_ptr().read() {
                self.sqbuf.add(i as usize).drop_in_place();
            }
            for i in h.rqoff.head.as_ptr().read()..h.rqoff.tail.as_ptr().read() {
                self.rqbuf.add(i as usize).drop_in_place();
            }
            dealloc_buffer(self.sqbuf, h.sqoff.ring_mask as usize + 1);
            dealloc_buffer(self.rqbuf, h.rqoff.ring_mask as usize + 1);
            dealloc(self.header);
        }
    }
}

impl<T> Queue<'_, T> {
    // TODO: support enqueuing multiple entries
    unsafe fn enqueue(&self, val: T) -> Result<(), T> {
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
    unsafe fn dequeue(&self) -> Option<T> {
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
}

pub struct UringBuilder<Sqe, Rqe, T> {
    sqsize: usize,
    rqsize: usize,
    ext: T,
    marker: PhantomData<(Sqe, Rqe)>,
}

impl<Sqe, Rqe, T> UringBuilder<Sqe, Rqe, T> {
    pub fn new(ext: T) -> Self {
        Self {
            sqsize: 32,
            rqsize: 32,
            ext,
            marker: PhantomData,
        }
    }

    pub fn sqsize(&mut self, size: usize) -> &mut Self {
        assert!(size.is_power_of_two());
        self.sqsize = size;
        self
    }

    pub fn rqsize(&mut self, size: usize) -> &mut Self {
        assert!(size.is_power_of_two());
        self.rqsize = size;
        self
    }

    pub fn build(self) -> (Sender<Sqe, Rqe, T>, Receiver<Sqe, Rqe, T>) {
        let Self {
            sqsize,
            rqsize,
            ext,
            marker: _,
        } = self;

        let header;
        let sqbuf;
        let rqbuf;

        unsafe {
            header = alloc::<UringHeader<T>>();
            sqbuf = alloc_buffer(sqsize);
            rqbuf = alloc_buffer(rqsize);

            header.write(UringHeader {
                sqoff: Offsets::new(sqsize as u32),
                rqoff: Offsets::new(rqsize as u32),
                rc: AtomicU32::new(2),
                ext,
            });
        }

        let sender = Sender(RawUring {
            header,
            sqbuf,
            rqbuf,
            marker: PhantomData,
        });
        let receiver = Receiver(RawUring {
            header,
            sqbuf,
            rqbuf,
            marker: PhantomData,
        });

        (sender, receiver)
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

        let (mut sq, mut rq) = UringBuilder::new(()).build();
        std::thread::scope(|cx| {
            cx.spawn(|| {
                for i in input.iter().copied().map(DropCounter) {
                    if i.0.is_uppercase() {
                        sq.send(i).unwrap();
                    }
                }
                drop(sq);
            });
            cx.spawn(|| {
                for i in input.iter().copied().map(DropCounter) {
                    if i.0.is_lowercase() {
                        rq.send(i).unwrap();
                    }
                }
                drop(rq);
            });
        });

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), input.len() * 2);
    }

    #[test]
    fn uring_threaded() {
        let input = std::iter::repeat_with(fastrand::alphabetic)
            .take(30)
            .collect::<Vec<_>>();

        let (mut sq, mut rq) = UringBuilder::new(()).build();
        let (sq_finished, rq_finished) = (AtomicBool::new(false), AtomicBool::new(false));
        std::thread::scope(|cx| {
            cx.spawn(|| {
                let mut r = vec![];
                for i in input.iter().copied() {
                    sq.send(i).unwrap();
                    while let Some(i) = sq.recv() {
                        r.push(i);
                    }
                }
                sq_finished.store(true, Ordering::Release);
                while !rq_finished.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                while let Some(i) = sq.recv() {
                    r.push(i);
                }
                assert_eq!(r, input);
            });
            cx.spawn(|| {
                let mut r = vec![];
                for i in input.iter().copied() {
                    rq.send(i).unwrap();
                    while let Some(i) = rq.recv() {
                        r.push(i);
                    }
                }
                rq_finished.store(true, Ordering::Release);
                while !sq_finished.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                while let Some(i) = rq.recv() {
                    r.push(i);
                }
                assert_eq!(r, input);
            });
        });
    }
}
