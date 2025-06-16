use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use evering::driver::OpId;
use evering::op::{Cancellation, Completable, Op as RawOp};

use crate::reactor::ReactorHandle;

pub(crate) struct Op<T: Completable>(RawOp<T>);

impl<T> Op<T>
where
    T: Completable<Driver = ReactorHandle>,
{
    pub(crate) fn new(id: OpId, data: T) -> Self {
        Self(RawOp::new(ReactorHandle, id, data))
    }
}

impl<T> Future for Op<T>
where
    T: Completable<Driver = ReactorHandle>,
{
    type Output = T::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().0).poll(cx)
    }
}

#[derive(Debug)]
pub(crate) struct Sqe {
    pub id: OpId,
    pub data: SqeData,
}

#[derive(Debug)]
pub(crate) struct Rqe {
    pub id: OpId,
    pub data: RqeData,
}

#[derive(Debug)]
pub(crate) enum SqeData {
    Exit,
    Ping { delay: Duration },
}

#[derive(Debug)]
pub(crate) enum RqeData {
    Exited,
    Pong { token: u64 },
}

pub(crate) struct Ping;

unsafe impl Completable for Ping {
    type Output = u64;
    type Driver = ReactorHandle;
    fn complete(self, _drv: &ReactorHandle, payload: RqeData) -> Self::Output {
        let RqeData::Pong { token } = payload else {
            unreachable!()
        };
        token
    }
    fn cancel(self, _drv: &ReactorHandle) -> Cancellation {
        Cancellation::noop()
    }
}

pub async fn ping(delay: Duration) -> u64 {
    ReactorHandle::submit(|id| {
        (Op::new(id, Ping), Sqe {
            id,
            data: SqeData::Ping { delay },
        })
    })
    .await
}

pub(crate) struct Exit;

unsafe impl Completable for Exit {
    type Output = ();
    type Driver = ReactorHandle;
    fn complete(self, _drv: &ReactorHandle, payload: RqeData) -> Self::Output {
        let RqeData::Exited = payload else {
            unreachable!()
        };
    }
    fn cancel(self, _drv: &ReactorHandle) -> Cancellation {
        Cancellation::noop()
    }
}

pub async fn exit() {
    ReactorHandle::submit(|id| {
        (Op::new(id, Exit), Sqe {
            id,
            data: SqeData::Exit,
        })
    })
    .await
}
