use std::mem::MaybeUninit;
use std::time::Duration;

use evering::driver::OpId;
use evering::op::{Cancellation, Completable};

use crate::reactor::ReactorHandle;
use crate::shm::{ShmBox, ShmToken};

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
    Ping {
        delay: Duration,
        token: ShmToken<[MaybeUninit<u8>]>,
    },
}

#[derive(Debug)]
pub(crate) enum RqeData {
    Exited,
    Pong,
}

pub(crate) struct Ping {
    token: ShmBox<[MaybeUninit<u8>]>,
}

unsafe impl Completable for Ping {
    type Output = ShmBox<[u8]>;
    type Driver = ReactorHandle;
    fn complete(self, _drv: &ReactorHandle, payload: RqeData) -> Self::Output {
        let RqeData::Pong = payload else {
            unreachable!()
        };
        unsafe { self.token.assume_init() }
    }
    fn cancel(self, _drv: &ReactorHandle) -> Cancellation {
        Cancellation::recycle(self.token)
    }
}

pub async fn ping(delay: Duration, token: ShmBox<[MaybeUninit<u8>]>) -> ShmBox<[u8]> {
    ReactorHandle::submit(Ping { token }, |id, p| Sqe {
        id,
        data: SqeData::Ping {
            delay,
            token: ShmBox::as_shm(&p.token),
        },
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
    ReactorHandle::submit(Exit, |id, _| Sqe {
        id,
        data: SqeData::Exit,
    })
    .await
}
