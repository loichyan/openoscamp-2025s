use alloc::collections::VecDeque;
use core::cell::RefCell;
use core::pin::Pin;
use core::task::{Context, LocalWaker, Poll};

use evering::driver::{Driver, DriverHandle, OpId};
use evering::op::{Completable, Op};
use evering::uring::Uring;
use local_executor::{Executor, ExecutorHandle, Task};

pub struct Runtime<P, U: Uring> {
    pub executor: Executor,
    pub uring: RefCell<U>,
    pub driver: Driver<P, U::Ext>,
    pub pending_submissions: RefCell<VecDeque<LocalWaker>>,
}

impl<P, U: Uring> Runtime<P, U> {
    pub fn new(uring: U) -> Self {
        Self {
            executor: Executor::new(),
            driver: Driver::with_capacity(uring.header().size_a()),
            uring: RefCell::new(uring),
            pending_submissions: RefCell::default(),
        }
    }

    pub fn run_on<C, Fut>(&self, complete: C, fut: Fut) -> RunOn<P, U, C, Fut>
    where
        C: FnMut(U::B),
        Fut: Future,
    {
        RunOn {
            rt: self,
            complete,
            fut,
        }
    }

    pub fn block_on<T>(&self, fut: impl Future<Output = T>) -> T {
        self.executor.block_on(fut)
    }

    pub fn into_uring(self) -> U {
        self.uring.into_inner()
    }

    pub fn spawn<T, F, Rt>(handle: Rt, fut: F) -> Task<T>
    where
        T: 'static,
        F: 'static + Future<Output = T>,
        Rt: RuntimeHandle<Payload = P, Uring = U>,
        Rt: ExecutorHandle,
    {
        Executor::spawn(handle, fut)
    }

    pub async fn submit<T, Rt>(
        handle: Rt,
        data: T,
        new_entry: impl FnOnce(OpId, &mut T) -> U::A,
    ) -> Op<T>
    where
        T: Completable<Driver = Rt>,
        Rt: RuntimeHandle<Payload = P, Uring = U>,
        Rt: DriverHandle<Payload = P, Ext = U::Ext>,
        U::Ext: Default,
    {
        Self::submit_ext(handle, <_>::default(), data, new_entry).await
    }

    pub async fn submit_ext<T, Rt>(
        handle: Rt,
        ext: U::Ext,
        mut data: T,
        new_entry: impl FnOnce(OpId, &mut T) -> U::A,
    ) -> Op<T>
    where
        T: Completable<Driver = Rt>,
        Rt: RuntimeHandle<Payload = P, Uring = U>,
        Rt: DriverHandle<Payload = P, Ext = U::Ext>,
    {
        let rt = RuntimeHandle::get(&handle);
        let id = rt.driver.submit_ext(ext);
        let mut entry = Some(new_entry(id, &mut data));
        core::future::poll_fn(move |cx| {
            let ent = entry.take().unwrap();
            if let Err(ent) = rt.uring.borrow_mut().send(ent) {
                entry = Some(ent);
                rt.pending_submissions
                    .borrow_mut()
                    .push_back(cx.local_waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        Op::new(handle, id, data)
    }
}

pin_project_lite::pin_project! {
    pub struct RunOn<'a,P, U, C, Fut>
    where
        U: Uring,
    {
        rt: &'a Runtime<P, U>,
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
        for ent in this.rt.uring.borrow_mut().recv_bulk() {
            _ = this
                .rt
                .pending_submissions
                .borrow_mut()
                .pop_front()
                .map(LocalWaker::wake);
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

pub trait RuntimeHandle: 'static + Unpin {
    type Payload;
    type Uring: Uring;
    type Ref: core::ops::Deref<Target = Runtime<Self::Payload, Self::Uring>>;

    fn get(&self) -> Self::Ref;
}
impl<P, U> RuntimeHandle for alloc::rc::Weak<Runtime<P, U>>
where
    P: 'static,
    U: 'static + Uring,
{
    type Payload = P;
    type Uring = U;
    type Ref = alloc::rc::Rc<Runtime<P, U>>;

    fn get(&self) -> Self::Ref {
        self.upgrade().expect("not inside a valid executor")
    }
}

pub struct ExecutorRef<R: RuntimeHandle>(pub R::Ref);
impl<Rt: RuntimeHandle> ExecutorRef<Rt> {
    pub fn new(rt: &Rt) -> Self {
        Self(rt.get())
    }
}
impl<Rt: RuntimeHandle> core::ops::Deref for ExecutorRef<Rt> {
    type Target = Executor;

    fn deref(&self) -> &Self::Target {
        &self.0.executor
    }
}

pub struct DriverRef<R: RuntimeHandle>(pub R::Ref);
impl<Rt: RuntimeHandle> DriverRef<Rt> {
    pub fn new(rt: &Rt) -> Self {
        Self(rt.get())
    }
}
impl<Rt: RuntimeHandle> core::ops::Deref for DriverRef<Rt> {
    type Target = Driver<Rt::Payload, <Rt::Uring as Uring>::Ext>;

    fn deref(&self) -> &Self::Target {
        &self.0.driver
    }
}
