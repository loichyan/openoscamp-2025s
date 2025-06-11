use crate::op::{Cancellation, Lifecycle};
use slab::Slab;
use std::cell::RefCell;
use std::mem;
use std::task::{Context, Poll};

#[derive(Clone, Copy, Debug)]
pub struct OpId(usize);

pub struct Driver<Payload>(RefCell<DriverInner<Payload>>);

// TODO: panic on drop if there are pending operations
struct DriverInner<Payload> {
    ops: Slab<Lifecycle<Payload>>,
}

impl<Payload> Driver<Payload> {
    pub const fn new() -> Self {
        Self(RefCell::new(DriverInner { ops: Slab::new() }))
    }

    pub fn submit(&self) -> OpId {
        self.0.borrow_mut().submit()
    }

    pub fn complete(&self, id: OpId, payload: Payload) {
        self.0.borrow_mut().complete(id, payload)
    }

    pub(crate) fn poll(&self, id: OpId, cx: &mut Context) -> Poll<Payload> {
        self.0.borrow_mut().poll(id, cx)
    }

    pub(crate) fn remove(&self, id: OpId, mut callback: impl FnMut() -> Cancellation) {
        self.0.borrow_mut().remove(id, &mut callback)
    }
}

impl<Payload> Default for Driver<Payload> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Payload> DriverInner<Payload> {
    fn submit(&mut self) -> OpId {
        OpId(self.ops.insert(Lifecycle::Submitted))
    }

    fn poll(&mut self, id: OpId, cx: &mut Context) -> Poll<Payload> {
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

    fn complete(&mut self, id: OpId, payload: Payload) {
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

pub trait DriverHandle: 'static + Unpin + Sized {
    type Payload;
    type Ref: std::ops::Deref<Target = Driver<Self::Payload>>;

    fn get(&self) -> Self::Ref;
}
