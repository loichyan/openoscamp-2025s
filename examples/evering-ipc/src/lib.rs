pub mod op;
pub mod runtime;
pub mod shm;

pub use anyhow::{Error, Result};
use evering::uring;

pub use self::op::{Rqe, RqeData, Sqe, SqeData};
pub use self::runtime::{Runtime, RuntimeHandle};
pub use self::shm::{ShmBox, ShmToken};

pub type ClientUring = uring::UringA<Sqe, Rqe>;
pub type ServerUring = uring::UringB<Sqe, Rqe>;
pub type UringBuilder = uring::Builder<Sqe, Rqe>;
pub type ShmHeader = shm::ShmHeader<Sqe, Rqe>;
