use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::{Rc, Weak};

use evering::driver::OpId;
use evering::op::{Completable, Op};

use crate::op::{Rqe, RqeData, Sqe};

type Sender = evering::uring::Sender<Sqe, Rqe>;
type ReactorInner = evering_utils::reactor::Reactor<RqeData, Sender>;

pub struct Reactor(ManuallyDrop<Rc<ReactorInner>>);

impl Reactor {
    pub fn new(sender: Sender) -> Self {
        Self(ManuallyDrop::new(Rc::new(ReactorInner::new(sender))))
    }

    pub async fn run_on<F: Future>(&self, fut: F) -> F::Output {
        let _guard = ReactorHandle::enter(&self.0);
        let rx = &self.0;
        rx.run_on(|rqe| _ = rx.driver.complete(rqe.id, rqe.data), fut)
            .await
    }

    pub fn into_sender(mut self) -> Sender {
        let rc = unsafe { ManuallyDrop::take(&mut self.0) };
        std::mem::forget(self);
        Rc::into_inner(rc)
            .unwrap_or_else(|| unreachable!("there should not be other strong references"))
            .uring
            .into_inner()
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

impl evering_utils::reactor::ReactorHandle for ReactorHandle {
    type Payload = RqeData;
    type Uring = Sender;
    type Ref = Rc<ReactorInner>;
    fn get(&self) -> Self::Ref {
        CX.with_borrow(Weak::upgrade)
            .expect("not inside a valid reactor")
    }
}

impl evering::driver::DriverHandle for ReactorHandle {
    type Payload = RqeData;
    type Ext = ();
    type Ref = evering_utils::reactor::DriverRef<ReactorHandle>;
    fn get(&self) -> Self::Ref {
        evering_utils::reactor::DriverRef::new(self)
    }
}

impl ReactorHandle {
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

    pub(crate) fn submit<T>(data: T, new_entry: impl FnOnce(OpId, &mut T) -> Sqe) -> Op<T>
    where
        T: Completable<Driver = ReactorHandle>,
    {
        ReactorInner::submit(Self, data, new_entry)
    }
}
