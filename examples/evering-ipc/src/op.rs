use std::mem::MaybeUninit;

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
        ping: i32,
        buf: ShmToken<[MaybeUninit<u8>]>,
    },
}

#[derive(Debug)]
pub enum RqeData {
    Exited,
    Pong { pong: i32 },
}

struct Ping {
    buf: ShmBox<[MaybeUninit<u8>]>,
}
unsafe impl Completable for Ping {
    type Output = (i32, ShmBox<[u8]>);
    type Driver = RuntimeHandle;
    fn complete(self, _drv: &RuntimeHandle, payload: RqeData) -> Self::Output {
        let RqeData::Pong { pong } = payload else {
            unreachable!()
        };
        (pong, unsafe { self.buf.assume_init() })
    }
    fn cancel(self, _drv: &RuntimeHandle) -> Cancellation {
        Cancellation::recycle(self.buf)
    }
}

pub async fn ping(ping: i32, buf: ShmBox<[MaybeUninit<u8>]>) -> (i32, ShmBox<[u8]>) {
    RuntimeHandle::submit(Ping { buf }, |id, p| Sqe {
        id,
        data: SqeData::Ping {
            ping,
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
