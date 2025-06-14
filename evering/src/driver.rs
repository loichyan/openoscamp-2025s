#![doc = include_str!("driver.md")]

use crate::op::Cancellation;
use core::cell::RefCell;
use core::mem;
use core::task::{Context, LocalWaker, Poll};
use slab::Slab;

#[derive(Clone, Copy, Debug)]
pub struct OpId(usize);

pub struct Driver<P, Ext = ()>(RefCell<DriverInner<P, Ext>>);

// TODO: panic on drop if there are pending operations
struct DriverInner<P, Ext> {
    ops: Slab<RawOp<P, Ext>>,
}

struct RawOp<P, Ext> {
    state: Lifecycle<P>,
    ext: Ext,
}

enum Lifecycle<P> {
    Submitted,
    Waiting(LocalWaker),
    Completed(P),
    Cancelled(#[allow(dead_code)] Cancellation),
}

impl<P, Ext> Driver<P, Ext> {
    pub const fn new() -> Self {
        Self(RefCell::new(DriverInner { ops: Slab::new() }))
    }

    pub fn contains(&self, id: OpId) -> bool {
        self.0.borrow().ops.contains(id.0)
    }

    pub fn submit(&self) -> OpId
    where
        Ext: Default,
    {
        self.submit_ext(Ext::default())
    }

    pub fn submit_ext(&self, ext: Ext) -> OpId {
        self.0.borrow_mut().submit(ext)
    }

    /// Completes a operation. It returns the given `payload` as an [`Err`] if
    /// the specified operation has been cancelled.
    ///
    /// The given `id` is always recycled even if the corresponding operation is
    /// cancelled.
    pub fn complete(&self, id: OpId, payload: P) -> Result<(), P> {
        self.0
            .borrow_mut()
            .complete(id, payload)
            .map_err(|(p, _)| p)
    }

    /// Completes a operation with the submitted extension.
    ///
    /// This function is similar to [`complete`](Self::complete), see its
    /// documentation for more details.
    pub fn complete_ext(&self, id: OpId, payload: P) -> Result<(), (P, Ext)> {
        self.0.borrow_mut().complete(id, payload)
    }

    pub(crate) fn poll(&self, id: OpId, cx: &mut Context) -> Poll<(P, Ext)> {
        self.0.borrow_mut().poll(id, cx)
    }

    pub(crate) fn remove(&self, id: OpId, mut callback: impl FnMut() -> Cancellation) {
        self.0.borrow_mut().remove(id, &mut callback)
    }
}

impl<P, Ext> Default for Driver<P, Ext>
where
    Ext: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P, Ext> DriverInner<P, Ext> {
    fn submit(&mut self, ext: Ext) -> OpId {
        OpId(self.ops.insert(RawOp {
            state: Lifecycle::Submitted,
            ext,
        }))
    }

    fn poll(&mut self, id: OpId, cx: &mut Context) -> Poll<(P, Ext)> {
        let op = self.ops.get_mut(id.0).expect("invalid driver state");
        match mem::replace(&mut op.state, Lifecycle::Submitted) {
            Lifecycle::Submitted => {
                op.state = Lifecycle::Waiting(cx.local_waker().clone());
                Poll::Pending
            },
            Lifecycle::Waiting(waker) if !waker.will_wake(cx.local_waker()) => {
                op.state = Lifecycle::Waiting(cx.local_waker().clone());
                Poll::Pending
            },
            Lifecycle::Waiting(waker) => {
                op.state = Lifecycle::Waiting(waker);
                Poll::Pending
            },
            Lifecycle::Completed(payload) => {
                // Remove this operation immediately if completed.
                let op = self.ops.remove(id.0);
                Poll::Ready((payload, op.ext))
            },
            Lifecycle::Cancelled(_) => unreachable!("invalid operation state"),
        }
    }

    fn complete(&mut self, id: OpId, payload: P) -> Result<(), (P, Ext)> {
        let op = self.ops.get_mut(id.0).expect("invalid driver state");
        match mem::replace(&mut op.state, Lifecycle::Submitted) {
            Lifecycle::Submitted => {
                op.state = Lifecycle::Completed(payload);
                Ok(())
            },
            Lifecycle::Waiting(waker) => {
                op.state = Lifecycle::Completed(payload);
                waker.wake();
                Ok(())
            },
            Lifecycle::Completed(_) => unreachable!("invalid operation state"),
            Lifecycle::Cancelled(_) => {
                let op = self.ops.remove(id.0);
                Err((payload, op.ext))
            },
        }
    }

    fn remove(&mut self, id: OpId, callback: &mut dyn FnMut() -> Cancellation) {
        // The operation may have been removed inside `poll`.
        let Some(op) = self.ops.get_mut(id.0) else {
            return;
        };
        match mem::replace(&mut op.state, Lifecycle::Submitted) {
            Lifecycle::Submitted | Lifecycle::Waiting(_) => {
                op.state = Lifecycle::Cancelled(callback());
            },
            Lifecycle::Completed(_) => _ = self.ops.remove(id.0),
            Lifecycle::Cancelled(_) => unreachable!("invalid operation state"),
        }
    }
}

pub trait DriverHandle: 'static + Unpin {
    type Payload;
    type Ext;
    type Ref: core::ops::Deref<Target = Driver<Self::Payload, Self::Ext>>;

    fn get(&self) -> Self::Ref;
}
impl<P, Ext> DriverHandle for alloc::rc::Weak<Driver<P, Ext>>
where
    P: 'static,
    Ext: 'static,
{
    type Payload = P;
    type Ext = Ext;
    type Ref = alloc::rc::Rc<Driver<P, Ext>>;
    fn get(&self) -> Self::Ref {
        self.upgrade().expect("not inside a valid executor")
    }
}
