use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

use evering::driver::{Driver, OpId};
use evering::op::Completable;
use evering::uring::{Sender, Uring};

use crate::op::{Op, Rqe, RqeData, Sqe};

pub struct Reactor(ManuallyDrop<Rc<ReactorInner>>);

struct ReactorInner {
    sender: RefCell<Sender<Sqe, Rqe>>,
    driver: Driver<RqeData>,
}

impl Reactor {
    pub fn new(sender: Sender<Sqe, Rqe>) -> Self {
        Self(ManuallyDrop::new(Rc::new(ReactorInner {
            sender: RefCell::new(sender),
            driver: Driver::new(),
        })))
    }

    pub async fn run_on<F: Future>(&self, fut: F) -> F::Output {
        let _guard = ReactorHandle::enter(&self.0);
        let rx = &self.0;

        let mut fut = std::pin::pin!(fut);
        let mut noop_cx = Context::from_waker(std::task::Waker::noop());
        std::future::poll_fn(move |cx| {
            while let Some(rqe) = { rx.sender.borrow_mut().recv() } {
                _ = rx.driver.complete(rqe.id, rqe.data);
            }
            match fut.as_mut().poll(&mut noop_cx) {
                // Always wake ourself if pending as the given `Future` may wait
                // us to wake it, which leads to a circular waiting chain.
                Poll::Pending => {
                    cx.local_waker().wake_by_ref();
                    Poll::Pending
                },
                ready => ready,
            }
        })
        .await
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        let rc = unsafe { ManuallyDrop::take(&mut self.0) };
        // TODO: should wait instead?
        if !rc.driver.is_empty() {
            std::mem::forget(rc);
        }
    }
}

thread_local! {
    static CX: RefCell<Weak<ReactorInner>> = const { RefCell::new(Weak::new()) };
}

pub(crate) struct ReactorHandle;

impl ReactorHandle {
    fn get() -> Rc<ReactorInner> {
        CX.with_borrow(Weak::upgrade)
            .expect("not inside a valid reactor")
    }

    fn enter(cx: &Rc<ReactorInner>) -> impl Drop {
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                CX.with_borrow_mut(|d| *d = Weak::new())
            }
        }
        CX.with_borrow_mut(|d| {
            if d.strong_count() != 0 {
                panic!("cannot run within a nested reactor")
            }
            *d = Rc::downgrade(cx)
        });
        Revert
    }

    pub(crate) fn submit<T>(f: impl FnOnce(OpId) -> (Op<T>, Sqe)) -> Op<T>
    where
        T: Completable,
    {
        let rt = ReactorHandle::get();
        let (op, sqe) = f(rt.driver.submit());
        rt.sender.borrow_mut().send(sqe).expect("out of capacity");
        op
    }
}

impl evering::driver::DriverHandle for ReactorHandle {
    type Payload = RqeData;
    type Ext = ();
    type Ref = DriverRef;
    fn get(&self) -> Self::Ref {
        DriverRef(ReactorHandle::get())
    }
}

pub(crate) struct DriverRef(Rc<ReactorInner>);
impl std::ops::Deref for DriverRef {
    type Target = Driver<RqeData>;
    fn deref(&self) -> &Self::Target {
        &self.0.driver
    }
}
