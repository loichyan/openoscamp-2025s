#![feature(local_waker)]

mod op;
mod runtime;

use std::collections::VecDeque;
use std::time::Duration;

use evering::uring::Uring;

use self::op::{Rqe, RqeData, Sqe, SqeData};
use self::runtime::{Runtime, RuntimeHandle};

fn main() {
    let (sq, mut rq) = evering::uring::Builder::new().build();

    std::thread::scope(|cx| {
        cx.spawn(|| {
            let rt = Runtime::new(sq);
            rt.block_on(async {
                let tasks = (0..)
                    .map(|i| async move {
                        let now = std::time::Instant::now();
                        let token = op::ping(Duration::from_millis(fastrand::u64(0..500))).await;
                        let elapsed = now.elapsed().as_millis();
                        println!("finished pong({i}) elapsed={elapsed}ms with token={token:#x}");
                    })
                    .map(RuntimeHandle::spawn)
                    .take(fastrand::usize(32..=64))
                    .collect::<Vec<_>>();

                for task in tasks {
                    task.await;
                }
                op::exit().await;
                println!("finished exit");
            });
            drop(rt.into_sender());
        });
        cx.spawn(|| {
            let mut local_queue = VecDeque::new();
            loop {
                let mut should_exit = false;
                if let Some(Sqe { id, data }) = rq.recv() {
                    println!("accepted task {data:?}");
                    let data = match data {
                        SqeData::Exit => {
                            should_exit = true;
                            RqeData::Exited
                        },
                        SqeData::Ping { delay } => {
                            std::thread::sleep(delay);
                            RqeData::Pong {
                                token: fastrand::u64(..),
                            }
                        },
                    };
                    if fastrand::bool() {
                        local_queue.push_back(Rqe { id, data });
                    } else {
                        local_queue.push_front(Rqe { id, data });
                    }
                }

                if local_queue.is_empty() {
                    std::thread::yield_now();
                } else if should_exit || fastrand::bool() {
                    for rqe in local_queue.drain(..) {
                        rq.send(rqe).expect("out of capacity");
                    }
                }

                if should_exit {
                    break;
                }
            }
        });
    });
}
