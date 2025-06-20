use std::mem::MaybeUninit;
use std::time::Duration;

use evering::driver::OpId;
use evering::op::{Cancellation, Completable};

use crate::runtime::RuntimeHandle;
use crate::shm::{ShmBox, ShmToken};

#[derive(Debug)]
pub struct Sqe {
    pub id: OpId,
    pub data: SqeData,
}

#[derive(Debug)]
pub struct Rqe {
    pub id: OpId,
    pub data: RqeData,
}

#[derive(Debug)]
pub enum SqeData {
    Exit,
    Ping {
        delay: Duration,
        buf: ShmToken<[MaybeUninit<u8>]>,
    },
}

#[derive(Debug)]
pub enum RqeData {
    Exited,
    Pong,
}

struct Ping {
    buf: ShmBox<[MaybeUninit<u8>]>,
}
unsafe impl Completable for Ping {
    type Output = ShmBox<[u8]>;
    type Driver = RuntimeHandle;
    fn complete(self, _drv: &RuntimeHandle, payload: RqeData) -> Self::Output {
        let RqeData::Pong = payload else {
            unreachable!()
        };
        unsafe { self.buf.assume_init() }
    }
    fn cancel(self, _drv: &RuntimeHandle) -> Cancellation {
        Cancellation::recycle(self.buf)
    }
}

pub async fn ping(delay: Duration, buf: ShmBox<[MaybeUninit<u8>]>) -> ShmBox<[u8]> {
    RuntimeHandle::submit(Ping { buf }, |id, p| Sqe {
        id,
        data: SqeData::Ping {
            delay,
            buf: ShmBox::as_shm(&p.buf),
        },
    })
    .await
}

struct Exit;
unsafe impl Completable for Exit {
    type Output = ();
    type Driver = RuntimeHandle;
    fn complete(self, _drv: &RuntimeHandle, payload: RqeData) -> Self::Output {
        let RqeData::Exited = payload else {
            unreachable!()
        };
    }
    fn cancel(self, _drv: &RuntimeHandle) -> Cancellation {
        Cancellation::noop()
    }
}

pub async fn exit() {
    RuntimeHandle::submit(Exit, |id, _| Sqe {
        id,
        data: SqeData::Exit,
    })
    .await
}
