#![feature(local_waker)]

mod op;
mod reactor;

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use evering::uring::Uring;
use local_executor::Executor;

use self::op::{Rqe, RqeData, Sqe, SqeData};
use self::reactor::Reactor;

fn main() {
    let (sq, mut rq) = evering::uring::Builder::new().build();

    std::thread::scope(|cx| {
        cx.spawn(|| {
            let reactor = Reactor::new(sq);
            let rt = Rc::new(Executor::new());
            rt.block_on(reactor.run_on(async {
                let tasks = (0..10)
                    .map(|i| async move {
                        let now = std::time::Instant::now();
                        let token = op::ping(Duration::from_millis(fastrand::u64(0..500))).await;
                        let elapsed = now.elapsed().as_millis();
                        println!("finished pong({i}) elapsed={elapsed}ms with token={token:#x}");
                    })
                    .map(|fut| local_executor::spawn(Rc::downgrade(&rt), fut))
                    .take(10)
                    .collect::<Vec<_>>();

                for task in tasks {
                    task.await;
                }
                op::exit().await;
                println!("finished exit");
            }));
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
