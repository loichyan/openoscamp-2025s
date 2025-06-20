// Credit: https://github.com/cloudwego/shmipc-rs/blob/de966a6ca2d76d574b943f6fd4d3abfa6ff2df5f/benches/bench.rs
//
// Copyright 2025 CloudWeGo Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::Once;
use std::time::{Duration, Instant};

use bytesize::ByteSize;
use criterion::{Criterion, criterion_group, criterion_main};
use tokio::task::spawn_local;

const BUFSIZES: &[usize] = &[
    64,
    512,
    1024,
    4096,
    16 << 10,
    32 << 10,
    64 << 10,
    256 << 10,
    512 << 10,
    1 << 20,
];
const CONCURRENCY: usize = 200;
const SHMSIZE: usize = 1 << 30;

fn block_on<T>(fut: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&rt, fut)
}

fn make_shmid(pref: &str) -> String {
    pref.chars()
        .chain(std::iter::repeat_with(fastrand::alphanumeric).take(6))
        .collect()
}

fn bench_evering(id: &str, iters: usize, bufsize: usize) -> Duration {
    use evering::uring::Uring;
    use evering_ipc::*;

    let shmid = make_shmid(id);
    let shmid = CString::new(shmid).unwrap();
    let shmfd_owned =
        nix::sys::memfd::memfd_create(shmid.as_c_str(), nix::sys::memfd::MFdFlags::empty())
            .expect("failed to create shared memory");
    let shmfd = shmfd_owned.as_fd();

    let mut elapsed = Duration::ZERO;
    let signal = Once::new();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            let (shm, mut rq);
            unsafe {
                let mut h = UringBuilder::new();
                h.size_a(CONCURRENCY.next_power_of_two());
                h.size_b(CONCURRENCY.next_power_of_two());
                let h = h.build_header();
                shm = ShmHeader::create(shmfd, SHMSIZE, h).unwrap();
                rq = ServerUring::from_raw(shm.as_ref().build_raw_uring());
                evering_ipc::shm::init_server(shm.as_ref());
            }
            signal.call_once(|| {});

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
                        SqeData::Ping { delay: _, buf } => {
                            assert_eq!(buf.as_ptr().len(), bufsize);
                            unsafe { buf.as_ptr().as_mut().fill(MaybeUninit::new(0)) }
                            RqeData::Pong
                        },
                    };
                    if let Err(p) = rq.send(Rqe { id, data }) {
                        pending = Some(p);
                        break;
                    }
                }
            }

            _ = rq.dispose_raw();
            unsafe { ShmHeader::close(shm, SHMSIZE).unwrap() }
        });
        // Client
        cx.spawn(|| {
            signal.wait();
            let (shm, sq);
            unsafe {
                shm = ShmHeader::open(shmfd, SHMSIZE).unwrap();
                sq = ClientUring::from_raw(shm.as_ref().build_raw_uring());
                evering_ipc::shm::init_client(shm.as_ref());
            }

            let rx = evering_ipc::Runtime::new(sq);
            block_on(rx.run_on(async {
                let tasks = std::iter::repeat_with(|| async move {
                    let mut rbuf = ShmBox::new_uninit_slice(bufsize);
                    for _ in 0..(iters / CONCURRENCY) {
                        let rbuf_init = evering_ipc::op::ping(Duration::ZERO, rbuf).await;
                        assert!(rbuf_init.iter().all(|b| *b == 0));
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
            unsafe { ShmHeader::close(shm, SHMSIZE).unwrap() }
        });
    });

    nix::unistd::close(shmfd_owned).expect("failed to remove shared memory");
    elapsed
}

fn bench_epoll(id: &str, iters: usize, bufsize: usize) -> Duration {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    const PING: i32 = 1;
    const PONG: i32 = 1;

    let sock = Path::new("/dev/shm").join(make_shmid(id));

    let mut elapsed = Duration::ZERO;
    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<()>();
    let (exit_tx, mut exit_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            block_on(async {
                let listener = UnixListener::bind(&sock).unwrap();
                signal_tx.send(()).unwrap();
                let worker = |mut conn: UnixStream| async move {
                    let wbuf = vec![0; bufsize];
                    loop {
                        match conn.read_i32().await {
                            Ok(i) => {
                                assert_eq!(i, PING);
                                conn.write_i32(PONG).await.unwrap();
                                conn.write_all(&wbuf).await.unwrap();
                                conn.flush().await.unwrap();
                            },
                            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                return;
                            },
                            Err(e) => panic!("{e}"),
                        }
                    }
                };
                loop {
                    tokio::select! {
                        r = listener.accept() => {
                            let (conn, _) = r.unwrap();
                            spawn_local(worker(conn));
                        },
                        _ = &mut exit_rx =>  break,
                    }
                }
            });
        });
        // Client
        cx.spawn(|| {
            block_on(async {
                signal_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    async move {
                        let mut conn = UnixStream::connect(sock).await.unwrap();
                        let mut rbuf = vec![0; bufsize];
                        for _ in 0..(iters / CONCURRENCY) {
                            conn.write_i32(PING).await.unwrap();
                            conn.flush().await.unwrap();
                            assert_eq!(conn.read_i32().await.unwrap(), PONG);
                            conn.read_exact(&mut rbuf).await.unwrap();
                            assert!(rbuf.iter().all(|b| *b == 0));
                        }
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
                exit_tx.send(()).unwrap();
            });
        });
    });

    _ = std::fs::remove_file(sock);
    elapsed
}

fn groups(c: &mut Criterion) {
    type BenchFn = fn(&str, usize, usize) -> Duration;
    let mut g = c.benchmark_group("evering");
    for (i, bufsize) in BUFSIZES.iter().copied().enumerate() {
        let bsize = ByteSize::b(bufsize as u64).display().iec_short();
        for (name, f) in [
            ("evering", bench_evering as BenchFn),
            ("epoll", bench_epoll as BenchFn),
        ] {
            let id = format!("ping_pong_{name}_{:02}_{bsize:.0}", i + 1);
            g.bench_function(&id, |b| {
                b.iter_custom(|iters| f(&id, iters as usize, bufsize))
            });
        }
    }
}

criterion_group!(ping_pong, groups);
criterion_main!(ping_pong);
