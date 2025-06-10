use std::sync::atomic::AtomicUsize;

pub struct Queue<T> {
    buf: Box<[T]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<T> Queue<T> {
    pub fn enqueue(&mut self, _item: T) {
        todo!()
    }

    pub fn dequeue(&mut self) -> Option<T> {
        todo!()
    }
}
