use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::sync::Once;

use ::evering::uring::Uring;
use evering_ipc::*;
use tokio::task::spawn_local;

use super::*;

pub fn bench(id: &str, iters: usize, bufsize: usize) -> Duration {
    let shmid = make_shmid(id);
    let shmid = CString::new(shmid).unwrap();
    let shmfd_owned =
        nix::sys::memfd::memfd_create(shmid.as_c_str(), nix::sys::memfd::MFdFlags::empty())
            .expect("failed to create shared memory");
    let shmfd = shmfd_owned.as_fd();
    let shmsize = shmsize(bufsize);

    let mut elapsed = Duration::ZERO;
    let started = Once::new();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            let resp = black_box(vec![MaybeUninit::new(BUFVAL); bufsize]);

            let (shm, mut rq);
            unsafe {
                let mut h = UringBuilder::new();
                h.size_a(CONCURRENCY.next_power_of_two());
                h.size_b(CONCURRENCY.next_power_of_two());
                let h = h.build_header();
                shm = ShmHeader::create(shmfd, shmsize, h).unwrap();
                rq = ServerUring::from_raw(shm.as_ref().build_raw_uring());
                evering_ipc::shm::init_server(shm.as_ref());
            }
            started.call_once(|| {});

            let mut pending = None::<Rqe>;
            'outer: loop {
                if let Some(p) = pending.take() {
                    if let Err(p) = rq.send(p) {
                        pending = Some(p);
                        std::thread::sleep(Duration::from_micros(1));
                        continue;
                    }
                }

                while let Some(Sqe { id, data }) = rq.recv() {
                    let data = match data {
                        SqeData::Exit => {
                            rq.send(Rqe {
                                id,
                                data: RqeData::Exited,
                            })
                            .unwrap();
                            break 'outer;
                        },
                        SqeData::Ping { ping, buf } => {
                            assert_eq!(ping, PING);
                            assert_eq!(buf.as_ptr().len(), bufsize);
                            unsafe { buf.as_ptr().as_mut().copy_from_slice(&resp) }
                            RqeData::Pong { pong: PONG }
                        },
                    };
                    if let Err(p) = rq.send(Rqe { id, data }) {
                        pending = Some(p);
                        break;
                    }
                }
            }

            _ = rq.dispose_raw();
            unsafe { ShmHeader::close(shm, shmsize).unwrap() }
        });
        // Client
        cx.spawn(|| {
            started.wait();
            let (shm, sq);
            unsafe {
                shm = ShmHeader::open(shmfd, shmsize).unwrap();
                sq = ClientUring::from_raw(shm.as_ref().build_raw_uring());
                evering_ipc::shm::init_client(shm.as_ref());
            }

            let rx = evering_ipc::Runtime::new(sq);
            block_on(rx.run_on(async {
                let tasks = std::iter::repeat_with(|| async move {
                    let mut rbuf = ShmBox::new_uninit_slice(bufsize);
                    for _ in 0..(iters / CONCURRENCY) {
                        let (pong, rbuf_init) = evering_ipc::op::ping(PING, rbuf).await;
                        assert_eq!(pong, PONG);
                        assert!(check_resp(bufsize, &rbuf_init));
                        rbuf = rbuf_init.into_uninit();
                    }
                })
                .map(spawn_local)
                .take(CONCURRENCY)
                .collect::<Vec<_>>();

                let now = Instant::now();
                for task in tasks {
                    task.await.unwrap();
                }
                elapsed = now.elapsed();
                evering_ipc::op::exit().await;
            }));

            _ = rx.into_uring().dispose_raw();
            unsafe { ShmHeader::close(shm, shmsize).unwrap() }
        });
    });

    nix::unistd::close(shmfd_owned).expect("failed to remove shared memory");
    elapsed
}
