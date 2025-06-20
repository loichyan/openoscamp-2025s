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
use std::time::{Duration, Instant};

use bytesize::ByteSize;
use criterion::{Criterion, criterion_group, criterion_main};

const BUFSIZES: &[u64] = &[
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

fn bench_evering(id: &str, iters: u64, bufsize: u64) -> Duration {
    use evering::uring::Uring;
    use evering_ipc::*;

    let bufsize = usize::try_from(bufsize).unwrap();
    let shmid = id
        .chars()
        .chain(std::iter::repeat_with(fastrand::alphanumeric).take(6))
        .collect::<String>();
    let shmid = CString::new(shmid).unwrap();
    let shmfd_owned = {
        nix::sys::memfd::memfd_create(shmid.as_c_str(), nix::sys::memfd::MFdFlags::empty())
            .expect("failed to create shared memory")
    };

    let shmfd = shmfd_owned.as_fd();
    let shm = unsafe {
        let mut h = UringBuilder::new();
        h.size_a(CONCURRENCY.next_power_of_two());
        h.size_b(CONCURRENCY.next_power_of_two());
        let h = h.build_header();
        ShmHeader::create(shmfd, SHMSIZE, h).unwrap()
    };

    let mut elapsed = Duration::ZERO;
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            let (shm, mut rq);
            unsafe {
                shm = ShmHeader::open(shmfd, SHMSIZE).unwrap();
                rq = ServerUring::from_raw(shm.as_ref().build_raw_uring());
                evering_ipc::shm::init_server(shm.as_ref());
            }

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
            let (shm, sq);
            unsafe {
                shm = ShmHeader::open(shmfd, SHMSIZE).unwrap();
                sq = ClientUring::from_raw(shm.as_ref().build_raw_uring());
                evering_ipc::shm::init_client(shm.as_ref());
            }

            let rt = evering_ipc::Runtime::new(sq);
            rt.block_on(async {
                let tasks = std::iter::repeat_with(|| async move {
                    let mut rbuf = ShmBox::new_uninit_slice(bufsize);
                    for _ in 0..(iters as usize / CONCURRENCY) {
                        let rbuf_init = evering_ipc::op::ping(Duration::ZERO, rbuf).await;
                        assert!(rbuf_init.iter().all(|b| *b == 0));
                        rbuf = rbuf_init.into_uninit();
                    }
                })
                .map(RuntimeHandle::spawn)
                .take(CONCURRENCY)
                .collect::<Vec<_>>();

                let now = Instant::now();
                for task in tasks {
                    task.await;
                }
                elapsed = now.elapsed();
                evering_ipc::op::exit().await;
            });

            _ = rt.into_uring().dispose_raw();
            unsafe { ShmHeader::close(shm, SHMSIZE).unwrap() }
        });
    });

    unsafe { ShmHeader::close(shm, SHMSIZE).unwrap() }
    nix::unistd::close(shmfd_owned).expect("failed to remove shared memory");
    elapsed
}

fn groups(c: &mut Criterion) {
    let mut g = c.benchmark_group("evering");
    for (i, bufsize) in BUFSIZES.iter().copied().enumerate() {
        let bsize = ByteSize::b(bufsize).display().iec_short();
        let id = format!("ping_pong_evering_{:02}_{bsize:.0}", i + 1);
        g.bench_function(&id, |b| {
            b.iter_custom(|iters| bench_evering(&id, iters, bufsize))
        });
    }
}

criterion_group!(ping_pong, groups);
criterion_main!(ping_pong);
