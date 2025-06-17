use std::time::Duration;

use evering::driver::OpId;
use evering::op::{Cancellation, Completable};

use crate::runtime::RuntimeHandle;

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
    type Driver = RuntimeHandle;
    fn complete(self, _drv: &RuntimeHandle, payload: RqeData) -> Self::Output {
        let RqeData::Pong { token } = payload else {
            unreachable!()
        };
        token
    }
    fn cancel(self, _drv: &RuntimeHandle) -> Cancellation {
        Cancellation::noop()
    }
}

pub async fn ping(delay: Duration) -> u64 {
    RuntimeHandle::submit(Ping, |id, _| Sqe {
        id,
        data: SqeData::Ping { delay },
    })
    .await
}

pub(crate) struct Exit;

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
