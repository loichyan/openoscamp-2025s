pub mod op;
pub mod runtime;
pub mod shm;

use std::time::Duration;

pub use anyhow::{Error, Result};
use evering::uring;
use evering::uring::Uring;

use self::op::{Rqe, RqeData, Sqe, SqeData};

pub type ClientUring = uring::UringA<Sqe, Rqe>;
pub type ServerUring = uring::UringB<Sqe, Rqe>;
pub type UringBuilder = uring::Builder<Sqe, Rqe>;
pub type ShmHeader = shm::ShmHeader<Sqe, Rqe>;

use self::runtime::{Runtime, RuntimeHandle};
use self::shm::ShmBox;

pub fn start_client(shm: &'static ShmHeader) -> bool {
    crate::shm::init_client(shm);
    let sq = unsafe { ClientUring::from_raw(shm.build_raw_uring()) };
    tracing::info!("started client, connected={}", sq.is_connected());

    let rt = Runtime::new(sq);
    rt.block_on(async {
        let tasks = (0..16)
            .map(|i| async move {
                let delay = fastrand::u64(50..500);
                tracing::info!("requested ping({i}), delay={delay:?}ms");

                let delay = fastrand::u64(0..500);
                let token = ShmBox::new_uninit_slice(fastrand::usize(8..=32));

                let now = std::time::Instant::now();
                let token = op::ping(Duration::from_millis(delay), token).await;
                let elapsed = now.elapsed().as_millis();

                let token_str = std::str::from_utf8(&token).unwrap();
                tracing::info!("responded pong({i}), elapsed={elapsed}ms, token={token_str}");
            })
            .map(RuntimeHandle::spawn)
            .collect::<Vec<_>>();

        for task in tasks {
            task.await;
        }
        op::exit().await;
        tracing::info!("exited client");
    });

    rt.into_sender().dispose_raw().is_ok()
}

pub fn start_server(shm: &'static ShmHeader) -> bool {
    crate::shm::init_server(shm);
    let mut rq = unsafe { ServerUring::from_raw(shm.build_raw_uring()) };
    tracing::info!("started server, connected={}", rq.is_connected());

    let mut local_queue = Vec::new();
    loop {
        let mut should_exit = false;
        if let Some(Sqe { id, data }) = rq.recv() {
            tracing::info!("accepted request, data={data:x?}");
            let data = match data {
                SqeData::Exit => {
                    should_exit = true;
                    RqeData::Exited
                },
                SqeData::Ping { delay, token } => {
                    std::thread::sleep(delay);
                    unsafe {
                        let mut token = token.as_ptr();
                        for c in token.as_mut().iter_mut() {
                            c.write(fastrand::alphanumeric() as u32 as u8);
                        }
                    }
                    RqeData::Pong
                },
            };
            local_queue.push(Rqe { id, data });
        }

        if local_queue.is_empty() {
            std::thread::yield_now();
        } else if should_exit || fastrand::bool() {
            // Randomize the returned response
            fastrand::shuffle(&mut local_queue);
            for rqe in local_queue.drain(..) {
                tracing::info!("replied response, data={:x?}", rqe.data);
                rq.send(rqe).expect("out of capacity");
            }
        }

        if should_exit {
            tracing::info!("exited server");
            break;
        }
    }

    rq.dispose_raw().is_ok()
}
