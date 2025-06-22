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

#[macro_use]
mod utils;
mod evering;
mod monoio_uring;
mod shmipc;
mod tokio_epoll;
mod tokio_uring;

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use bytes::Bytes;
use bytesize::ByteSize;
use criterion::{Criterion, criterion_group, criterion_main};

const BUFSIZES: &[usize] = &[
    4,
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
    4 << 20,
];
const CONCURRENCY: usize = 200;

// Fixed constants
const PING: i32 = 1;
const PONG: i32 = 2;

static PONGDATA: &[u8] = PONG.to_be_bytes().as_slice();
static PINGDATA: &[u8] = PING.to_be_bytes().as_slice();

type BenchFn = fn(&str, usize, usize) -> Duration;

const fn shmsize(bufsize: usize) -> usize {
    if bufsize < 4 << 20 {
        256 << 20
    } else {
        1 << 30
    }
}

fn tokio_block_on_current<T>(fut: impl Future<Output = T>) -> T {
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

fn check_reqdata(bufsize: usize, req: &[u8]) {
    check_bufdata(bufsize, req, b'S');
}

fn make_reqdata(bufsize: usize) -> Bytes {
    make_bufdata(bufsize, b'S')
}

fn make_respdata(bufsize: usize) -> Bytes {
    make_bufdata(bufsize, b'R')
}

fn check_respdata(bufsize: usize, resp: &[u8]) {
    check_bufdata(bufsize, resp, b'R');
}

fn check_bufdata(bufsize: usize, resp: &[u8], expected: u8) {
    assert_eq!(resp.len(), bufsize);
    // Pick a few bytes to check. Checking all bytes is meaningless and will
    // significantly slow down the benchmark.
    for _ in 0..(32.min(bufsize)) {
        let b = *fastrand::choice(resp).unwrap();
        assert_eq!(black_box(b), black_box(expected));
    }
}

/// Returns arbitrary response data.
fn make_bufdata(bufsize: usize, expected: u8) -> Bytes {
    // Black boxed to mock runtime values
    black_box(Bytes::from(vec![black_box(expected); bufsize]))
}

fn groups(c: &mut Criterion) {
    macro_rules! benches {
        ($($name:ident),* $(,)?) => ([$((stringify!($name), self::$name::bench as BenchFn),)*]);
    }

    let mut g = c.benchmark_group("ipc_benchmark");
    for (i, bufsize) in BUFSIZES.iter().copied().enumerate() {
        let bsize = ByteSize::b(bufsize as u64).display().iec_short();
        for (name, f) in benches![evering, monoio_uring, shmipc, tokio_epoll, tokio_uring] {
            let id = format!("ipc_benchmark_{i:02}_{bsize:.0}_{name}");
            g.bench_function(&id, |b| {
                b.iter_custom(|iters| f(&id, iters as usize, bufsize))
            });
        }
    }
}

criterion_group!(
    name = ipc_benchmark;
    // TODO: increase sample size
    config = Criterion::default().sample_size(50).measurement_time(Duration::from_secs(30));
    targets = groups
);
criterion_main!(ipc_benchmark);
