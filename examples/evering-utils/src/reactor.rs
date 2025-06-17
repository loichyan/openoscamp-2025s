use core::cell::RefCell;
use core::pin::Pin;
use core::task::{Context, Poll};

use evering::driver::{Driver, DriverHandle, OpId};
use evering::op::{Completable, Op};
use evering::uring::Uring;

#[non_exhaustive]
pub struct Reactor<P, U: Uring> {
    pub uring: RefCell<U>,
    pub driver: Driver<P, U::Ext>,
}

impl<P, U: Uring> Reactor<P, U> {
    pub fn new(uring: U) -> Self {
        Self {
            uring: RefCell::new(uring),
            driver: Driver::new(),
        }
    }

    pub fn run_on<C, Fut>(&self, complete: C, fut: Fut) -> RunOn<P, U, C, Fut>
    where
        C: FnMut(U::B),
        Fut: Future,
    {
        RunOn {
            rx: self,
            complete,
            fut,
        }
    }

    pub fn submit<T, Rx>(handle: Rx, data: T, new_entry: impl FnOnce(OpId, &mut T) -> U::A) -> Op<T>
    where
        T: Completable<Driver = Rx>,
        Rx: ReactorHandle<Payload = P, Uring = U>,
        Rx: DriverHandle<Payload = P, Ext = U::Ext>,
        U::Ext: Default,
    {
        Self::submit_ext(handle, <_>::default(), data, new_entry)
    }

    pub fn submit_ext<T, Rx>(
        handle: Rx,
        ext: U::Ext,
        mut data: T,
        new_entry: impl FnOnce(OpId, &mut T) -> U::A,
    ) -> Op<T>
    where
        T: Completable<Driver = Rx>,
        Rx: ReactorHandle<Payload = P, Uring = U>,
        Rx: DriverHandle<Payload = P, Ext = U::Ext>,
    {
        let rx = ReactorHandle::get(&handle);
        let id = rx.driver.submit_ext(ext);
        let ent = new_entry(id, &mut data);
        rx.uring
            .borrow_mut()
            .send(ent)
            // TODO: queue entries locally
            .unwrap_or_else(|_| panic!("out of capacity"));
        Op::new(handle, id, data)
    }
}

pin_project_lite::pin_project! {
    pub struct RunOn<'a,P, U, C, Fut>
    where
        U: Uring,
    {
        rx: &'a Reactor<P, U>,
        complete:C,
        #[pin]
        fut: Fut,
    }
}

impl<'a, P, U, C, Fut> Future for RunOn<'a, P, U, C, Fut>
where
    U: Uring,
    C: FnMut(U::B),
    Fut: Future,
{
    type Output = Fut::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        for ent in this.rx.uring.borrow_mut().recv_bulk() {
            (this.complete)(ent);
        }
        let mut noop_cx = Context::from_waker(core::task::Waker::noop());
        match this.fut.as_mut().poll(&mut noop_cx) {
            // Always wake ourself if pending as the given `Future` may wait us
            // to wake it, which leads to a circular waiting chain.
            Poll::Pending => {
                cx.local_waker().wake_by_ref();
                Poll::Pending
            },
            ready => ready,
        }
    }
}

pub trait ReactorHandle: 'static + Unpin {
    type Payload;
    type Uring: Uring;
    type Ref: core::ops::Deref<Target = Reactor<Self::Payload, Self::Uring>>;

    fn get(&self) -> Self::Ref;
}
impl<P, U> ReactorHandle for alloc::rc::Weak<Reactor<P, U>>
where
    P: 'static,
    U: 'static + Uring,
{
    type Payload = P;
    type Uring = U;
    type Ref = alloc::rc::Rc<Reactor<P, U>>;

    fn get(&self) -> Self::Ref {
        self.upgrade().expect("not inside a valid executor")
    }
}

pub struct DriverRef<R: ReactorHandle>(pub R::Ref);
impl<Rx: ReactorHandle> DriverRef<Rx> {
    pub fn new(rx: &Rx) -> Self {
        Self(rx.get())
    }
}
impl<Rx: ReactorHandle> core::ops::Deref for DriverRef<Rx> {
    type Target = Driver<Rx::Payload, <Rx::Uring as Uring>::Ext>;

    fn deref(&self) -> &Self::Target {
        &self.0.driver
    }
}
