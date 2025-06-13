use crate::op::Cancellation;
use core::cell::RefCell;
use core::mem;
use core::task::{Context, LocalWaker, Poll};
use slab::Slab;

#[derive(Clone, Copy, Debug)]
pub struct OpId(usize);

pub struct Driver<P>(RefCell<DriverInner<P>>);

// TODO: panic on drop if there are pending operations
struct DriverInner<P> {
    ops: Slab<Lifecycle<P>>,
}

enum Lifecycle<P> {
    Submitted,
    Waiting(LocalWaker),
    Completed(P),
    Cancelled(#[allow(dead_code)] Cancellation),
}

impl<P> Driver<P> {
    pub const fn new() -> Self {
        Self(RefCell::new(DriverInner { ops: Slab::new() }))
    }

    pub fn submit(&self) -> OpId {
        self.0.borrow_mut().submit()
    }

    pub fn complete(&self, id: OpId, payload: P) {
        self.0.borrow_mut().complete(id, payload)
    }

    pub(crate) fn poll(&self, id: OpId, cx: &mut Context) -> Poll<P> {
        self.0.borrow_mut().poll(id, cx)
    }

    pub(crate) fn remove(&self, id: OpId, mut callback: impl FnMut() -> Cancellation) {
        self.0.borrow_mut().remove(id, &mut callback)
    }
}

impl<P> Default for Driver<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> DriverInner<P> {
    fn submit(&mut self) -> OpId {
        OpId(self.ops.insert(Lifecycle::Submitted))
    }

    fn poll(&mut self, id: OpId, cx: &mut Context) -> Poll<P> {
        let op = self.ops.get_mut(id.0).expect("invalid driver state");
        match mem::replace(op, Lifecycle::Submitted) {
            Lifecycle::Submitted => {
                *op = Lifecycle::Waiting(cx.local_waker().clone());
                Poll::Pending
            },
            Lifecycle::Waiting(waker) if !waker.will_wake(cx.local_waker()) => {
                *op = Lifecycle::Waiting(cx.local_waker().clone());
                Poll::Pending
            },
            Lifecycle::Waiting(waker) => {
                *op = Lifecycle::Waiting(waker);
                Poll::Pending
            },
            Lifecycle::Completed(payload) => {
                // Remove this operation immediately if completed.
                self.ops.remove(id.0);
                Poll::Ready(payload)
            },
            Lifecycle::Cancelled(_) => unreachable!("invalid operation state"),
        }
    }

    fn complete(&mut self, id: OpId, payload: P) {
        let op = self.ops.get_mut(id.0).expect("invalid driver state");
        match mem::replace(op, Lifecycle::Submitted) {
            Lifecycle::Submitted => *op = Lifecycle::Completed(payload),
            Lifecycle::Waiting(waker) => {
                *op = Lifecycle::Completed(payload);
                waker.wake();
            },
            Lifecycle::Completed(_) => unreachable!("invalid operation state"),
            Lifecycle::Cancelled(_) => _ = self.ops.remove(id.0),
        }
    }

    fn remove(&mut self, id: OpId, callback: &mut dyn FnMut() -> Cancellation) {
        // The operation may have been removed inside `poll`.
        let Some(op) = self.ops.get_mut(id.0) else {
            return;
        };
        match mem::replace(op, Lifecycle::Submitted) {
            Lifecycle::Submitted | Lifecycle::Waiting(_) => *op = Lifecycle::Cancelled(callback()),
            Lifecycle::Completed(_) => _ = self.ops.remove(id.0),
            Lifecycle::Cancelled(_) => unreachable!("invalid operation state"),
        }
    }
}

pub trait DriverHandle: 'static + Unpin {
    type Payload;
    type Ref: core::ops::Deref<Target = Driver<Self::Payload>>;

    fn get(&self) -> Self::Ref;
}
impl<P: 'static> DriverHandle for alloc::rc::Weak<Driver<P>> {
    type Payload = P;
    type Ref = alloc::rc::Rc<Driver<P>>;
    fn get(&self) -> Self::Ref {
        self.upgrade().expect("not inside a valid executor")
    }
}
