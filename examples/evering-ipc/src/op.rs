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
        req: ShmToken<[u8]>,
        resp: ShmToken<[MaybeUninit<u8>]>,
    },
}

#[derive(Debug)]
pub enum RqeData {
    Exited,
    Pong { pong: i32 },
}

struct Ping {
    req: ShmBox<[u8]>,
    resp: ShmBox<[MaybeUninit<u8>]>,
}
pub struct Pong {
    pub pong: i32,
    pub req: ShmBox<[u8]>,
    pub resp: ShmBox<[u8]>,
}
unsafe impl Completable for Ping {
    type Output = Pong;
    type Driver = RuntimeHandle;
    fn complete(self, _drv: &RuntimeHandle, payload: RqeData) -> Self::Output {
        let RqeData::Pong { pong } = payload else {
            unreachable!()
        };
        Pong {
            pong,
            req: self.req,
            resp: unsafe { self.resp.assume_init() },
        }
    }
    fn cancel(self, _drv: &RuntimeHandle) -> Cancellation {
        Cancellation::recycle((self.req, self.resp))
    }
}

pub async fn ping(ping: i32, req: ShmBox<[u8]>, resp: ShmBox<[MaybeUninit<u8>]>) -> Pong {
    RuntimeHandle::submit(Ping { req, resp }, |id, p| Sqe {
        id,
        data: SqeData::Ping {
            ping,
            req: ShmBox::as_shm(&p.req),
            resp: ShmBox::as_shm(&p.resp),
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
